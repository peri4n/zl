mod commands;
mod config;
mod index;
mod notes;
mod paths;

use anyhow::Context;
use clap::Parser;

use crate::commands::{Cli, Command};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Like `git -C`: every command resolves its paths, the config and the templates relative to the
    // working directory, so switching to the notebook makes them all relative to it
    if let Some(dir) = &cli.notebook_dir {
        std::env::set_current_dir(dir)
            .with_context(|| format!("Failed to open notebook directory {}", dir.display()))?;
    }

    match &cli.cmd {
        Command::Config => commands::config::run(),
        Command::Init(args) => commands::init::run(args),
        Command::Validate(args) => commands::validate::run(args),
        Command::Tasks(args) => commands::tasks::run(args),
        Command::List(args) => commands::list::run(args),
        Command::Tags(args) => commands::tags::run(args),
        Command::New(args) => commands::new::run(args),
    }
}
