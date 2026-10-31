use std::{collections::HashMap, path::Path};

use anyhow::bail;

use crate::{index::Index, notes::{self, Note}};

pub(crate) fn run(path: &str) -> anyhow::Result<()> {
    let files = notes::collect(Path::new(path))?;
    let parse_options = notes::parse_options();

    // Whether a link target exists depends on the whole notebook, so that comes from the index.
    // Its warnings about unparsable notes are skipped, those are reported below.
    let mut broken_links: HashMap<_, Vec<_>> = HashMap::new();
    for link in Index::open()?.broken_links()? {
        broken_links.entry(link.path.clone()).or_default().push(link);
    }

    let mut failed = 0;
    for file in &files {
        // Loading a note parses its markdown and frontmatter, so it fails on everything we check
        match Note::load(file, &parse_options) {
            Ok(note) => match broken_links.get(&note.path) {
                None => println!("ok    {}", file.display()),
                Some(links) => {
                    failed += 1;
                    for link in links {
                        println!("FAIL  {}:{}: broken link `{}`", file.display(), link.line, link.url);
                    }
                }
            },
            Err(reason) => {
                failed += 1;
                println!("FAIL  {}: {:#}", file.display(), reason);
            }
        }
    }

    println!("\n{} file(s) checked, {} failed", files.len(), failed);

    if failed > 0 {
        bail!("{} file(s) failed validation", failed);
    }

    Ok(())
}
