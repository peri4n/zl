pub(crate) mod config;
pub(crate) mod init;
pub(crate) mod list;
pub(crate) mod lsp;
pub(crate) mod new;
pub(crate) mod tags;
pub(crate) mod tasks;
pub(crate) mod validate;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub(crate) struct Cli {
    /// Notebook to work on, instead of the current directory. All paths are relative to it.
    #[arg(
        short = 'C',
        long,
        global = true,
        value_name = "DIR",
        env = "ZL_NOTEBOOK_DIR"
    )]
    pub(crate) notebook_dir: Option<PathBuf>,

    #[command(subcommand)]
    pub(crate) cmd: Command,
}

/// The arguments of each command are defined next to its implementation
#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    /// Prints the current configuration to stdout
    Config,
    /// Initializes a zettelkasten directory
    Init(init::Args),
    /// Checks if markdown files and their frontmatter can be parsed correctly
    Validate(validate::Args),
    /// Lists tasks (`- [ ]` and `- [x]` list items) in markdown files
    Tasks(tasks::Args),
    /// Lists notes matching all given filters
    ///
    /// Tags are read from the `tags` field of the frontmatter. The title is the frontmatter
    /// `title`, otherwise the first level 1 heading, otherwise the file name.
    List(list::Args),
    /// Lists all tags used in the frontmatter of notes, with the number of notes using them
    ///
    /// Tags can be nested with `/`, a note tagged `lang/rust` also counts for `lang`.
    Tags(tags::Args),
    /// Creates a new markdown file from a template
    ///
    /// Templates are read from `.zl/templates/<name>.md` and rendered with Tera.
    /// The following variables are available in every template:
    ///
    ///   title      Title of the note (--title or note.default_title)
    ///   id         Zettel ID derived from the creation time, e.g. 20261031143005
    ///   random_id  Random ID of note.id_length characters of note.id_charset, e.g. 3f9a
    ///   date       Creation date, e.g. 2026-10-31
    ///   time       Creation time, e.g. 14:30
    ///   datetime   Creation timestamp in RFC 3339, e.g. 2026-10-31T14:30:05+01:00
    ///   filename   File name of the note without extension
    ///   extra      Values of [extra] in the config, merged with the [extra] of the note's group,
    ///              e.g. {{ extra.author }}
    ///
    /// Additional variables can be passed with --var KEY=VALUE. The `slug` filter turns text into
    /// a file name, e.g. {{ title | slug }} is `retrieval-practice` for "Retrieval Practice!".
    /// The `date` filter formats dates with strftime, e.g. {{ datetime | date(format="%A") }} is
    /// `Saturday` for 2026-10-31.
    ///
    /// Given a folder, the note is named by the Tera template note.filename, which has the same
    /// variables except `filename`, followed by note.extension. Names that are taken are numbered.
    /// For example, in .zl/config.toml:
    ///
    ///   [note]
    ///   filename = "{{ random_id }}"   # the default is "{{ id }}"
    ///
    ///   [group.journal]
    ///   paths = ["journal/**"]
    ///
    ///   [group.journal.note]
    ///   filename = "{{ date }}"        # zl new journal/ creates journal/2026-10-31.md
    ///
    /// If the note's folder matches the paths of a group in the config, the group's [note] and
    /// [extra] settings override the root ones. Use --group to choose a group explicitly.
    #[command(verbatim_doc_comment)]
    New(new::Args),
    /// Runs a language server on stdin and stdout, for navigating and linking notes in editors
    ///
    /// - Go to definition on a link opens the linked note
    /// - References list the links to the note under the cursor, or to the linked note if the
    ///   cursor is on a link
    /// - Hover on a link shows the start of the linked note
    /// - Completion in a link destination like `[text](` offers the paths of all notes
    /// - Diagnostics warn about links to notes that don't exist in open notes
    /// - Document symbols are the headings of a note, workspace symbols the notes by title
    ///
    /// Unsaved changes of open notes are taken into account.
    ///
    /// The notebook is the workspace folder of the editor. Neovim (0.11 or later) starts the
    /// server for markdown files in notebooks with:
    ///
    ///   vim.lsp.config('zl', { cmd = { 'zl', 'lsp' }, filetypes = { 'markdown' }, root_markers = { '.zl' } })
    ///   vim.lsp.enable('zl')
    #[command(verbatim_doc_comment)]
    Lsp(lsp::Args),
}
