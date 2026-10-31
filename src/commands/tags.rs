use std::{cmp::Reverse, collections::BTreeMap, path::Path};

use clap::ValueEnum;

use crate::notes;

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

pub(crate) fn run(path: &str, sort: Sort) -> anyhow::Result<()> {
    // Keyed by the lowercase tag, matching the case-insensitive `zl list --tag`
    let mut usages: BTreeMap<String, TagUsage> = BTreeMap::new();

    for note in notes::load_all(Path::new(path))? {
        // A note listing the same tag twice is still a single use
        let mut note_tags: Vec<_> = note.tags.into_iter().map(|tag| (tag.to_lowercase(), tag)).collect();
        note_tags.sort();
        note_tags.dedup_by(|(a, _), (b, _)| a == b);

        for (key, tag) in note_tags {
            usages.entry(key).or_insert(TagUsage { name: tag, count: 0 }).count += 1;
        }
    }

    let mut usages: Vec<_> = usages.into_values().collect();
    if sort == Sort::Count {
        // Stable sort, so tags with the same count stay alphabetical
        usages.sort_by_key(|usage| Reverse(usage.count));
    }

    let width = usages.iter().map(|usage| usage.count.to_string().len()).max().unwrap_or(0);
    for usage in usages {
        println!("{:>width$}  {}", usage.count, usage.name);
    }

    Ok(())
}
