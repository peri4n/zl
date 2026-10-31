use std::{collections::HashMap, fs, path::{Path, PathBuf}};

use anyhow::{Context, anyhow};
use markdown::{Constructs, ParseOptions, mdast::{ListItem, Node}, to_mdast};
use percent_encoding::percent_decode_str;
use serde::Deserialize;

use crate::paths::normalize;

/// Metadata of a note, taken from its frontmatter and content
#[derive(Debug)]
pub(crate) struct Note {
    /// Path relative to the notebook root
    pub(crate) path: PathBuf,
    /// Frontmatter `title`, otherwise the first level 1 heading, otherwise the file name
    pub(crate) title: String,
    /// Frontmatter `tags`
    pub(crate) tags: Vec<String>,
    /// All links in the note: markdown links and autolinks
    pub(crate) links: Vec<Link>,
    /// All task list items (`- [ ]` and `- [x]`), including nested ones
    pub(crate) tasks: Vec<Task>,
}

#[derive(Debug)]
pub(crate) struct Link {
    /// Target as written, e.g. `../a.md#intro`
    pub(crate) url: String,
    pub(crate) line: usize,
    /// Path of the linked note relative to the notebook root, `None` for links to anything else
    pub(crate) target: Option<PathBuf>,
}

#[derive(Debug)]
pub(crate) struct Task {
    pub(crate) line: usize,
    pub(crate) checked: bool,
    /// Raw markdown of the task's first paragraph, so links and inline code stay intact
    pub(crate) text: String,
}

