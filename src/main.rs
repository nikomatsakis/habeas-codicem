use clap::{Parser, Subcommand};
use std::error::Error;
use std::io;
use std::io::Write;
use std::path::PathBuf;

mod agents;
mod setup;

// {impl}`requirements.cli.clap`
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
        #[arg(long)]
        install_skills: bool,
    },
    Agents,
    Status {
        #[arg(long)]
        all: bool,
    },
    Info {
        selector: String,
    },
}

#[derive(Subcommand)]
enum MdbookCommand {
    Supports { renderer: String },
}

fn main() -> Result<(), Box<dyn Error>> {
    // {impl}`requirements.cli.clap`
    match Cli::parse().command {
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
        Command::Setup {
            path,
            install_skills,
        } => setup::run(path.as_deref(), install_skills),
        // {impl}`requirements.cli.agents`
        Command::Agents => agents::run(),
        // {impl}`requirements.cli.status`
        Command::Status { all } => {
            let root = setup::project_root(true)?;
            io::stdout()
                .lock()
                .write_all(habeas::status_markdown(&root, all)?.as_bytes())?;
            Ok(())
        }
        // {impl}`requirements.cli.info`
        Command::Info { selector } => {
            let root = setup::project_root(true)?;
            io::stdout()
                .lock()
                .write_all(habeas::info_markdown(&root, &selector)?.as_bytes())?;
            Ok(())
        }
    }
}
