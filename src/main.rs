use clap::{ArgGroup, Parser, Subcommand};
use std::error::Error;
use std::io;
use std::io::Write;
use std::path::PathBuf;

mod hook;
mod setup;

// {impl}`requirements.cli.clap`
#[derive(Parser)]
#[command(name = "habeas")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

// {impl}`requirements.cli.clap`
#[derive(Subcommand)]
enum Command {
    Mdbook {
        #[command(subcommand)]
        command: Option<MdbookCommand>,
    },
    // {impl}`requirements.cli.setup_local_path`
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
    // {impl}`requirements.authoring.batch`
    // `--config <name>` selects a configuration for this invocation.
    // {impl}`requirements.authoring.configuration_precedence`
    // `--confirm` applies to a named subject or `--all`.
    // {impl}`requirements.authoring.confirmation`
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
        /// Confirm true and FIXME results with an independent second
        /// assessment, verifying the less favorable result when they disagree.
        #[arg(long)]
        confirm: bool,
    },
    #[command(hide = true)]
    Hook {
        #[command(subcommand)]
        command: HookCommand,
    },
}

// {impl}`requirements.cli.clap`
#[derive(Subcommand)]
enum MdbookCommand {
    Supports { renderer: String },
}

// {impl}`requirements.cli.clap`
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
        Command::Status {
            all,
            selection,
            verbose,
        } => {
            let root = setup::project_root()?;
            io::stdout().lock().write_all(
                habeas::status_markdown_with_options(&root, all, selection.as_deref(), verbose)?
                    .as_bytes(),
            )?;
            Ok(())
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
        Command::Author {
            subject,
            all,
            config,
            stdout,
            verbose,
            confirm,
        } => {
            let root = setup::project_root()?;
            // Tokio's fixed worker pool is shared by all async assessments.
            let runtime = tokio::runtime::Runtime::new()?;
            if all {
                let outcome = runtime.block_on(habeas::authoring::author_all_with_options(
                    &root,
                    config.as_deref(),
                    habeas::authoring::AuthoringOptions { verbose, confirm },
                ))?;
                println!(
                    "{} authored; {} unchanged; {} failed",
                    outcome.completed.len(),
                    outcome.unchanged,
                    outcome.failures.len()
                );
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
            let outcome = runtime.block_on(habeas::authoring::author_evidence_with_options(
                &root,
                &subject.expect("clap requires a subject or --all"),
                config.as_deref(),
                output,
                habeas::authoring::AuthoringOptions { verbose, confirm },
            ))?;
            let mut summary = format!(
                "{}: {} / {}",
                outcome.subject, outcome.backend, outcome.model,
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
            Ok(())
        }
        // {impl}`requirements.integration.symposium_setup_hook`
        Command::Hook {
            command: HookCommand::SessionStart,
        } => hook::session_start(),
    }
}
