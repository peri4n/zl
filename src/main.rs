use std::{fs, error::Error};

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
            let input = fs::read_to_string(path);

            match input {
                Err(err) => return Err(err.to_string()),
                Ok(content) => {
                    let result = markdown::to_mdast(&content, &markdown::ParseOptions::default());

                    match result {
                        Ok(_) => println!("Validated successfully."),
                        Err(err) => println!("{}", err),
                    }
                }
            }


        },
    }

    Ok(())

}
