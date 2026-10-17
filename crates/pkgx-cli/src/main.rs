use clap::{Parser, Subcommand, ValueEnum};
mod demo;
mod output;
mod session;
mod tui;
use pkgx_core::{backend::Backend, detection, manifest::DesiredState, providers, ProviderId};
use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
};
#[derive(Parser)]
#[command(
    name = "pkgx",
    version,
    about = "Inspect, plan and reconcile native packages conservatively"
)]
struct Cli {
    /// Print versioned JSON results (export and completions emit native text).
    #[arg(long, global = true)]
    json: bool,
    /// Use sudo only for confirmed APT execution; previews stay unprivileged.
    #[arg(long, global = true)]
    sudo: bool,
    /// Dedicated private transaction directory; inspection never creates it.
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,
    /// Select a manifest profile; repeat to compose several profiles.
    #[arg(long, global = true)]
    profile: Vec<String>,
    #[command(subcommand)]
    command: Commands,
}
#[derive(Clone, ValueEnum)]
enum InventoryProvider {
    Apt,
    Pacman,
    Dnf,
    Flatpak,
    Brew,
}
impl From<InventoryProvider> for ProviderId {
    fn from(value: InventoryProvider) -> Self {
        match value {
            InventoryProvider::Apt => Self::Apt,
            InventoryProvider::Pacman => Self::Pacman,
            InventoryProvider::Dnf => Self::Dnf,
            InventoryProvider::Flatpak => Self::Flatpak,
            InventoryProvider::Brew => Self::Brew,
        }
    }
}
#[derive(Subcommand)]
enum Commands {
    /// Browse synthetic packages without native tools (read-only).
    Demo {
        /// Print the demo dataset instead of opening the terminal browser.
        #[arg(long)]
        snapshot: bool,
    },
    /// List explicit APT management grants (never inferred from installed state).
    Managed,
    /// Grant or forget strict-mode management of a concrete installed APT package.
    Manage {
        package: String,
        #[arg(long)]
        forget: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(long, conflicts_with = "dry_run")]
        yes: bool,
    },
    /// Search existing native metadata (provider-specific query syntax).
    Search {
        query: String,
        /// Search the installed snapshot with a literal, case-insensitive filter.
        #[arg(long)]
        installed: bool,
        #[arg(long, value_enum)]
        provider: InventoryProvider,
    },
    /// Inspect package records; non-APT adapters require an installed package.
    Info { package: String },
    /// Report native installation reason and its provenance limits.
    Why { package: String },
    /// Inspect installed native dependency expressions without resolving a graph.
    Deps { package: String },
    /// Preview or confirm an installation (execution supports APT only).
    Install {
        package: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(long, conflicts_with = "dry_run")]
        yes: bool,
    },
    /// Preview or confirm a removal (execution supports APT only).
    Remove {
        package: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(long, conflicts_with = "dry_run")]
        yes: bool,
    },
    /// Preview a qualified package operation or a resolved manifest.
    Plan {
        #[arg(
            short = 'f',
            long,
            conflicts_with = "package",
            required_unless_present = "package"
        )]
        file: Option<PathBuf>,
        #[arg(required_unless_present = "file")]
        package: Option<String>,
        #[arg(long, conflicts_with = "file")]
        remove: bool,
        /// Save a single-provider package preview; manifest bundles are not saved.
        #[arg(long, conflicts_with = "file")]
        save: Option<PathBuf>,
        #[arg(long, default_value_t = 300, requires = "save")]
        expires_in: u64,
    },
    /// Export installed identities to an additive TOML manifest.
    Export {
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long, value_enum, default_value = "apt")]
        provider: Vec<InventoryProvider>,
    },
    /// Report provider-scoped manifest drift without changing packages.
    Diff {
        #[arg(short = 'f', long)]
        file: PathBuf,
    },
    /// Reconcile an additive manifest after plan review and confirmation.
    Apply {
        #[arg(
            short = 'f',
            long,
            required_unless_present = "plan",
            conflicts_with = "plan"
        )]
        file: Option<PathBuf>,
        /// Apply a saved native preview after expiry and stale-state checks.
        #[arg(long, required_unless_present = "file", conflicts_with = "file")]
        plan: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long, conflicts_with = "dry_run")]
        yes: bool,
    },
    /// Open the package browser (read-only unless --allow-apply).
    Tui {
        #[arg(long, value_enum, default_value = "apt")]
        provider: Vec<InventoryProvider>,
        /// Enable typed confirmation for experimental APT execution.
        #[arg(long)]
        allow_apply: bool,
        #[arg(short = 'f', long)]
        file: Option<PathBuf>,
    },
    /// Inspect native orphan suggestions; never delete paths or packages.
    Audit {
        #[arg(long, value_enum, default_value = "apt")]
        provider: Vec<InventoryProvider>,
    },
    /// Preview cleanup findings; execution is deliberately unsupported.
    Clean {
        #[arg(long, required = true)]
        dry_run: bool,
        #[arg(long, value_enum, default_value = "apt")]
        provider: Vec<InventoryProvider>,
    },
    /// Validate or verify script artifacts without executing them.
    Recipe {
        #[command(subcommand)]
        command: RecipeCommand,
    },
    /// Print Bash, Zsh or Fish completion definitions.
    Completions {
        #[arg(value_enum)]
        shell: CompletionShell,
    },
    /// Read transaction records; missing results are running or unknown.
    History {
        #[arg(long, default_value_t=20, value_parser=clap::value_parser!(u16).range(1..=1000))]
        limit: u16,
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    /// Detect provider binaries and available adapter capabilities.
    Providers,
    /// Report provider diagnostics and interrupted/unknown journal entries.
    Doctor,
    /// List installed packages from an explicitly selected provider.
    List {
        #[arg(long, value_enum)]
        provider: InventoryProvider,
    },
}
#[derive(Clone, ValueEnum)]
enum CompletionShell {
    Bash,
    Zsh,
    Fish,
}

