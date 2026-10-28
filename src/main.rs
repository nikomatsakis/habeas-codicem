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
        glob: Option<String>,
    },
    Info {
        selector: String,
    },
    #[command(hide = true)]
    Hook {
        #[command(subcommand)]
        command: HookCommand,
    },
    #[command(
        group(ArgGroup::new("target").required(true).multiple(false).args(["for_requirement", "for_example", "for_rust_item"])),
        group(ArgGroup::new("mode").required(true).multiple(false).args(["file_range", "rust_item", "requirements"]))
    )]
    AddInput {
        #[arg(long)]
        for_requirement: Option<String>,
        #[arg(long)]
        for_example: Option<String>,
        #[arg(long, num_args = 2, value_names = ["rust_file", "rust_item"])]
        for_rust_item: Option<Vec<String>>,
        #[arg(long)]
        file_range: Option<String>,
        #[arg(long)]
        rust_item: Option<String>,
        #[arg(long)]
        requirements: Option<String>,
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
        Command::Setup { path } => setup::run(path.as_deref()),
        // {impl}`requirements.cli.status`
        Command::Status { all, glob } => {
            let root = setup::project_root()?;
            io::stdout().lock().write_all(
                habeas::status_markdown_filtered(&root, all, glob.as_deref())?.as_bytes(),
            )?;
            Ok(())
        }
        // {impl}`requirements.cli.info`
        Command::Info { selector } => {
            let root = setup::project_root()?;
            io::stdout()
                .lock()
                .write_all(habeas::info_markdown(&root, &selector)?.as_bytes())?;
            Ok(())
        }
        // {impl}`requirements.integration.symposium_setup_hook`
        Command::Hook {
            command: HookCommand::SessionStart,
        } => hook::session_start(),
        // {impl}`requirements.cli.add_input`
        Command::AddInput {
            for_requirement,
            for_example,
            for_rust_item,
            file_range,
            rust_item,
            requirements,
        } => {
            let root = setup::project_root()?;
            let target = if let Some(name) = for_requirement {
                habeas::InputTarget::Requirement(name)
            } else if let Some(name) = for_example {
                habeas::InputTarget::Example(name)
            } else {
                let parts = for_rust_item.expect("clap requires a target");
                habeas::InputTarget::RustFunction {
                    rust_file: parts[0].clone(),
                    rust_item: parts[1].clone(),
                }
            };
            let mode = if let Some(specification) = file_range {
                habeas::InputMode::FileRange(specification)
            } else if let Some(specification) = rust_item {
                habeas::InputMode::RustItemsAtLine(specification)
            } else {
                habeas::InputMode::Requirements(requirements.expect("clap requires an input mode"))
            };
            io::stdout()
                .lock()
                .write_all(habeas::add_input(&root, target, mode)?.as_bytes())?;
            Ok(())
        }
    }
}
