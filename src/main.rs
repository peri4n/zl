use std::fs;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand, Debug, Clone)]
enum Command {
    Validate {
        #[arg(short, long)]
        path: String,
    },
}

fn main() -> Result<(), String> {
    let cli = Cli::parse();

    match &cli.cmd {
        Command::Validate { path } => {
            let input = fs::read_to_string(path).map_err(|_| "File not found")?;
            let _ = markdown::to_mdast(&input, &markdown::ParseOptions::default());
        },
    }

    Ok(())

}