#[derive(Subcommand)]
enum RecipeCommand {
    /// Validate integrity metadata and inspect declared actions without downloading.
    Check {
        #[arg(short = 'f', long)]
        file: PathBuf,
    },
    /// Download, verify SHA-256 and discard the artifact without executing it.
    Verify {
        #[arg(short = 'f', long)]
        file: PathBuf,
    },
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        // Do not echo command stderr, which may contain credentials or private paths.
        if std::env::args().any(|arg| arg == "--json") {
            eprintln!(
                "{}",
                serde_json::json!({"schema_version":1,"error":error.to_string()})
            );
        } else {
            eprintln!("error: {error}");
        }
        std::process::exit(1);
    }
}
fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    if cli.sudo
        && !matches!(
            &cli.command,
            Commands::Install { .. }
                | Commands::Remove { .. }
                | Commands::Apply { .. }
                | Commands::Tui {
                    allow_apply: true,
                    ..
                }
        )
    {
        return Err(pkgx_core::Error::InvalidInput("--sudo supports install, remove, apply and tui --allow-apply only; inspection is never elevated".into()).into());
    }
    let runner = session::Session::new(cli.sudo)?;
    let journal = || state_directory(cli.state_dir.clone()).map(pkgx_core::journal::Journal::new);
    if !cli.profile.is_empty()
        && !matches!(
            &cli.command,
            Commands::Apply { file: Some(_), .. }
                | Commands::Diff { .. }
                | Commands::Plan { file: Some(_), .. }
                | Commands::Tui { file: Some(_), .. }
        )
    {
        return Err(pkgx_core::Error::InvalidInput(
            "--profile requires manifest diff, plan, apply or tui --file".into(),
        )
        .into());
    }
    match cli.command {
        Commands::Managed => {
            display(&managed_registry(cli.state_dir.clone())?.read()?, cli.json)?;
        }
        Commands::Manage {
            package,
            forget,
            dry_run,
            yes,
        } => {
            let id = qualified_id(&package)?;
            if id.provider != ProviderId::Apt || id.architecture_or_scope.is_none() {
                return Err(pkgx_core::Error::InvalidInput(
                    "management requires a concrete apt:package:architecture identity".into(),
                )
                .into());
            }
            let registry = managed_registry(cli.state_dir.clone())?;
            let state = registry.read()?;
            let package = if forget {
                if !state.packages.iter().any(|p| p.id == id) {
                    return Err(pkgx_core::Error::InvalidInput(
                        "no matching management grant".into(),
                    )
                    .into());
                }
                None
            } else {
                Some(pkgx_core::dependency::installed_match(
                    providers::backend(id.provider, &runner, std::env::consts::OS)?.inventory()?,
                    &id,
                )?)
            };
            let preview = serde_json::json!({"management_action":if forget {"forget"} else {"grant"}, "package":id, "observed":package, "native_changes":false, "warning":"A grant allows strict manifests to remove this exact native package/version/reason. It does not claim ownership of files. New installations are never automatically granted."});
            if dry_run {
                display(&preview, cli.json)?;
                return Ok(());
            }
            eprintln!("{}", output::human(&preview)?);
            confirm_action(&runner, yes, "Change this management grant? Type yes: ")?;
            let mut guard = registry.lock(&state)?;
            if let Some(package) = package {
                let current = pkgx_core::dependency::installed_match(
                    providers::backend(id.provider, &runner, std::env::consts::OS)?.inventory()?,
                    &id,
                )?;
                if current != package {
                    return Err(pkgx_core::Error::Safety(
                        "package changed since grant preview; no registry change performed".into(),
                    )
                    .into());
                }
                guard.grant(&package)?;
            } else {
                guard.forget(&id)?;
            }
            display(&guard.state, cli.json)?;
        }
        Commands::Demo { snapshot } => {
            let backends = demo::backends();
            if snapshot {
                let packages = backends
                    .iter()
                    .map(|b| b.inventory())
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>();
                display(
                    &serde_json::json!({"demo":true,"notice":demo::NOTICE,"packages":packages}),
                    cli.json,
                )?;
            } else {
                if cli.json {
                    return Err(pkgx_core::Error::InvalidInput(
                        "demo --json requires --snapshot".into(),
                    )
                    .into());
                }
                tui::run(&backends, &runner, None, false, None)?;
            }
        }

        Commands::Tui {
            provider,
            file,
            allow_apply,
        } => {
            if cli.json {
                return Err(
                    pkgx_core::Error::InvalidInput("tui does not support --json".into()).into(),
                );
            }
            if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
                return Err(pkgx_core::Error::InvalidInput(
                    "tui requires an interactive terminal".into(),
                )
                .into());
            }
            let manifest = file
                .map(|file| load_manifest(file, &cli.profile))
                .transpose()?;
            let mut selected: Vec<ProviderId> = provider.into_iter().map(Into::into).collect();
            if let Some(manifest) = &manifest {
                selected.extend(manifest.providers()?);
            }
            selected.sort();
            selected.dedup();
            let backends = selected
                .into_iter()
                .map(|provider| providers::backend(provider, &runner, std::env::consts::OS))
                .collect::<Result<Vec<_>, _>>()?;
            tui::run(
                &backends,
                &runner,
                manifest.as_ref(),
                allow_apply,
                cli.state_dir.clone(),
            )?;
        }
        Commands::Audit { provider } | Commands::Clean { provider, .. } => {
            let mut selected: Vec<ProviderId> = provider.into_iter().map(Into::into).collect();
            selected.sort();
            selected.dedup();
            let reports = selected
                .into_iter()
                .map(|id| {
                    let backend = providers::backend(id, &runner, std::env::consts::OS)?;
                    pkgx_core::audit::inspect(backend.as_ref())
                })
                .collect::<Result<Vec<_>, _>>()?;
            display(&reports, cli.json)?;
        }
        Commands::Recipe { command } => {
            use std::io::Read;
            let (file, verify) = match command {
                RecipeCommand::Check { file } => (file, false),
                RecipeCommand::Verify { file } => (file, true),
            };
            let mut input = String::new();
            std::fs::File::open(file)?
                .take(1024 * 1024 + 1)
                .read_to_string(&mut input)?;
            let recipe = pkgx_core::recipe::Recipe::parse(&input)?;
            if verify {
                display(
                    &recipe.verify_download(&runner, std::env::consts::OS)?,
                    cli.json,
                )?;
            } else {
                display(&recipe.summary()?, cli.json)?;
            }
        }
        Commands::Completions { shell } => {
            print!(
                "{}",
                match shell {
                    CompletionShell::Bash => include_str!("../../../completions/pkgx.bash"),
                    CompletionShell::Zsh => include_str!("../../../completions/_pkgx"),
                    CompletionShell::Fish => include_str!("../../../completions/pkgx.fish"),
                }
            );
        }
        Commands::History { limit, offset } => display(
            &journal()?.history_page(offset, usize::from(limit))?,
            cli.json,
        )?,
        Commands::Search {
            query,
            provider,
            installed,
        } => {
            let selected = providers::backend(provider.into(), &runner, std::env::consts::OS)?;
            let results = if installed {
                pkgx_core::catalog::search_installed(selected.as_ref(), &query)?
            } else {
                selected.search(&query)?
            };
            display(&results, cli.json)?;
        }
        Commands::Info { package } => {
            let id = qualified_id(&package)?;
            display(
                &providers::backend(id.provider, &runner, std::env::consts::OS)?.info(&id)?,
                cli.json,
            )?;
        }
        Commands::Deps { package } => {
            let id = qualified_id(&package)?;
            display(
                &providers::backend(id.provider, &runner, std::env::consts::OS)?
                    .dependencies(&id)?,
                cli.json,
            )?;
        }
        Commands::Why { package } => {
            let id = qualified_id(&package)?;
            display(
                &providers::backend(id.provider, &runner, std::env::consts::OS)?.why(&id)?,
                cli.json,
            )?;
        }
        Commands::Install {
            package,
            dry_run,
            yes,
        } => {
            transact(
                &runner,
                &[qualified_id(&package)?],
                &[],
                dry_run,
                yes,
                cli.json,
                &journal,
            )?;
        }
        Commands::Remove {
            package,
            dry_run,
            yes,
        } => {
            transact(
                &runner,
                &[],
                &[qualified_id(&package)?],
                dry_run,
                yes,
                cli.json,
                &journal,
            )?;
        }
        Commands::Apply {
            file,
            plan,
            dry_run,
            yes,
        } => {
            if let Some(file) = plan {
                use std::io::Read;
                let mut input = String::new();
                std::fs::File::open(file)?
                    .take(pkgx_core::saved_plan::MAX_BYTES as u64 + 1)
                    .read_to_string(&mut input)?;
                let saved = pkgx_core::saved_plan::SavedPlan::parse(&input, now_secs()?)?;
                if dry_run {
                    let mut preview = saved.plan.clone();
                    preview.warnings.push(format!("Stored preview only: native state has not been revalidated; expires at Unix time {}.",saved.expires_unix_secs));
                    display(&preview, cli.json)?;
                } else {
                    if saved.provider != ProviderId::Apt {
                        return Err(pkgx_core::Error::Unsupported(
                            "saved-plan execution currently supports APT only".into(),
                        )
                        .into());
                    }
                    require_linux()?;
                    execute_reviewed(
                        &runner,
                        &saved.plan,
                        &saved.install,
                        &saved.remove,
                        yes,
                        cli.json,
                        &journal,
                        Some(&saved),
                        None,
                    )?;
                }
                return Ok(());
            }
            let state = load_manifest(
                file.expect("clap requires manifest or saved plan"),
                &cli.profile,
            )?;
            let backends = manifest_backends(&state, &runner)?;
            if state.system.mode == pkgx_core::manifest::SyncMode::Strict {
                let registry = managed_registry(cli.state_dir.clone())?;
                let managed = registry.read()?;
                let references: Vec<&dyn Backend> = backends.iter().map(|b| b.as_ref()).collect();
                let review =
                    pkgx_core::planner::plan_manifest_multi_managed(&references, &state, &managed)?;
                if dry_run {
                    display(&review, cli.json)?;
                    return Ok(());
                }
                if let Some(provider) = review
                    .providers
                    .iter()
                    .find(|p| !p.plan.operations.is_empty())
                {
                    execute_reviewed(
                        &runner,
                        &provider.plan,
                        &review.diff.missing,
                        &review.diff.removals,
                        yes,
                        cli.json,
                        &journal,
                        None,
                        Some((&registry, &managed)),
                    )?;
                } else {
                    display(
                        &serde_json::json!({"operations":[],"success":true,"verified":true,"warnings":review.diff.warnings}),
                        cli.json,
                    )?;
                }
                return Ok(());
            }
            if dry_run {
                let references: Vec<&dyn Backend> = backends.iter().map(|b| b.as_ref()).collect();
                display(
                    &manifest_plan(&references, &state, cli.state_dir.clone())?,
                    cli.json,
                )?;
                return Ok(());
            }
            let observed = backends
                .iter()
                .map(|b| b.inventory())
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            let diff = state.diff(&observed)?;
            if diff.missing.iter().any(|id| id.provider != ProviderId::Apt) {
                return Err(pkgx_core::Error::Unsupported("manifest requires a missing package from a provider without an executor; no changes performed".into()).into());
            }
            if diff.missing.is_empty() {
                display(
                    &serde_json::json!({"operations":[],"success":true,"verified":true,"warnings":diff.warnings}),
                    cli.json,
                )?;
            } else {
                transact(
                    &runner,
                    &diff.missing,
                    &[],
                    dry_run,
                    yes,
                    cli.json,
                    &journal,
                )?;
            }
        }
        Commands::Plan {
            file,
            package,
            remove,
            save,
            expires_in,
        } => {
            if let Some(file) = file {
                let state = load_manifest(file, &cli.profile)?;
                let backends = manifest_backends(&state, &runner)?;
                let references: Vec<&dyn Backend> = backends.iter().map(|b| b.as_ref()).collect();
                display(
                    &manifest_plan(&references, &state, cli.state_dir.clone())?,
                    cli.json,
                )?;
            } else if let Some(package) = package {
                let id = qualified_id(&package)?;
                let selected = providers::backend(id.provider, &runner, std::env::consts::OS)?;
                let plan = if remove {
                    selected.plan(&[], std::slice::from_ref(&id))?
                } else {
                    selected.plan(std::slice::from_ref(&id), &[])?
                };
                if let Some(file) = save {
                    let (install, removals) = if remove {
                        (vec![], vec![id])
                    } else {
                        (vec![id], vec![])
                    };
                    let saved = pkgx_core::saved_plan::SavedPlan::new(
                        selected.provider(),
                        install,
                        removals,
                        plan.clone(),
                        now_secs()?,
                        expires_in,
                    )?;
                    pkgx_core::manifest::write_export(&file, &saved.to_json()?)?;
                }
                display(&plan, cli.json)?;
            }
        }
        Commands::Diff { file } => {
            let state = load_manifest(file, &cli.profile)?;
            let backends = manifest_backends(&state, &runner)?;
            let observed = backends
                .iter()
                .map(|b| b.inventory())
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            let diff = if state.system.mode == pkgx_core::manifest::SyncMode::Strict {
                state.diff_managed(&observed, &managed_registry(cli.state_dir.clone())?.read()?)?
            } else {
                state.diff(&observed)?
            };
            display(&diff, cli.json)?;
        }
        Commands::Export { output, provider } => {
            let mut observed = vec![];
            let mut selected: Vec<ProviderId> = provider.into_iter().map(Into::into).collect();
            selected.sort();
            selected.dedup();
            for id in selected {
                observed
                    .extend(providers::backend(id, &runner, std::env::consts::OS)?.inventory()?);
            }
            let manifest = DesiredState::export(&observed)?;
            if let Some(path) = output {
                pkgx_core::manifest::write_export(&path, &manifest)?;
            } else {
                print!("{manifest}");
            }
        }
        Commands::Doctor => {
            let providers = detection::detect(&runner, std::env::consts::OS);
            let status = journal().and_then(|journal| journal.status());
            let (status, journal_error) = match status {
                Ok(status) => (Some(status), None),
                Err(error) => (None, Some(error.to_string())),
            };
            let interrupted_or_running = status.as_ref().map(|s| s.pending_or_unknown);
            let value = serde_json::json!({"schema_version":1,"os":std::env::consts::OS,"providers":providers,"interrupted_or_running_transactions":interrupted_or_running,"journal_status":status,"journal_error":journal_error,"limitations":["Native version compatibility is not yet certified.","Transaction records without a result are running, interrupted or unknown; no rollback is implied."]});
            if cli.json {
                println!("{value}");
            } else {
                println!("{}", serde_json::to_string_pretty(&value)?);
            }
        }
        Commands::Providers => {
            let providers = detection::detect(&runner, std::env::consts::OS);
            if cli.json {
                println!(
                    "{}",
                    serde_json::json!({"schema_version":1,"os":std::env::consts::OS,"providers":providers})
                );
            } else {
                for p in providers {
                    println!(
                        "{:?}\t{}\t{}",
                        p.provider,
                        if p.available {
                            "detected"
                        } else {
                            "unavailable"
                        },
                        p.version.unwrap_or_default()
                    );
                    println!("  capabilities: {:?}", p.capabilities);
                    if let Some(diagnostic) = p.diagnostic {
                        println!("  {diagnostic}");
                    }
                }
            }
        }
        Commands::List { provider } => {
            let packages =
                providers::backend(provider.into(), &runner, std::env::consts::OS)?.inventory()?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::json!({"schema_version":1,"packages":packages})
                );
            } else {
                for p in packages {
                    println!("{}\t{}\t{:?}", p.id, p.version, p.install_reason);
                }
            }
        }
    }
    Ok(())
}

