use std::error::Error;
use std::io;
use std::path::Path;

mod agents;
mod setup;

const USAGE: &str =
    "usage: habeas <mdbook | setup [--path <crate-dir>] [--install-skills] | agents>";

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        // {impl}`requirements.cli.mdbook`
        [command] if command == "mdbook" => {
            let (context, mut book) = mdbook_preprocessor::parse_input(io::stdin())?;
            habeas::preprocess_book(&mut book, &context.root)?;
            serde_json::to_writer(io::stdout().lock(), &book)?;
            Ok(())
        }
        // {impl}`requirements.cli.invocation`
        [command, supports, renderer] if command == "mdbook" && supports == "supports" => {
            if renderer == "html" {
                Ok(())
            } else {
                std::process::exit(1)
            }
        }
        // {impl}`requirements.cli.setup`
        [command, options @ ..] if command == "setup" => {
            let (local_crate, install_skills) = match options {
                [] => (None, false),
                [flag] if flag == "--install-skills" => (None, true),
                [flag, path] if flag == "--path" => (Some(Path::new(path)), false),
                [flag, path, install] if flag == "--path" && install == "--install-skills" => {
                    (Some(Path::new(path)), true)
                }
                [install, flag, path] if install == "--install-skills" && flag == "--path" => {
                    (Some(Path::new(path)), true)
                }
                _ => return Err(USAGE.into()),
            };
            setup::run(local_crate, install_skills)
        }
        // {impl}`requirements.cli.agents`
        [command] if command == "agents" => agents::run(),
        // {impl}`requirements.cli.invocation`
        _ => Err(USAGE.into()),
    }
}
