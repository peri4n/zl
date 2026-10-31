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
        Command::Init { path } => commands::init::run(path.as_deref()),
        Command::Validate { path } => commands::validate::run(path),
        Command::Tasks { path, status } => commands::tasks::run(path, *status),
        Command::List { path, filter, format } => commands::list::run(path, filter, format.as_ref()),
        Command::Tags { path, sort } => commands::tags::run(path, *sort),
        Command::New { path, template, title, group, vars } => commands::new::run(path, template.as_deref(), title.as_deref(), group.as_deref(), vars),
    }
}
