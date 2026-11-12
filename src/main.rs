use clap::{ArgGroup, Parser, Subcommand};
use std::error::Error;
use std::io;
use std::io::Write;
use std::path::PathBuf;

mod hook;
mod setup;

#[derive(Parser)]
#[command(name = "habeas")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Mdbook {
        #[command(subcommand)]
        command: Option<MdbookCommand>,
    },
    Setup {
        #[arg(long, value_name = "crate-dir")]
        path: Option<PathBuf>,
    },
    Status {
        #[arg(long)]
        all: bool,
        /// List the judgments and original FIXME entries within each issue group.
        #[arg(long)]
        verbose: bool,
        /// A full requirement or example name, or a definition glob.
        selection: Option<String>,
    },
    Info {
        /// A full requirement or example name, or a definition glob.
        selection: String,
    },
    // The single-subject syntax, its alias, and the `--all` exclusions.
    // {impl}`requirements.authoring.invocation`
    // `--config <name>` selects a configuration for this invocation.
    // `--missing` and `--force` choose which selected subjects are assessed;
    // `--stdout` implies `--force`, so it excludes `--missing`.
    // `--policy <name>` selects the assessment policy for every form.
    // {impl}`requirements.authoring.policy`
    #[command(alias = "author-evidence", group(ArgGroup::new("author_target").required(true).multiple(false).args(["subject", "all"])))]
    Author {
        subject: Option<String>,
        /// Author all requirement and example evidence needing attention.
        #[arg(long, conflicts_with = "stdout")]
        all: bool,
        #[arg(long)]
        config: Option<String>,
        /// Write evidence JSON to stdout instead of updating its file.
        #[arg(long)]
        stdout: bool,
        /// Show the stored rationale for false judgments.
        #[arg(short, long)]
        verbose: bool,
        /// Assess only subjects whose evidence is missing, stale, or invalid
        /// (the default).
        #[arg(long, conflicts_with_all = ["force", "stdout"])]
        missing: bool,
        /// Assess every selected subject, including those with current evidence.
        #[arg(long)]
        force: bool,
        /// The assessment policy: `confirm` (the default) checks each true or
        /// FIXME result in further independent conversations; `once` records a
        /// single assessment.
        #[arg(long, value_name = "name")]
        policy: Option<habeas::config::AssessmentPolicy>,
    },
    /// Update evidence after moving files, outline nodes, or definitions.
    Refactor {
        /// `<old>=<new>`: a file or directory path, a `<file>#<selector>`
        /// node, or a definition name.
        #[arg(long = "map", value_name = "old=new", required = true)]
        mappings: Vec<String>,
        /// Report what would change without writing anything.
        #[arg(long)]
        dry_run: bool,
    },
    #[command(hide = true)]
    Hook {
        #[command(subcommand)]
        command: HookCommand,
    },
}

#[derive(Subcommand)]
enum MdbookCommand {
    Supports { renderer: String },
}

#[derive(Subcommand)]
enum HookCommand {
    SessionStart,
}

