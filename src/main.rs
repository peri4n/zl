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
        Command::List { path, tags, title, format } => commands::list::run(path, tags, title.as_ref(), format.as_ref()),
        Command::Tags { path, sort } => commands::tags::run(path, *sort),
        Command::New { path, template, title, group, vars } => commands::new::run(path, template.as_deref(), title.as_deref(), group.as_deref(), vars),
    }
}
