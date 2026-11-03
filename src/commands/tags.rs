use std::{cmp::Reverse, collections::BTreeMap, path::Path};

use clap::ValueEnum;
use termtree::Tree;

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

/// A tag with the tags nested below it, keyed by their lowercase last segment
struct TagNode {
    usage: TagUsage,
    children: BTreeMap<String, TagNode>,
}

#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// Markdown file or directory to search
    #[arg(default_value = ".")]
    path: String,

    /// Order of the tags
    #[arg(short, long, value_enum, default_value_t = Sort::Name)]
    sort: Sort,

    /// Show nested tags as a tree below their parents
    #[arg(long)]
    tree: bool,
}

pub(crate) fn run(args: &Args) -> anyhow::Result<()> {
    // Keyed by the lowercase tag, matching the case-insensitive `zl list --tag`
    let mut usages: BTreeMap<String, TagUsage> = BTreeMap::new();

    let index = Index::open()?;
    index.warn_skipped();

    for note in index.notes(Path::new(&args.path))? {
        // A note listing the same tag twice, or a tag and one nested below it, is still a single use
        let mut note_tags: Vec<_> = note
            .implied_tags()
            .map(|tag| (tag.to_lowercase(), tag.to_string()))
            .collect();
        note_tags.sort();
        note_tags.dedup_by(|(a, _), (b, _)| a == b);

        for (key, tag) in note_tags {
            usages
                .entry(key)
                .or_insert(TagUsage {
                    name: tag,
                    count: 0,
                })
                .count += 1;
        }
    }

    if args.tree {
        for node in sorted(nest(usages), args.sort) {
            print!("{}", render(node, args.sort));
        }
        return Ok(());
    }

    let mut usages: Vec<_> = usages.into_values().collect();
    if args.sort == Sort::Count {
        // Stable sort, so tags with the same count stay alphabetical
        usages.sort_by_key(|usage| Reverse(usage.count));
    }

    let width = usages
        .iter()
        .map(|usage| usage.count.to_string().len())
        .max()
        .unwrap_or(0);
    for usage in usages {
        println!("{:>width$}  {}", usage.count, usage.name);
    }

    Ok(())
}

/// Arranges the tags below their parents. As every parent of a nested tag is counted too, and is
/// ordered before it, the parents are always present.
fn nest(usages: BTreeMap<String, TagUsage>) -> BTreeMap<String, TagNode> {
    let mut roots: BTreeMap<String, TagNode> = BTreeMap::new();

    for (key, usage) in usages {
        let (parents, leaf) = key.rsplit_once('/').unwrap_or(("", &key));
        let mut siblings = &mut roots;
        for segment in parents.split('/').filter(|segment| !segment.is_empty()) {
            siblings = &mut siblings
                .get_mut(segment)
                .expect("parents are ordered before their nested tags")
                .children;
        }
        siblings.insert(
            leaf.to_string(),
            TagNode {
                usage,
                children: BTreeMap::new(),
            },
        );
    }

    roots
}

fn sorted(nodes: BTreeMap<String, TagNode>, sort: Sort) -> Vec<TagNode> {
    let mut nodes: Vec<_> = nodes.into_values().collect();
    if sort == Sort::Count {
        // Stable sort, so tags with the same count stay alphabetical
        nodes.sort_by_key(|node| Reverse(node.usage.count));
    }
    nodes
}

fn render(node: TagNode, sort: Sort) -> Tree<String> {
    let name = node.usage.name.rsplit('/').next().unwrap_or_default();
    Tree::new(format!("{} ({})", name, node.usage.count)).with_leaves(
        sorted(node.children, sort)
            .into_iter()
            .map(|child| render(child, sort)),
    )
}
