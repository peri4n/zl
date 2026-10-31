mod commands;
mod config;

use clap::Parser;

use crate::commands::{Cli, Command};

fn main() -> Result<(), String> {
    let cli = Cli::parse();

    match &cli.cmd {
        Command::Config => commands::config::run(),
        Command::Init { path } => commands::init::run(path.as_deref()),
        Command::Parse { path } => commands::parse::run(path),
        Command::New { path, template } => commands::new::run(path, template),
    }
}
