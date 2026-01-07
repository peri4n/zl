use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) cmd: Command,
}

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum Command {
    /// Prints the current configuration to stdout
    Config,
    /// Initializes a zettelkasten directory
    Init {
        path: Option<String>,
    },
    /// Checks if a markdown file can be parsed correctly
    Parse {
        /// Path of the markdown file to validate
        path: String,
    },
    /// Creates a new markdown file from a template
    New {
        /// Path of the new markdown file
        path: String,

        /// Template of the new markdown file
        #[arg(short, long)]
        template: String,
    },
}

