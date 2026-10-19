use crate::{
    command::{checked, CommandRunner, CommandSpec},
    Capability, ProviderId,
};
use serde::Serialize;
#[derive(Debug, Serialize)]
pub struct ProviderStatus {
    pub provider: ProviderId,
    pub available: bool,
    pub version: Option<String>,
    pub capabilities: Vec<Capability>,
    pub diagnostic: Option<String>,
    pub compatibility: crate::version::Compatibility,
}
pub fn detect(runner: &dyn CommandRunner, os: &str) -> Vec<ProviderStatus> {
    [
        (ProviderId::Apt, "apt-get", "--version"),
        (ProviderId::Pacman, "pacman", "--version"),
        (ProviderId::Dnf, "dnf", "--version"),
        (ProviderId::Flatpak, "flatpak", "--version"),
        (ProviderId::Brew, "brew", "--version"),
        (ProviderId::Apk, "apk", "--version"),
        (ProviderId::Portage, "emerge", "--version"),
    ]
    .into_iter()
    .map(|(provider, binary, flag)| {
        if os != "linux" && !(os == "macos" && provider == ProviderId::Brew) {
            return ProviderStatus {
                provider,
                available: false,
                version: None,
                capabilities: vec![],
                diagnostic: Some(format!("unsupported OS: {os}")),
                compatibility: crate::version::compatibility(provider, ""),
            };
        }
        match checked(runner, &CommandSpec::new(binary, &[flag])) {
            Ok(output) => {
                let output_text = String::from_utf8_lossy(&output.stdout);
                let version = crate::version::reported_line(provider, &output_text).map(str::to_owned);
                let compatibility = crate::version::compatibility(provider, &output_text);
                let mut capabilities = vec![];
                if provider == ProviderId::Apt {
                    let dpkg =
                        checked(runner, &CommandSpec::new("dpkg-query", &["--version"])).is_ok();
                    let cache =
                        checked(runner, &CommandSpec::new("apt-cache", &["--version"])).is_ok();
                    let marks =
                        checked(runner, &CommandSpec::new("apt-mark", &["--version"])).is_ok();
                    if dpkg {
                        capabilities.extend([Capability::Inventory, Capability::ConfigInspection, Capability::Dependencies]);
                    }
                    if cache {
                        capabilities.push(Capability::Search);
                    }
                    if dpkg && marks {
                        capabilities.extend([
                            Capability::DryRun,
                            Capability::Orphans,
                            Capability::Install,
                            Capability::Remove,
                        ]);
                    }
                }
                match provider {
                    ProviderId::Pacman => capabilities.extend([
                        Capability::Inventory,
                        Capability::Search,
                        Capability::Dependencies,
                        Capability::Orphans,
                        Capability::DryRun,
                    ]),
                    ProviderId::Dnf
                        if checked(runner, &CommandSpec::new("rpm", &["--version"])).is_ok() =>
                    {
                        capabilities.extend([Capability::Inventory, Capability::Dependencies])
                    }
                    ProviderId::Flatpak => capabilities.push(Capability::Inventory),
                    ProviderId::Apk | ProviderId::Portage => {
                        capabilities.extend([Capability::Inventory, Capability::DryRun])
                    }
                    ProviderId::Brew => {
                        capabilities.extend([Capability::Inventory, Capability::Dependencies, Capability::Orphans])
                    }
                    _ => {}
                }
                let diagnostic = if compatibility.minimum_met != Some(true) {
                    capabilities.clear();
                    Some("native version is below the implementation target or unrecognized; compatibility is not assumed".into())
                } else if capabilities.is_empty() {
                    Some("no implemented adapter capabilities available".into())
                } else {
                    None
                };
                ProviderStatus {
                    provider,
                    available: true,
                    version,
                    capabilities,
                    diagnostic,
                    compatibility,
                }
            }
            Err(e) => ProviderStatus {
                provider,
                available: false,
                version: None,
                capabilities: vec![],
                diagnostic: Some(e.to_string()),
                compatibility: crate::version::compatibility(provider, ""),
            },
        }
    })
    .collect()
}
