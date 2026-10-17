use crate::{apt, Error, PackageId, ProviderId};
use std::{fmt, str::FromStr};
impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Apt => "apt",
            Self::Pacman => "pacman",
            Self::Dnf => "dnf",
            Self::Flatpak => "flatpak",
            Self::Brew => "brew",
            Self::Script => "script",
        })
    }
}
impl FromStr for ProviderId {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self, Error> {
        match value {
            "apt" => Ok(Self::Apt),
            "pacman" => Ok(Self::Pacman),
            "dnf" => Ok(Self::Dnf),
            "flatpak" => Ok(Self::Flatpak),
            "brew" => Ok(Self::Brew),
            "script" => Ok(Self::Script),
            _ => Err(Error::InvalidInput("unknown provider".into())),
        }
    }
}
impl fmt::Display for PackageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.provider, self.name)?;
        if let Some(scope) = &self.architecture_or_scope {
            write!(f, ":{scope}")?;
        }
        Ok(())
    }
}
impl FromStr for PackageId {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self, Error> {
        let (provider, name) = value.split_once(':').ok_or_else(|| {
            Error::InvalidInput("explicit provider:package[:scope] identity required".into())
        })?;
        parse_name(provider.parse()?, name)
    }
}
fn token(value: &str, extra: &[u8]) -> bool {
    !value.is_empty()
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || extra.contains(&b))
}
pub fn parse_name(provider: ProviderId, input: &str) -> Result<PackageId, Error> {
    if provider == ProviderId::Apt {
        return apt::package_id(input);
    }
    let mut parts = input.split(':');
    let name = parts.next().unwrap_or_default();
    let scope = parts.next();
    if parts.next().is_some() {
        return Err(Error::InvalidInput("too many identity components".into()));
    }
    let valid = match provider {
        ProviderId::Pacman => token(name, b"@._+-") && scope.is_none_or(|s| token(s, b"_-")),
        ProviderId::Dnf => token(name, b"._+-") && scope.is_none_or(|s| token(s, b"_")),
        ProviderId::Flatpak => {
            let fields: Vec<_> = name.split('/').collect();
            fields.len() == 4
                && matches!(fields[0], "app" | "runtime")
                && fields[1].contains('.')
                && fields[1..].iter().all(|s| token(s, b"._-"))
                && matches!(scope, Some("user" | "system"))
        }
        ProviderId::Brew => {
            let fields: Vec<_> = name.split('/').collect();
            matches!(fields.len(), 1 | 3)
                && fields.iter().all(|s| token(s, b"@._+-"))
                && matches!(scope, Some("formula" | "cask"))
        }
        _ => false,
    };
    if !valid {
        return Err(Error::InvalidInput(format!(
            "invalid {provider} identity or missing concrete scope"
        )));
    }
    Ok(PackageId {
        provider,
        name: name.into(),
        architecture_or_scope: scope.map(str::to_owned),
    })
}
pub fn validate(id: &PackageId) -> Result<(), Error> {
    let name = if let Some(scope) = &id.architecture_or_scope {
        format!("{}:{scope}", id.name)
    } else {
        id.name.clone()
    };
    if parse_name(id.provider, &name)? != *id {
        return Err(Error::InvalidInput("noncanonical identity".into()));
    }
    Ok(())
}
