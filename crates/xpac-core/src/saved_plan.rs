use crate::{identity, Error, PackageId, Plan, ProviderId};
use serde::{Deserialize, Serialize};

pub const MAX_BYTES: usize = 16 * 1024 * 1024;
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedPlan {
    pub version: u32,
    pub provider: ProviderId,
    pub created_unix_secs: u64,
    pub expires_unix_secs: u64,
    pub install: Vec<PackageId>,
    pub remove: Vec<PackageId>,
    pub plan: Plan,
}
impl SavedPlan {
    pub fn new(
        provider: ProviderId,
        install: Vec<PackageId>,
        remove: Vec<PackageId>,
        plan: Plan,
        now: u64,
        ttl_secs: u64,
    ) -> Result<Self, Error> {
        let expires_unix_secs = now
            .checked_add(ttl_secs)
            .ok_or_else(|| Error::InvalidInput("plan expiration overflow".into()))?;
        let saved = Self {
            version: 1,
            provider,
            created_unix_secs: now,
            expires_unix_secs,
            install,
            remove,
            plan,
        };
        saved.validate_at(now)?;
        Ok(saved)
    }
    pub fn parse(input: &str, now: u64) -> Result<Self, Error> {
        if input.len() > MAX_BYTES {
            return Err(Error::InvalidInput("saved plan exceeds 16 MiB".into()));
        }
        let saved: Self = serde_json::from_str(input)
            .map_err(|_| Error::InvalidInput("invalid strict saved-plan schema".into()))?;
        saved.validate_at(now)?;
        Ok(saved)
    }
    pub fn validate_at(&self, now: u64) -> Result<(), Error> {
        if self.version != 1 {
            return Err(Error::Unsupported("saved-plan schema version".into()));
        }
        if self.created_unix_secs > now {
            return Err(Error::Safety(
                "saved plan was created in the future; check the clock and regenerate".into(),
            ));
        }
        let ttl = self
            .expires_unix_secs
            .checked_sub(self.created_unix_secs)
            .ok_or_else(|| Error::InvalidInput("saved-plan expiration precedes creation".into()))?;
        if !(1..=3600).contains(&ttl) {
            return Err(Error::InvalidInput(
                "saved-plan lifetime must be 1–3600 seconds".into(),
            ));
        }
        if now >= self.expires_unix_secs {
            return Err(Error::Safety(
                "saved plan expired; inspect and save a new plan".into(),
            ));
        }
        if self.install.is_empty() && self.remove.is_empty() {
            return Err(Error::InvalidInput(
                "saved plan requires requested package operations".into(),
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for id in self.install.iter().chain(&self.remove) {
            identity::validate(id)?;
            if id.provider != self.provider {
                return Err(Error::InvalidInput(
                    "saved plan cannot silently switch providers".into(),
                ));
            }
            if !seen.insert(id.to_string()) {
                return Err(Error::InvalidInput(
                    "saved plan contains duplicate or conflicting requests".into(),
                ));
            }
        }
        for operation in &self.plan.operations {
            identity::validate(&operation.package)?;
            if operation.package.provider != self.provider {
                return Err(Error::InvalidInput(
                    "saved plan contains an operation from another provider".into(),
                ));
            }
        }
        for package in &self.plan.observed_packages {
            identity::validate(&package.id)?;
            if package.id.provider != self.provider {
                return Err(Error::InvalidInput(
                    "saved plan baseline contains another provider".into(),
                ));
            }
        }
        if crate::planner::inventory_token(&self.plan.observed_packages)
            != self.plan.observed_state_fingerprint
        {
            return Err(Error::InvalidInput(
                "saved-plan baseline token is inconsistent".into(),
            ));
        }
        Ok(())
    }
    pub fn to_json(&self) -> Result<String, Error> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|_| Error::Parse("cannot serialize saved plan".into()))?;
        if json.len() > MAX_BYTES {
            return Err(Error::InvalidInput("saved plan exceeds 16 MiB".into()));
        }
        Ok(json)
    }
}
