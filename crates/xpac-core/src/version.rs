use crate::{Error, ProviderId};
use serde::Serialize;
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct NativeVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}
#[derive(Debug, Serialize)]
pub struct Compatibility {
    pub detected: Option<NativeVersion>,
    pub minimum_target: NativeVersion,
    pub minimum_met: Option<bool>,
    pub certified: bool,
}
fn minimum(provider: ProviderId) -> NativeVersion {
    let (major, minor) = match provider {
        ProviderId::Apt => (1, 6),
        ProviderId::Pacman => (6, 0),
        ProviderId::Dnf => (4, 0),
        ProviderId::Flatpak => (1, 14),
        ProviderId::Brew => (4, 0),
        ProviderId::Script => (1, 0),
    };
    NativeVersion {
        major,
        minor,
        patch: 0,
    }
}
pub fn reported_line(provider: ProviderId, output: &str) -> Option<&str> {
    let lines = || output.lines().filter(|line| !line.trim().is_empty());
    match provider {
        ProviderId::Pacman => lines().find(|line| line.contains("Pacman v")),
        ProviderId::Apt => lines().find(|line| line.starts_with("apt ")),
        ProviderId::Flatpak => lines().find(|line| line.starts_with("Flatpak ")),
        ProviderId::Brew => lines().find(|line| line.starts_with("Homebrew ")),
        ProviderId::Dnf => lines().next(),
        ProviderId::Script => None,
    }
}
pub fn compatibility(provider: ProviderId, output: &str) -> Compatibility {
    let detected = reported_line(provider, output).and_then(|line| {
        line.split_whitespace().find_map(|word| {
            let word = word.strip_prefix('v').unwrap_or(word);
            if !word.as_bytes().first().is_some_and(u8::is_ascii_digit) {
                return None;
            }
            let mut components = word.split('.');
            let major = components.next()?.parse().ok()?;
            let minor = components.next()?.parse().ok()?;
            let patch = components
                .next()
                .unwrap_or("0")
                .split(|ch: char| !ch.is_ascii_digit())
                .next()?
                .parse()
                .ok()?;
            Some(NativeVersion {
                major,
                minor,
                patch,
            })
        })
    });
    let minimum_target = minimum(provider);
    Compatibility {
        detected,
        minimum_target,
        minimum_met: detected.map(|v| v >= minimum_target),
        certified: false,
    }
}
pub fn require_target(provider: ProviderId, output: &str) -> Result<(), Error> {
    let compatibility = compatibility(provider, output);
    match compatibility.minimum_met {
        Some(true) => Ok(()),
        Some(false) => Err(Error::Unsupported(format!(
            "{provider} version is below the implementation target {}.{}.{}",
            compatibility.minimum_target.major,
            compatibility.minimum_target.minor,
            compatibility.minimum_target.patch
        ))),
        None => Err(Error::Unsupported(format!(
            "cannot recognize {provider} native version; no compatibility is assumed"
        ))),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pacman_banner_dnf_generations_and_suffixes_are_recognized() {
        assert_eq!(
            compatibility(
                ProviderId::Pacman,
                " .--.\n/ _.-'  Pacman v6.1.0 - libalpm v14.0.0\n"
            )
            .detected
            .unwrap()
            .major,
            6
        );
        assert_eq!(
            compatibility(ProviderId::Dnf, "dnf5 version 5.2.7.0\n")
                .detected
                .unwrap()
                .major,
            5
        );
        assert_eq!(
            compatibility(ProviderId::Dnf, "4.21.1\nInstalled: dnf\n")
                .detected
                .unwrap()
                .major,
            4
        );
        assert!(require_target(ProviderId::Apt, "apt 2.6.1ubuntu1 (amd64)").is_ok());
        assert!(require_target(ProviderId::Apt, "apt 2.4.0 (amd64)").is_ok());
        assert!(require_target(ProviderId::Apt, "apt 1.6.17 (amd64)").is_ok());
        assert!(require_target(ProviderId::Apt, "apt 1.4.11 (amd64)").is_err());
        assert!(require_target(ProviderId::Brew, "unrecognized wrapper output").is_err());
        assert!(!compatibility(ProviderId::Brew, "Homebrew 4.6.0").certified);
    }
}
