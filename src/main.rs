mod commands;
mod config;
mod notes;

use clap::Parser;

use crate::commands::{Cli, Command};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match &cli.cmd {
        Command::Config => commands::config::run(),
        Command::Init { path } => commands::init::run(path.as_deref()),
        Command::Validate { path } => commands::validate::run(path),
        Command::Tasks { path, status } => commands::tasks::run(path, *status),
        Command::List { path, tags, title } => commands::list::run(path, tags, title.as_ref()),
        Command::New { path, template, title, vars } => commands::new::run(path, template.as_deref(), title.as_deref(), vars),
    }
}
