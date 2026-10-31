pub(crate) mod config;
pub(crate) mod init;
pub(crate) mod list;
pub(crate) mod new;
pub(crate) mod tags;
pub(crate) mod tasks;
pub(crate) mod validate;

use clap::{Parser, Subcommand};
use regex::Regex;

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
    /// Lists tasks (`- [ ]` and `- [x]` list items) in markdown files
    Tasks {
        /// Markdown file or directory to search
        #[arg(default_value = ".")]
        path: String,

        /// Only show tasks with this status
        #[arg(short, long, value_enum, default_value_t = tasks::TaskStatus::All)]
        status: tasks::TaskStatus,
    },
    /// Lists notes matching all given filters
    ///
    /// Tags are read from the `tags` field of the frontmatter. The title is the frontmatter
    /// `title`, otherwise the first level 1 heading, otherwise the file name.
    List {
        /// Markdown file or directory to search
        #[arg(default_value = ".")]
        path: String,

        /// Only list notes with this tag (case-insensitive), can be repeated to require several tags
        #[arg(short, long = "tag", value_name = "TAG")]
        tags: Vec<String>,

        /// Only list notes whose title matches this regex, e.g. '(?i)^rust'
        #[arg(long, value_name = "REGEX")]
        title: Option<Regex>,

        /// Output format of each note [default: list.format from the config]
        ///
        /// Placeholders:
        ///   {path}      Path of the note, e.g. ./projects/zl.md
        ///   {dir}       Directory of the note, e.g. ./projects
        ///   {filename}  File name without extension, e.g. zl
        ///   {title}     Title of the note
        ///   {tags}      Comma separated tags, e.g. rust, cli
        ///   {link}      Wiki link to the note, e.g. [[zl]]
        ///   {mdlink}    Markdown link to the note, e.g. [zl Project](./projects/zl.md)
        ///   {modified}  Last modification date, e.g. 2026-10-31
        ///
        /// Use {{ and }} for literal braces, \t and \n for tabs and newlines.
        #[arg(long, value_name = "FORMAT", allow_hyphen_values = true, verbatim_doc_comment)]
        format: Option<list::Format>,
    },
    /// Lists all tags used in the frontmatter of notes, with the number of notes using them
    Tags {
        /// Markdown file or directory to search
        #[arg(default_value = ".")]
        path: String,

        /// Order of the tags
        #[arg(short, long, value_enum, default_value_t = tags::Sort::Name)]
        sort: tags::Sort,
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

        /// Template of the new markdown file [default: rule for the note's folder in
        /// template.folders, otherwise note.default_template]
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
