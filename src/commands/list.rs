use std::path::Path;

use regex::Regex;

use crate::notes::{self, Note};

pub(crate) fn run(path: &str, tags: &[String], title: Option<&Regex>) -> anyhow::Result<()> {
    let parse_options = notes::parse_options();

    for file in notes::collect(Path::new(path))? {
        // A single broken note shouldn't hide all other matches
        let note = match Note::load(&file, &parse_options) {
            Ok(note) => note,
            Err(e) => {
                eprintln!("warning: skipping {}: {:#}", file.display(), e);
                continue;
            }
        };

        if matches(&note, tags, title) {
            let tags = note.tags.iter().map(|tag| format!("#{}", tag)).collect::<Vec<_>>().join(" ");
            println!("{}", format!("{}  {}  {}", note.path.display(), note.title, tags).trim_end());
        }
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
