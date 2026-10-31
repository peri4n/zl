mod format;

use std::path::Path;

use anyhow::anyhow;
use regex::Regex;

use crate::{config::{CONFIG_DIR, Config}, notes::{self, Note}};

pub(crate) use format::Format;

pub(crate) fn run(path: &str, tags: &[String], title: Option<&Regex>, format: Option<&Format>) -> anyhow::Result<()> {
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

    for note in notes::load_all(Path::new(path))?.filter(|note| matches(note, tags, title)) {
        let line = format.render(&note);
        println!("{}", if trim { line.trim_end() } else { &line });
    }

    Ok(())
}

fn matches(note: &Note, tags: &[String], title: Option<&Regex>) -> bool {
    let has_all_tags = tags.iter().all(|wanted| {
        let wanted = wanted.trim_start_matches('#').to_lowercase();
        note.tags.iter().any(|tag| tag.to_lowercase() == wanted)
    });

    has_all_tags && title.is_none_or(|regex| regex.is_match(&note.title))
}
