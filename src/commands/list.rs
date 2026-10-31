mod format;

use std::path::{Path, PathBuf};

use anyhow::anyhow;
use clap::Args;
use regex::Regex;

use crate::{config::{CONFIG_DIR, Config}, index::Index, notes::Note};

pub(crate) use format::Format;

pub(crate) fn run(path: &str, filter: &Filter, format: Option<&Format>) -> anyhow::Result<()> {
    // The configured format is trimmed so notes without tags don't end in spaces,
    // an explicit --format is printed exactly as given
    let configured_format;
    let (format, trim) = match format {
        Some(format) => (format, false),
        None => {
            let config = Config::from_path_or_default(&format!("{CONFIG_DIR}/config.toml"))?;
            configured_format = config.list_format()
                .parse::<Format>()
                .map_err(|e| anyhow!("Invalid list.format in config: {}", e))?;
            (&configured_format, true)
        }
    };

    let index = Index::open()?;
    index.warn_skipped();

    for note in filter.apply(&index, Path::new(path))? {
        let line = format.render(&note);
        println!("{}", if trim { line.trim_end() } else { &line });
    }

    Ok(())
}

/// Criteria a note has to meet to be listed, all given ones have to match
#[derive(Args, Debug, Clone)]
pub(crate) struct Filter {
    /// Only list notes with this tag (case-insensitive), can be repeated to require several tags
    #[arg(short, long = "tag", value_name = "TAG")]
    tags: Vec<String>,

    /// Only list notes whose title matches this regex, e.g. '(?i)^rust'
    #[arg(long, value_name = "REGEX")]
    title: Option<Regex>,

    /// Only list notes without any links: markdown links and URLs
    #[arg(long)]
    no_links: bool,

    /// Only list notes linking to this note, i.e. its backlinks
    #[arg(long, value_name = "NOTE")]
    links_to: Option<PathBuf>,

    /// Only list notes no other note links to
    #[arg(long)]
    orphan: bool,
}

impl Filter {
    /// Notes at or below `path` matching all criteria
    fn apply(&self, index: &Index, path: &Path) -> anyhow::Result<Vec<Note>> {
        // Link criteria depend on all notes of the notebook, not just the note itself
        let backlinks = self.links_to.as_deref().map(|note| index.backlinks(note)).transpose()?;
        let orphans = if self.orphan { Some(index.orphans()?) } else { None };

        let notes = index.notes(path)?.into_iter().filter(|note| {
            self.matches(note)
                && backlinks.as_ref().is_none_or(|paths| paths.contains(&note.path))
                && orphans.as_ref().is_none_or(|paths| paths.contains(&note.path))
        });

        Ok(notes.collect())
    }

    fn matches(&self, note: &Note) -> bool {
        let has_all_tags = self.tags.iter().all(|wanted| {
            let wanted = wanted.trim_start_matches('#').to_lowercase();
            note.tags.iter().any(|tag| tag.to_lowercase() == wanted)
        });

        has_all_tags
            && self.title.as_ref().is_none_or(|regex| regex.is_match(&note.title))
            && (!self.no_links || note.links.is_empty())
    }
}