fn qualified_id(input: &str) -> Result<pkgx_core::PackageId, pkgx_core::Error> {
    input.parse()
}

fn require_linux() -> Result<(), pkgx_core::Error> {
    if std::env::consts::OS != "linux" {
        return Err(pkgx_core::Error::Unavailable("APT requires Linux".into()));
    }
    Ok(())
}
fn display(value: &impl serde::Serialize, json: bool) -> Result<(), serde_json::Error> {
    if json {
        println!("{}", serde_json::json!({"schema_version":1,"data":value}));
    } else {
        println!("{}", output::human(value)?);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn transact(
    runner: &session::Session,
    install: &[pkgx_core::PackageId],
    remove: &[pkgx_core::PackageId],
    dry_run: bool,
    yes: bool,
    json: bool,
    journal: &dyn Fn() -> Result<pkgx_core::journal::Journal, pkgx_core::Error>,
) -> Result<(), Box<dyn std::error::Error>> {
    if dry_run
        && install
            .iter()
            .chain(remove)
            .any(|id| id.provider != ProviderId::Apt)
    {
        let provider = install
            .iter()
            .chain(remove)
            .next()
            .expect("requested package")
            .provider;
        display(
            &providers::backend(provider, runner, std::env::consts::OS)?.plan(install, remove)?,
            json,
        )?;
        return Ok(());
    }
    if install
        .iter()
        .chain(remove)
        .any(|id| id.provider != ProviderId::Apt)
    {
        return Err(pkgx_core::Error::Unsupported(
            "execution currently supports APT only; no provider fallback is performed".into(),
        )
        .into());
    }
    require_linux()?;
    let plan =
        providers::backend(ProviderId::Apt, runner, std::env::consts::OS)?.plan(install, remove)?;
    if dry_run {
        display(&plan, json)?;
        return Ok(());
    }
    execute_reviewed(
        runner, &plan, install, remove, yes, json, journal, None, None,
    )
}
#[allow(clippy::too_many_arguments)]
fn execute_reviewed(
    runner: &session::Session,
    plan: &pkgx_core::Plan,
    install: &[pkgx_core::PackageId],
    remove: &[pkgx_core::PackageId],
    yes: bool,
    json: bool,
    journal: &dyn Fn() -> Result<pkgx_core::journal::Journal, pkgx_core::Error>,
    saved: Option<&pkgx_core::saved_plan::SavedPlan>,
    managed: Option<(
        &pkgx_core::managed::ManagedRegistry,
        &pkgx_core::managed::ManagedState,
    )>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Keep stdout a single result document in JSON mode; preview goes to stderr.
    eprintln!("{}", output::human(&plan)?);
    confirm_action(runner, yes, "Apply this native APT transaction? Type yes: ")?;
    if let Some(saved) = saved {
        saved.validate_at(now_secs()?)?;
    }
    let use_sudo = pkgx_core::privilege::authorize_apt(
        &pkgx_core::privilege::ProcessSudoAuthenticator,
        runner.sudo_requested,
        plan,
        true,
        &runner.canceled,
    )?;
    let elevated = pkgx_core::privilege::SudoAptRunner::new(runner);
    let execution_runner: &dyn pkgx_core::command::CommandRunner =
        if use_sudo { &elevated } else { runner };
    let journal = journal()?;
    let record = if let Some((registry, state)) = managed {
        let guard = registry.lock(state)?;
        pkgx_core::execution::apply_managed_apt(
            execution_runner,
            plan,
            install,
            remove,
            true,
            &runner.canceled,
            Some(&journal),
            &guard,
        )?
    } else if let Some(saved) = saved {
        pkgx_core::execution::apply_saved_apt(
            execution_runner,
            saved,
            true,
            &runner.canceled,
            Some(&journal),
        )?
    } else {
        pkgx_core::execution::apply_apt_recorded(
            execution_runner,
            plan,
            install,
            remove,
            true,
            &runner.canceled,
            Some(&journal),
        )?
    };
    let success = record.success;
    display(&record, json)?;
    if !success {
        return Err(pkgx_core::Error::Safety(
            "native transaction failed or verification failed; inspect execution record".into(),
        )
        .into());
    }
    Ok(())
}

fn confirm_action(
    runner: &session::Session,
    yes: bool,
    prompt: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let confirmed = if yes {
        true
    } else {
        if !io::stdin().is_terminal() {
            return Err(pkgx_core::Error::Safety(
                "noninteractive execution requires --yes; use --dry-run to preview".into(),
            )
            .into());
        }
        eprint!("{prompt}");
        io::stderr().flush()?;
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut answer = String::new();
            let result = io::stdin()
                .read_line(&mut answer)
                .map(|_| answer.trim() == "yes");
            let _ = sender.send(result);
        });
        loop {
            if runner.canceled.load(std::sync::atomic::Ordering::Relaxed) {
                break false;
            }
            match receiver.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(result) => break result?,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => break false,
            }
        }
    };
    if !confirmed {
        return Err(
            pkgx_core::Error::Safety("transaction canceled; no changes performed".into()).into(),
        );
    }
    if runner.canceled.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(pkgx_core::Error::Canceled("confirmation".into()).into());
    }
    Ok(())
}
fn managed_registry(
    state_dir: Option<PathBuf>,
) -> Result<pkgx_core::managed::ManagedRegistry, pkgx_core::Error> {
    Ok(pkgx_core::managed::ManagedRegistry::new(
        state_directory(state_dir)?.join("managed"),
    ))
}
fn manifest_plan(
    backends: &[&dyn Backend],
    state: &DesiredState,
    state_dir: Option<PathBuf>,
) -> Result<pkgx_core::planner::MultiPlan, pkgx_core::Error> {
    if state.system.mode == pkgx_core::manifest::SyncMode::Strict {
        pkgx_core::planner::plan_manifest_multi_managed(
            backends,
            state,
            &managed_registry(state_dir)?.read()?,
        )
    } else {
        pkgx_core::planner::plan_manifest_multi(backends, state)
    }
}