impl Note {
    /// Reads and parses the note at `path`, relative to the notebook root. Fails on unreadable files and
    /// invalid frontmatter, errors don't mention `path`, callers are expected to report it.
    pub(crate) fn load(path: &Path, parse_options: &ParseOptions) -> anyhow::Result<Self> {
        let path = normalize(path);
        let input = fs::read_to_string(&path).context("Failed to read file")?;
        let ast = to_mdast(&input, parse_options).map_err(|e| anyhow!("Failed to parse markdown: {}", e))?;

        let frontmatter = Frontmatter::from_ast(&ast).context("Invalid frontmatter")?;

        let title = frontmatter.title
            .or_else(|| first_heading(&ast))
            .unwrap_or_else(|| path.file_stem().unwrap_or_default().to_string_lossy().into_owned());

        Ok(Note {
            links: links(&ast, &path),
            tasks: tasks(&ast, &input),
            path,
            title,
            tags: frontmatter.tags.map(Tags::into_vec).unwrap_or_default(),
        })
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Frontmatter {
    title: Option<String>,
    tags: Option<Tags>,
}

impl Frontmatter {
    /// Frontmatter is always the first node of the document, if present
    fn from_ast(ast: &Node) -> anyhow::Result<Self> {
        let frontmatter = match ast.children().and_then(|children| children.first()) {
            Some(Node::Yaml(yaml)) => serde_norway::from_str::<Option<Frontmatter>>(&yaml.value)?.unwrap_or_default(),
            Some(Node::Toml(toml)) => toml::from_str(&toml.value)?,
            _ => Frontmatter::default(),
        };

        Ok(frontmatter)
    }
}

/// Tags as a list (`tags: [a, b]`) or as a single string (`tags: a, b` or `tags: a b`)
#[derive(Debug, Deserialize)]
#[serde(untagged, expecting = "expected `tags` to be a list or a comma separated string")]
enum Tags {
    List(Vec<String>),
    Text(String),
}

impl Tags {
    fn into_vec(self) -> Vec<String> {
        let tags = match self {
            Tags::List(tags) => tags,
            Tags::Text(text) => text.split([',', ' ']).map(str::to_string).collect(),
        };

        tags.iter()
            .map(|tag| tag.trim().trim_start_matches('#'))
            .filter(|tag| !tag.is_empty())
            .map(str::to_string)
            .collect()
    }
}

/// Links in document order. Reference links (`[text][ref]`) use the URL of their definition,
/// code is skipped as markdown doesn't parse links in it.
fn links(ast: &Node, note_path: &Path) -> Vec<Link> {
    let mut definitions = HashMap::new();
    walk(ast, &mut |node| {
        if let Node::Definition(definition) = node {
            definitions.insert(definition.identifier.clone(), definition.url.clone());
        }
    });

    let mut links = Vec::new();
    walk(ast, &mut |node| {
        let url = match node {
            Node::Link(link) => Some(&link.url),
            Node::LinkReference(reference) => definitions.get(&reference.identifier),
            _ => None,
        };

        if let Some(url) = url {
            links.push(Link {
                url: url.clone(),
                line: node.position().map_or(0, |position| position.start.line),
                target: resolve_link(note_path, url),
            });
        }
    });

    links
}

/// Path of the note `url` links to, relative to the notebook root, or `None` if it links to anything
/// else than a markdown file. Relative URLs are resolved against the folder of `note_path`, URLs
/// starting with `/` against the notebook root.
pub(crate) fn resolve_link(note_path: &Path, url: &str) -> Option<PathBuf> {
    if has_scheme(url) {
        return None;
    }

    let path = url.split(['#', '?']).next().filter(|path| !path.is_empty())?;
    let path = percent_decode_str(path).decode_utf8().ok()?;

    let target = match path.strip_prefix('/') {
        Some(from_root) => PathBuf::from(from_root),
        None => note_path.parent().unwrap_or(Path::new("")).join(path.as_ref()),
    };
    let target = normalize(&target);

    target.extension().is_some_and(|ext| ext == "md").then_some(target)
}

/// Whether `url` starts with a URI scheme like `https:` or `mailto:`
fn has_scheme(url: &str) -> bool {
    url.split_once(':').is_some_and(|(scheme, _)| {
        scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

fn tasks(ast: &Node, input: &str) -> Vec<Task> {
    let mut tasks = Vec::new();
    walk(ast, &mut |node| {
        if let Node::ListItem(item) = node
            && let Some(checked) = item.checked
        {
            tasks.push(Task {
                line: item.position.as_ref().map_or(0, |position| position.start.line),
                checked,
                text: task_text(item, input),
            });
        }
    });

    tasks
}

/// The task's first paragraph with line breaks and repeated spaces collapsed into single spaces
fn task_text(item: &ListItem, input: &str) -> String {
    let Some(position) = item.children.first().and_then(Node::position) else {
        return String::new();
    };

    input[position.start.offset..position.end.offset]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn walk<'a>(node: &'a Node, visit: &mut impl FnMut(&'a Node)) {
    visit(node);
    for child in node.children().into_iter().flatten() {
        walk(child, visit);
    }
}

fn first_heading(node: &Node) -> Option<String> {
    match node {
        Node::Heading(heading) if heading.depth == 1 => Some(node.to_string()),
        _ => node.children()?.iter().find_map(first_heading),
    }
}

/// Markdown dialect of the notes: GitHub flavored markdown with frontmatter
pub(crate) fn parse_options() -> ParseOptions {
    ParseOptions {
        constructs: Constructs {
            frontmatter: true,
            ..Constructs::gfm()
        },
        ..ParseOptions::gfm()
    }
}

/// Recursively collects all markdown files below `dir`, skipping hidden files and directories.
pub(crate) fn markdown_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for entry in fs::read_dir(dir)? {
        let path = entry?.path();

        if path.file_name().is_some_and(|name| name.to_string_lossy().starts_with('.')) {
            continue;
        }

        if path.is_dir() {
            files.extend(markdown_files(&path)?);
        } else if path.extension().is_some_and(|ext| ext == "md") {
            files.push(path);
        }
    }

    files.sort();

    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(note: &str, url: &str) -> Option<PathBuf> {
        resolve_link(Path::new(note), url)
    }

    #[test]
    fn resolves_relative_links() {
        assert_eq!(resolve("a.md", "b.md"), Some("b.md".into()));
        assert_eq!(resolve("dir/a.md", "b.md"), Some("dir/b.md".into()));
        assert_eq!(resolve("dir/a.md", "./sub/b.md"), Some("dir/sub/b.md".into()));
        assert_eq!(resolve("dir/a.md", "../b.md"), Some("b.md".into()));
        assert_eq!(resolve("dir/a.md", "/other/b.md"), Some("other/b.md".into()));
    }

    #[test]
    fn strips_anchors_and_decodes() {
        assert_eq!(resolve("a.md", "b.md#intro"), Some("b.md".into()));
        assert_eq!(resolve("a.md", "b.md?x=1"), Some("b.md".into()));
        assert_eq!(resolve("a.md", "note%20one.md"), Some("note one.md".into()));
        assert_eq!(resolve("a.md", "my note.md"), Some("my note.md".into()));
    }

    #[test]
    fn ignores_everything_but_notes() {
        assert_eq!(resolve("a.md", "https://example.com/b.md"), None);
        assert_eq!(resolve("a.md", "mailto:me@example.com"), None);
        assert_eq!(resolve("a.md", "#section"), None);
        assert_eq!(resolve("a.md", "image.png"), None);
        assert_eq!(resolve("a.md", ""), None);
    }
}
