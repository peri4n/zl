pub(crate) mod config;
pub(crate) mod init;
pub(crate) mod new;
pub(crate) mod validate;

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
    Validate {
        /// Path of the markdown file to validate
        path: String,
    },
    /// Creates a new markdown file from a template
    ///
    /// Templates are read from `.zl/templates/<name>.md` and rendered with Tera.
    /// The following variables are available in every template:
    ///
    ///   title     Title of the note (--title or note.default_title)
    ///   id        Zettel ID derived from the creation time, e.g. 20261031143005
    ///   date      Creation date, e.g. 2026-10-31
    ///   time      Creation time, e.g. 14:30
    ///   datetime  Creation timestamp in RFC 3339, e.g. 2026-10-31T14:30:05+01:00
    ///   filename  File name of the note without extension
    ///
    /// Additional variables can be passed with --var KEY=VALUE.
    #[command(verbatim_doc_comment)]
    New {
        /// Path of the new markdown file
        path: String,

        /// Template of the new markdown file [default: note.default_template]
        #[arg(short, long)]
        template: Option<String>,

        /// Title of the new note [default: note.default_title]
        #[arg(long)]
        title: Option<String>,

        /// Additional template variable, can be repeated
        #[arg(long = "var", value_name = "KEY=VALUE", value_parser = parse_key_value)]
        vars: Vec<(String, String)>,
    },
}


fn parse_key_value(s: &str) -> Result<(String, String), String> {
    let (key, value) = s
        .split_once('=')
        .ok_or_else(|| format!("Invalid KEY=VALUE: no `=` found in `{}`", s))?;

    if key.is_empty() {
        return Err(format!("Invalid KEY=VALUE: empty key in `{}`", s));
    }

    Ok((key.to_string(), value.to_string()))
}