fn state_directory(explicit: Option<PathBuf>) -> Result<PathBuf, pkgx_core::Error> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    if let Some(base) =
        std::env::var_os("XDG_STATE_HOME").filter(|base| PathBuf::from(base).is_absolute())
    {
        return Ok(PathBuf::from(base).join("pkgx/transactions"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        return Ok(PathBuf::from(home).join(".local/state/pkgx/transactions"));
    }
    Err(pkgx_core::Error::InvalidInput(
        "set --state-dir or HOME for transaction history".into(),
    ))
}

fn load_manifest(
    file: PathBuf,
    profiles: &[String],
) -> Result<DesiredState, Box<dyn std::error::Error>> {
    use std::io::Read;
    let mut input = String::new();
    std::fs::File::open(file)?
        .take(1024 * 1024 + 1)
        .read_to_string(&mut input)?;
    Ok(DesiredState::parse(&input)?.resolve(std::env::consts::OS, profiles)?)
}
fn manifest_backends<'a>(
    state: &DesiredState,
    runner: &'a dyn pkgx_core::command::CommandRunner,
) -> Result<Vec<Box<dyn Backend + 'a>>, pkgx_core::Error> {
    state
        .providers()?
        .into_iter()
        .map(|p| providers::backend(p, runner, std::env::consts::OS))
        .collect()
}

fn now_secs() -> Result<u64, pkgx_core::Error> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|time| time.as_secs())
        .map_err(|_| pkgx_core::Error::Safety("system clock predates Unix epoch".into()))
}
