use std::{cmp::Reverse, collections::BTreeMap, path::Path};

use clap::ValueEnum;

use crate::index::Index;

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sort {
    /// Alphabetically
    Name,
    /// Most used first
    Count,
}

struct TagUsage {
    /// Spelling of the first note using the tag, as tags differing only in case are counted together
    name: String,
    count: usize,
}

#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// Markdown file or directory to search
    #[arg(default_value = ".")]
    path: String,

    /// Order of the tags
    #[arg(short, long, value_enum, default_value_t = Sort::Name)]
    sort: Sort,
}

pub(crate) fn run(args: &Args) -> anyhow::Result<()> {
    // Keyed by the lowercase tag, matching the case-insensitive `zl list --tag`
    let mut usages: BTreeMap<String, TagUsage> = BTreeMap::new();

    let index = Index::open()?;
    index.warn_skipped();

    for note in index.notes(Path::new(&args.path))? {
        // A note listing the same tag twice is still a single use
        let mut note_tags: Vec<_> = note.tags.into_iter().map(|tag| (tag.to_lowercase(), tag)).collect();
        note_tags.sort();
        note_tags.dedup_by(|(a, _), (b, _)| a == b);

        for (key, tag) in note_tags {
            usages.entry(key).or_insert(TagUsage { name: tag, count: 0 }).count += 1;
        }
    }

    let mut usages: Vec<_> = usages.into_values().collect();
    if args.sort == Sort::Count {
        // Stable sort, so tags with the same count stay alphabetical
        usages.sort_by_key(|usage| Reverse(usage.count));
    }

    let width = usages.iter().map(|usage| usage.count.to_string().len()).max().unwrap_or(0);
    for usage in usages {
        println!("{:>width$}  {}", usage.count, usage.name);
    }

    Ok(())
}
