//! Recipe preparation only. No downloaded script is ever executed by this module.
use crate::{
    command::{CommandRunner, CommandSpec},
    providers::output,
    Error,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, path::Path};
use url::Url;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub version: u32,
    pub name: String,
    pub pinned_version: String,
    pub url: String,
    pub sha256: String,
    pub interpreter: Interpreter,
    pub install: Action,
    pub uninstall: Option<Action>,
    pub resources: Vec<Resource>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpreter {
    Sh,
    Bash,
    Python3,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub args: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    pub kind: ResourceKind,
    pub path: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    File,
    Link,
    Configuration,
    Service,
}
#[derive(Debug, Serialize)]
pub struct RecipeSummary {
    pub name: String,
    pub pinned_version: String,
    pub source: String,
    pub sha256: String,
    pub interpreter: Interpreter,
    pub install: Action,
    pub uninstall: Option<Action>,
    pub resources: Vec<crate::ManagedResource>,
    pub execution_supported: bool,
    pub ownership: String,
}
#[derive(Debug, Serialize)]
pub struct VerifiedDownload {
    pub recipe: RecipeSummary,
    pub byte_count: u64,
    pub verified: bool,
}
impl Recipe {
    pub fn parse(input: &str) -> Result<Self, Error> {
        if input.len() > 1024 * 1024 {
            return Err(Error::InvalidInput("recipe exceeds 1 MiB".into()));
        }
        let recipe: Self = toml::from_str(input)
            .map_err(|_| Error::InvalidInput("invalid strict recipe schema".into()))?;
        recipe.validate()?;
        Ok(recipe)
    }
    pub fn validate(&self) -> Result<(), Error> {
        if self.version != 1 {
            return Err(Error::Unsupported("recipe schema version".into()));
        }
        if self.name.len() < 2
            || self.name.len() > 128
            || !self.name.as_bytes()[0].is_ascii_alphanumeric()
            || !self
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(Error::InvalidInput("invalid recipe name".into()));
        }
        if self.pinned_version.is_empty()
            || self.pinned_version.len() > 128
            || matches!(
                self.pinned_version.as_str(),
                "latest" | "stable" | "main" | "master"
            )
            || self.pinned_version.chars().any(char::is_control)
        {
            return Err(Error::InvalidInput(
                "recipe requires a pinned version".into(),
            ));
        }
        let url = Url::parse(&self.url)
            .map_err(|_| Error::InvalidInput("invalid recipe HTTPS URL".into()))?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.query().is_some()
        {
            return Err(Error::Safety(
                "recipe URL must use HTTPS without credentials, query or fragment".into(),
            ));
        }
        if self.sha256.len() != 64
            || !self.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || self.sha256.bytes().all(|b| b == b'0')
        {
            return Err(Error::Safety(
                "recipe requires a concrete SHA-256 digest; no integrity bypass exists".into(),
            ));
        }
        for action in std::iter::once(&self.install).chain(self.uninstall.iter()) {
            if action.args.len() > 128
                || action.args.iter().any(|s| {
                    s.len() > 4096 || s.contains('\0') || s.contains("${") || s.contains("$(")
                })
            {
                return Err(Error::InvalidInput(
                    "recipe args must be bounded literal data without interpolation".into(),
                ));
            }
        }
        let mut paths = BTreeSet::new();
        for resource in &self.resources {
            let path = Path::new(&resource.path);
            if !path.is_absolute()
                || path.parent().is_none()
                || path
                    .components()
                    .any(|part| matches!(part, std::path::Component::ParentDir))
                || resource.path.chars().any(char::is_control)
                || !paths.insert(resource.path.clone())
            {
                return Err(Error::InvalidInput(
                    "resources require unique absolute non-root paths without parent traversal"
                        .into(),
                ));
            }
        }
        Ok(())
    }
    pub fn summary(&self) -> Result<RecipeSummary, Error> {
        self.validate()?;
        Ok(RecipeSummary {
            name: self.name.clone(),
            pinned_version: self.pinned_version.clone(),
            source: self.url.clone(),
            sha256: self.sha256.to_ascii_lowercase(),
            interpreter: self.interpreter.clone(),
            install: self.install.clone(),
            uninstall: self.uninstall.clone(),
            resources: self
                .resources
                .iter()
                .cloned()
                .map(|resource| crate::ManagedResource {
                    resource,
                    ownership: crate::ResourceOwnership::DeclaredOnly,
                    confidence: crate::Confidence::DeclaredOnly,
                })
                .collect(),
            execution_supported: false,
            ownership: "declared_only_unverified; not permission to remove files".into(),
        })
    }
    /// Download to private temporary storage, verify using the native SHA-256
    /// utility and discard the artifact on return. Never chmod or invoke it.
    pub fn verify_download(
        &self,
        runner: &dyn CommandRunner,
        os: &str,
    ) -> Result<VerifiedDownload, Error> {
        self.validate()?;
        if !matches!(os, "linux" | "macos") {
            return Err(Error::Unsupported("recipe verification OS".into()));
        }
        let directory = tempfile::tempdir().map_err(io)?;
        let artifact = directory.path().join("artifact");
        let path = artifact
            .to_str()
            .ok_or_else(|| Error::InvalidInput("temporary path is not UTF-8".into()))?;
        let mut download = CommandSpec::new(
            "curl",
            &[
                "--disable",
                "--fail",
                "--silent",
                "--show-error",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "--location",
                "--max-redirs",
                "0",
                "--connect-timeout",
                "10",
                "--max-time",
                "25",
                "--max-filesize",
                "8388608",
                "--output",
                "-",
                "--",
                &self.url,
            ],
        );
        download.env.insert("SSLKEYLOGFILE".into(), String::new());
        // Capture through the runner's hard output bound as well as curl's size
        // guard, including old curl versions and chunked responses.
        let bytes = crate::command::checked(runner, &download)?.stdout;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(Error::Safety("recipe download exceeds 8 MiB".into()));
        }
        fs::write(&artifact, &bytes).map_err(io)?;
        let metadata = fs::symlink_metadata(&artifact).map_err(io)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > 8 * 1024 * 1024
        {
            return Err(Error::Safety(
                "download must be a regular file of at most 8 MiB".into(),
            ));
        }
        let spec = if os == "macos" {
            CommandSpec::new("shasum", &["-a", "256", "--", path])
        } else {
            CommandSpec::new("sha256sum", &["--", path])
        };
        let digest = output(runner, spec)?;
        let digest = digest
            .split_whitespace()
            .next()
            .ok_or_else(|| Error::Parse("checksum utility returned no digest".into()))?;
        if !digest.eq_ignore_ascii_case(&self.sha256) {
            return Err(Error::Safety(
                "download SHA-256 mismatch; artifact discarded".into(),
            ));
        }
        Ok(VerifiedDownload {
            recipe: self.summary()?,
            byte_count: metadata.len(),
            verified: true,
        })
    }
}
fn io(source: std::io::Error) -> Error {
    Error::Io {
        program: "recipe staging".into(),
        source,
    }
}