fn main() -> Result<(), Box<dyn Error>> {
    // {impl}`requirements.cli.clap`
    let cli = Cli::parse();
    // {impl}`requirements.configuration.workspace_location`
    // {impl}`requirements.configuration.workspace_structure`
    // {impl}`requirements.configuration.loading`
    // Commands other than `author` load only the evidence settings, so status
    // and book rendering never need authoring configuration or credentials.
    // {impl}`requirements.authoring.deterministic_validation`
    // Only commands that resolve evidence inputs use the workspace evidence
    // path policy; setup, the session-start hook, and support queries do not.
    // `status` and `info` render the validation report, which records an
    // invalid workspace file as one finding and still runs the other checks,
    // so they must not stop here.
    // {impl}`requirements.ci.errors`
    if !matches!(
        cli.command,
        Command::Mdbook {
            command: Some(MdbookCommand::Supports { .. })
        } | Command::Setup { .. }
            | Command::Hook { .. }
            | Command::Status { .. }
            | Command::Info { .. }
    ) {
        habeas::config::load_workspace_evidence(&std::env::current_dir()?)?;
    }
    match cli.command {
        // {impl}`requirements.cli.mdbook`
        Command::Mdbook { command: None } => {
            let (context, mut book) = mdbook_preprocessor::parse_input(io::stdin())?;
            habeas::preprocess_book(&mut book, &context.root)?;
            serde_json::to_writer(io::stdout().lock(), &book)?;
            Ok(())
        }
        // {impl}`requirements.cli.invocation`
        Command::Mdbook {
            command: Some(MdbookCommand::Supports { renderer }),
        } => {
            if renderer == "html" {
                Ok(())
            } else {
                std::process::exit(1)
            }
        }
        // {impl}`requirements.cli.setup`
        // {impl}`requirements.cli.setup_local_path`
        Command::Setup { path } => setup::run(path.as_deref()),
        // {impl}`requirements.cli.status`
        // {impl}`requirements.cli.fixme_summary`
        // {impl}`requirements.cli.fixme_issues`
        Command::Status {
            all,
            selection,
            verbose,
        } => {
            let root = setup::project_root()?;
            io::stdout().lock().write_all(
                habeas::status_markdown_with_github(
                    &root,
                    all,
                    selection.as_deref(),
                    verbose,
                    &habeas::GithubIssues::from_env(&root),
                )?
                .as_bytes(),
            )?;
            Ok(())
        }
        // {impl}`requirements.refactor.command`
        // {impl}`requirements.refactor.invalid_mappings`
        Command::Refactor { mappings, dry_run } => {
            let root = setup::project_root()?;
            match habeas::refactor(&root, &mappings, dry_run) {
                Ok(report) => {
                    io::stdout().lock().write_all(report.as_bytes())?;
                    Ok(())
                }
                Err(habeas::RefactorError::Usage(message)) => {
                    use clap::CommandFactory;
                    Cli::command()
                        .error(clap::error::ErrorKind::ValueValidation, message)
                        .exit()
                }
                Err(habeas::RefactorError::Io(error)) => Err(error.into()),
            }
        }
        // {impl}`requirements.cli.info`
        Command::Info { selection } => {
            let root = setup::project_root()?;
            io::stdout()
                .lock()
                .write_all(habeas::info_markdown(&root, &selection)?.as_bytes())?;
            Ok(())
        }
        // {impl}`requirements.authoring.invocation`
        // {impl}`requirements.authoring.batch`
        // {impl}`requirements.authoring.concurrency`
        // {impl}`requirements.authoring.reporting`
        // {impl}`requirements.authoring.read_denial`
        // {impl}`requirements.authoring.configuration_precedence`
        // {impl}`requirements.authoring.confirmation`
        // {impl}`requirements.authoring.refresh`
        // {impl}`requirements.authoring.policy`
        Command::Author {
            subject,
            all,
            config,
            stdout,
            verbose,
            missing: _,
            force,
            policy,
        } => {
            let root = setup::project_root()?;
            // Tokio's fixed worker pool is shared by all async assessments.
            let runtime = tokio::runtime::Runtime::new()?;
            // `sites:<full-name>` without a site authors every site review of
            // the requirement, as a batch.
            // {impl}`requirements.references.site_review`
            let sites_of = subject
                .as_deref()
                .and_then(|subject| subject.strip_prefix("sites:"))
                .filter(|rest| !rest.contains('/'));
            if all || sites_of.is_some() {
                if stdout && sites_of.is_some() {
                    return Err(io::Error::other(
                        "--stdout requires a single subject; sites:<full-name> names every site review of a requirement",
                    )
                    .into());
                }
                let options = habeas::authoring::AuthoringOptions {
                    verbose,
                    policy,
                    force,
                };
                let outcome = match sites_of {
                    Some(requirement) => {
                        runtime.block_on(habeas::authoring::author_sites_with_options(
                            &root,
                            requirement,
                            config.as_deref(),
                            options,
                        ))?
                    }
                    None => runtime.block_on(habeas::authoring::author_all_with_options(
                        &root,
                        config.as_deref(),
                        options,
                    ))?,
                };
                let mut summary = format!(
                    "{} authored; {} skipped as current; {} failed",
                    outcome.completed.len(),
                    outcome.unchanged,
                    outcome.failures.len()
                );
                if let Some(policy) = outcome.policy {
                    summary.push_str(&format!("; policy {policy}"));
                }
                println!("{summary}");
                print_coverage_warnings(&root);
                if !outcome.failures.is_empty() {
                    return Err(io::Error::other(
                        "evidence authoring failed for one or more subjects; see diagnostics above",
                    )
                    .into());
                }
                return Ok(());
            }
            let output = if stdout {
                habeas::authoring::EvidenceOutput::Stdout
            } else {
                habeas::authoring::EvidenceOutput::File
            };
            let subject = subject.expect("clap requires a subject or --all");
            let outcome = runtime.block_on(habeas::authoring::author_evidence_with_options(
                &root,
                &subject,
                config.as_deref(),
                output,
                habeas::authoring::AuthoringOptions {
                    verbose,
                    policy,
                    force,
                },
            ))?;
            // Under `--missing`, a current record is kept without a model.
            let Some(outcome) = outcome else {
                println!(
                    "{subject}: evidence is current; 1 skipped as current (use --force to reassess)"
                );
                return Ok(());
            };
            let mut summary = format!(
                "{}: {} / {}; policy {}",
                outcome.subject, outcome.backend, outcome.model, outcome.policy,
            );
            if matches!(outcome.determination, habeas::judgment::Judgment::Fixme(_)) {
                summary.push_str(" — FIXME");
            }
            if matches!(outcome.determination, habeas::judgment::Judgment::False) {
                eprintln!("{}: false", outcome.subject);
                if verbose {
                    eprintln!("{}", outcome.rationale);
                }
            }
            if let Some(path) = outcome.evidence_path {
                println!(
                    "{summary}\nEvidence: {}\nUsage: {}",
                    path.display(),
                    outcome.usage_path.display()
                );
            } else {
                eprintln!(
                    "{summary}\nEvidence: stdout\nUsage: {}",
                    outcome.usage_path.display()
                );
            }
            print_coverage_warnings(&root);
            Ok(())
        }
        // {impl}`requirements.integration.symposium_setup_hook`
        Command::Hook {
            command: HookCommand::SessionStart,
        } => hook::session_start(),
    }
}

/// Prints the coverage warnings after an authoring run on stderr, so stdout
/// keeps only what the command reports there. A failure to compute them is
/// reported but does not fail the command.
// {impl}`requirements.references.coverage.report`
fn print_coverage_warnings(root: &std::path::Path) {
    match habeas::coverage_warning_lines(root) {
        Ok(lines) if lines.is_empty() => {}
        Ok(lines) => {
            eprintln!(
                "Coverage warnings (these items have no recent coverage; they do not fail validation):"
            );
            for line in lines {
                eprintln!("- {line}");
            }
        }
        Err(error) => eprintln!("cannot compute coverage warnings: {error}"),
    }
}
