//! Native package caches: read-only measurement of the directories the package
//! manager itself reports, and confirmed cleanup through its own command only.
use crate::{
    command::{CommandRunner, CommandSpec},
    execution::ExecutionRecord,
    providers::output,
    Error, Plan, ProviderId,
};
use serde::Serialize;
use std::{fs, path::PathBuf, sync::atomic::AtomicBool, time::Duration};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CacheReport {
    pub provider: ProviderId,
    pub category: &'static str,
    pub directories: Vec<PathBuf>,
    pub files: u64,
    pub bytes: u64,
    /// The exact native command cleanup would run.
    pub native_command: Vec<String>,
    pub semantics: String,
}
impl CacheReport {
    fn token(&self) -> String {
        format!(
            "cache:{}:{}:{}:{}",
            self.provider,
            self.directories
                .iter()
                .map(|d| d.display().to_string())
                .collect::<Vec<_>>()
                .join(","),
            self.files,
            self.bytes
        )
    }
}
/// Fixed cleanup commands; sudo delegation accepts exactly these vectors.
pub const APT_CLEAN: &[&str] = &["apt-get", "clean"];
pub const PACMAN_CLEAN: &[&str] = &[
    "pacman",
    "--sync",
    "--clean",
    "--noconfirm",
    "--noprogressbar",
    "--color",
    "never",
];
fn command(provider: ProviderId) -> Result<&'static [&'static str], Error> {
    match provider {
        ProviderId::Apt => Ok(APT_CLEAN),
        ProviderId::Pacman => Ok(PACMAN_CLEAN),
        _ => Err(Error::Unsupported(format!(
            "package cache cleanup for {provider}"
        ))),
    }
}
fn directories(runner: &dyn CommandRunner, provider: ProviderId) -> Result<Vec<PathBuf>, Error> {
    let dirs: Vec<PathBuf> = match provider {
        ProviderId::Apt => {
            // apt-config's /d suffix resolves Dir::Cache::archives to a full path.
            let shell = output(
                runner,
                CommandSpec::new(
                    "apt-config",
                    &["shell", "ARCHIVES", "Dir::Cache::archives/d"],
                ),
            )?;
            let value = shell
                .trim()
                .strip_prefix("ARCHIVES='")
                .and_then(|rest| rest.strip_suffix('\''))
                .filter(|value| !value.contains('\''))
                .ok_or_else(|| Error::Parse("unexpected apt-config shell output".into()))?;
            vec![PathBuf::from(value)]
        }
        ProviderId::Pacman => output(runner, CommandSpec::new("pacman-conf", &["CacheDir"]))?
            .lines()
            .filter(|line| !line.is_empty())
            .map(PathBuf::from)
            .collect(),
        _ => {
            return Err(Error::Unsupported(format!(
                "package cache inspection for {provider}"
            )))
        }
    };
    if dirs.is_empty() || dirs.iter().any(|d| !d.is_absolute()) {
        return Err(Error::Parse(
            "native cache directories must be absolute".into(),
        ));
    }
    Ok(dirs)
}
fn package_file(provider: ProviderId, name: &str) -> bool {
    match provider {
        ProviderId::Apt => name.ends_with(".deb"),
        ProviderId::Pacman => name.contains(".pkg.tar"),
        _ => false,
    }
}
/// Count only regular package files directly inside each native cache directory.
/// Symlinks, subdirectories (such as APT's partial/) and other files are ignored.
pub fn measure(runner: &dyn CommandRunner, provider: ProviderId) -> Result<CacheReport, Error> {
    let directories = directories(runner, provider)?;
    let (mut files, mut bytes) = (0u64, 0u64);
    for directory in &directories {
        let entries = match fs::read_dir(directory) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(Error::Io {
                    program: format!("cache directory {}", directory.display()),
                    source,
                })
            }
            Ok(entries) => entries,
        };
        for entry in entries.flatten() {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let name = entry.file_name();
            if metadata.is_file()
                && !entry.file_type().is_ok_and(|t| t.is_symlink())
                && name.to_str().is_some_and(|n| package_file(provider, n))
            {
                files += 1;
                bytes = bytes.saturating_add(metadata.len());
            }
        }
    }
    Ok(CacheReport {
        provider,
        category: "safe_native_cleanup",
        directories,
        files,
        bytes,
        native_command: command(provider)?.iter().map(|s| (*s).into()).collect(),
        semantics: match provider {
            ProviderId::Apt => "apt-get clean removes every downloaded archive; packages must be downloaded again to reinstall".into(),
            _ => "pacman -Sc keeps archives of installed versions, removes others and unused sync databases; older versions can no longer be reinstalled from cache".into(),
        },
    })
}
/// Preview as a journalable plan; cache cleanup has no package operations.
pub fn plan(report: &CacheReport) -> Plan {
    Plan {
        operations: vec![],
        observed_state_fingerprint: report.token(),
        observed_packages: vec![],
        uncertain: false,
        warnings: vec![
            format!(
                "Native cache cleanup: `{}` ({} files, {} bytes in {}).",
                report.native_command.join(" "),
                report.files,
                report.bytes,
                report
                    .directories
                    .iter()
                    .map(|d| d.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            report.semantics.clone(),
        ],
    }
}
pub fn sudo_template(program: &str, args: &[String]) -> bool {
    [APT_CLEAN, PACMAN_CLEAN].iter().any(|template| {
        template[0] == program
            && args.len() == template.len() - 1
            && args.iter().zip(&template[1..]).all(|(a, b)| a == b)
    })
}
/// Re-measure, refuse a stale preview, run the native cleanup and verify the
/// cache did not grow (APT must be empty afterwards). Never deletes files itself.
pub fn clean(
    runner: &dyn CommandRunner,
    preview: &CacheReport,
    confirmed: bool,
    canceled: &AtomicBool,
    journal: Option<&crate::journal::Journal>,
) -> Result<ExecutionRecord, Error> {
    if !confirmed {
        return Err(Error::Safety(
            "cache cleanup requires explicit confirmation".into(),
        ));
    }
    if canceled.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(Error::Canceled("cache cleanup".into()));
    }
    let current = measure(runner, preview.provider)?;
    if &current != preview {
        return Err(Error::Safety(
            "cache changed since preview; inspect a new preview".into(),
        ));
    }
    let token = current.token();
    if current.files == 0 {
        return Ok(ExecutionRecord {
            success: true,
            native_status: None,
            verified: true,
            errors: vec![],
            rollback_attempted: false,
            observed_before: token.clone(),
            observed_after: Some(token),
        });
    }
    let template = command(current.provider)?;
    let mut spec = CommandSpec::new(template[0], &template[1..]);
    spec.requires_privilege = true;
    let journal_id = journal.map(|j| j.begin(&plan(&current))).transpose()?;
    let mut errors = vec![];
    let native_status = match runner.run(&spec, Duration::from_secs(600), canceled) {
        Ok(output) => {
            if output.status != Some(0) {
                errors.push(format!(
                    "native cache cleanup failed with status {:?}",
                    output.status
                ));
            }
            output.status
        }
        Err(error) => {
            errors.push(error.to_string());
            None
        }
    };
    let (verified, observed_after) = match measure(runner, current.provider) {
        Ok(after) => {
            let ok = after.bytes <= current.bytes
                && (current.provider != ProviderId::Apt || after.files == 0);
            if !ok {
                errors.push("cache was not reduced as the native command documents".into());
            }
            (ok, Some(after.token()))
        }
        Err(error) => {
            errors.push(format!("post-cleanup measurement unavailable: {error}"));
            (false, None)
        }
    };
    let mut record = ExecutionRecord {
        success: native_status == Some(0) && verified,
        native_status,
        verified,
        errors,
        rollback_attempted: false,
        observed_before: token,
        observed_after,
    };
    if let (Some(journal), Some(id)) = (journal, journal_id) {
        if let Err(error) = journal.finish(&id, &record) {
            record.success = false;
            record
                .errors
                .push(format!("journal finalization failed: {error}"));
        }
    }
    Ok(record)
}
