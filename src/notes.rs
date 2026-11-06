use std::{
    collections::HashMap,
    fs,
    ops::Range,
    path::{Path, PathBuf},
};

use anyhow::{Context, anyhow};
use markdown::{
    Constructs, ParseOptions,
    mdast::{ListItem, Node, Root},
    to_mdast,
};
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
    /// Frontmatter `tags`, nested ones separated by `/`, e.g. `lang/rust`
    pub(crate) tags: Vec<String>,
    /// All links in the note: markdown links and autolinks
    pub(crate) links: Vec<Link>,
    /// All task list items (`- [ ]` and `- [x]`), including nested ones
    pub(crate) tasks: Vec<Task>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Link {
    /// Target as written, e.g. `../a.md#intro`
    pub(crate) url: String,
    pub(crate) line: usize,
    /// Byte offsets of the whole link in the note, e.g. of `[text](url)`
    pub(crate) span: Range<usize>,
    /// Path of the linked note relative to the notebook root, `None` for links to anything else
    pub(crate) target: Option<PathBuf>,
}

/// A note as it is being edited in the language server. Unlike [`Note::load`] parsing never fails, so
/// it also works while the frontmatter is invalid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Draft {
    /// Like [`Note::title`], ignoring invalid frontmatter
    pub(crate) title: String,
    /// Headings with text, in document order
    pub(crate) headings: Vec<Heading>,
    pub(crate) links: Vec<Link>,
    /// Byte offset of the content after the frontmatter
    pub(crate) body_start: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Heading {
    /// 1 for `#`, up to 6 for `######`
    pub(crate) depth: u8,
    pub(crate) text: String,
    /// Byte offsets of the whole heading in the note
    pub(crate) span: Range<usize>,
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
        let ast = to_mdast(&input, parse_options)
            .map_err(|e| anyhow!("Failed to parse markdown: {}", e))?;

        let frontmatter = Frontmatter::from_ast(&ast).context("Invalid frontmatter")?;

        Ok(Note {
            title: title(frontmatter.title, &ast, &path),
            links: links(&ast, &path),
            tasks: tasks(&ast, &input),
            path,
            tags: frontmatter.tags.map(Tags::into_vec).unwrap_or_default(),
        })
    }

    /// The tags of the note together with their ancestors, as a nested tag implies its parents:
    /// `a/b/c` yields `a`, `a/b` and `a/b/c`. May yield the same tag several times.
    pub(crate) fn implied_tags(&self) -> impl Iterator<Item = &str> {
        self.tags.iter().flat_map(|tag| {
            tag.match_indices('/')
                .map(|(end, _)| &tag[..end])
                .chain([tag.as_str()])
        })
    }
}

/// Removes a leading `#` and empty segments of nested tags, e.g. `#a//b/ ` becomes `a/b`
pub(crate) fn normalize_tag(tag: &str) -> String {
    tag.trim()
        .trim_start_matches('#')
        .split('/')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("/")
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
            Some(Node::Yaml(yaml)) => {
                serde_norway::from_str::<Option<Frontmatter>>(&yaml.value)?.unwrap_or_default()
            }
            Some(Node::Toml(toml)) => toml::from_str(&toml.value)?,
            _ => Frontmatter::default(),
        };

        Ok(frontmatter)
    }
}

/// Tags as a list (`tags: [a, b]`) or as a single string (`tags: a, b` or `tags: a b`)
#[derive(Debug, Deserialize)]
#[serde(
    untagged,
    expecting = "expected `tags` to be a list or a comma separated string"
)]
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
            .map(|tag| normalize_tag(tag))
            .filter(|tag| !tag.is_empty())
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
            let position = node.position();
            links.push(Link {
                url: url.clone(),
                line: position.map_or(0, |position| position.start.line),
                span: position.map_or(0..0, |position| position.start.offset..position.end.offset),
                target: resolve_link(note_path, url),
            });
        }
    });

    links
}

impl Draft {
    /// Parses the note at `note_path`, relative to the notebook root, with content `input`
    pub(crate) fn parse(note_path: &Path, input: &str) -> Self {
        // Markdown without MDX constructs always parses, errors are only possible for MDX
        let ast = to_mdast(input, &parse_options()).unwrap_or_else(|_| {
            Node::Root(Root {
                children: Vec::new(),
                position: None,
            })
        });

        let frontmatter = Frontmatter::from_ast(&ast).unwrap_or_default();
        let body_start = match ast.children().and_then(|children| children.first()) {
            Some(node @ (Node::Yaml(_) | Node::Toml(_))) => {
                node.position().map_or(0, |position| position.end.offset)
            }
            _ => 0,
        };

        Draft {
            title: title(frontmatter.title, &ast, note_path),
            headings: headings(&ast),
            links: links(&ast, note_path),
            body_start,
        }
    }
}

fn headings(ast: &Node) -> Vec<Heading> {
    let mut headings = Vec::new();
    walk(ast, &mut |node| {
        if let Node::Heading(heading) = node
            && let Some(position) = &heading.position
        {
            let text = node.to_string();
            if !text.trim().is_empty() {
                headings.push(Heading {
                    depth: heading.depth,
                    text,
                    span: position.start.offset..position.end.offset,
                });
            }
        }
    });

    headings
}

/// URL of a link from the note at `note_path` to the note at `target`, both relative to the notebook
/// root: relative to the folder of `note_path` and percent-encoded where markdown requires it, so
/// [`resolve_link`] resolves it to `target` again.
pub(crate) fn link_url(note_path: &Path, target: &Path) -> String {
    let from: Vec<_> = note_path.parent().into_iter().flatten().collect();
    let to: Vec<_> = target.iter().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();

    let segments = std::iter::repeat_n("..".to_string(), from.len() - common).chain(
        to[common..]
            .iter()
            .map(|segment| encode_url_segment(&segment.to_string_lossy())),
    );

    segments.collect::<Vec<_>>().join("/")
}

/// Percent-encodes the characters that would end a link destination or change its meaning. Other
/// characters, including non-ASCII ones, stay readable.
fn encode_url_segment(segment: &str) -> String {
    segment
        .chars()
        .map(|c| match c {
            ' ' | '%' | '(' | ')' | '<' | '>' | '[' | ']' | '#' | '?' | '\\' | '`' => {
                format!("%{:02X}", c as u32)
            }
            c if c.is_ascii_control() => format!("%{:02X}", c as u32),
            c => c.to_string(),
        })
        .collect()
}

/// Path of the note `url` links to, relative to the notebook root, or `None` if it links to anything
/// else than a markdown file. Relative URLs are resolved against the folder of `note_path`, URLs
/// starting with `/` against the notebook root. URLs without an extension, like `Some%20Note`, link
/// to the note of that name, `Some Note.md`.
pub(crate) fn resolve_link(note_path: &Path, url: &str) -> Option<PathBuf> {
    if has_scheme(url) {
        return None;
    }

    let path = url
        .split(['#', '?'])
        .next()
        .filter(|path| !path.is_empty())?;
    let path = percent_decode_str(path).decode_utf8().ok()?;

    let target = match path.strip_prefix('/') {
        Some(from_root) => PathBuf::from(from_root),
        None => note_path
            .parent()
            .unwrap_or(Path::new(""))
            .join(path.as_ref()),
    };
    let target = normalize(&target);

    match target.extension() {
        Some(ext) if ext == "md" => Some(target),
        // A trailing `/` links to a folder, not a note
        None if !path.ends_with('/') => Some(target.with_extension("md")),
        _ => None,
    }
}

/// Whether `url` starts with a URI scheme like `https:` or `mailto:`
fn has_scheme(url: &str) -> bool {
    url.split_once(':').is_some_and(|(scheme, _)| {
        scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

fn tasks(ast: &Node, input: &str) -> Vec<Task> {
    let mut tasks = Vec::new();
    walk(ast, &mut |node| {
        if let Node::ListItem(item) = node
            && let Some(checked) = item.checked
        {
            tasks.push(Task {
                line: item
                    .position
                    .as_ref()
                    .map_or(0, |position| position.start.line),
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

/// The frontmatter title, otherwise the first level 1 heading, otherwise the file name of the note
fn title(frontmatter_title: Option<String>, ast: &Node, note_path: &Path) -> String {
    frontmatter_title
        .or_else(|| first_heading(ast))
        .unwrap_or_else(|| {
            note_path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        })
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

        if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with('.'))
        {
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
        assert_eq!(
            resolve("dir/a.md", "./sub/b.md"),
            Some("dir/sub/b.md".into())
        );
        assert_eq!(resolve("dir/a.md", "../b.md"), Some("b.md".into()));
        assert_eq!(
            resolve("dir/a.md", "/other/b.md"),
            Some("other/b.md".into())
        );
    }

    #[test]
    fn strips_anchors_and_decodes() {
        assert_eq!(resolve("a.md", "b.md#intro"), Some("b.md".into()));
        assert_eq!(resolve("a.md", "b.md?x=1"), Some("b.md".into()));
        assert_eq!(resolve("a.md", "note%20one.md"), Some("note one.md".into()));
        assert_eq!(resolve("a.md", "my note.md"), Some("my note.md".into()));
    }

    #[test]
    fn resolves_links_without_extension() {
        assert_eq!(resolve("a.md", "b"), Some("b.md".into()));
        assert_eq!(
            resolve("dir/a.md", "Retrieval%20Practice"),
            Some("dir/Retrieval Practice.md".into())
        );
        assert_eq!(resolve("a.md", "b#intro"), Some("b.md".into()));
        assert_eq!(resolve("a.md", "dir/"), None);
    }

    #[test]
    fn keeps_spans_of_links() {
        let input = "[a](a.md) <https://example.com>\n\n[b][ref]\n\n[ref]: b.md\n";
        let spans: Vec<_> = Draft::parse(Path::new("n.md"), input)
            .links
            .into_iter()
            .map(|link| (link.url, &input[link.span]))
            .collect();

        assert_eq!(
            spans,
            [
                ("a.md".to_string(), "[a](a.md)"),
                ("https://example.com".to_string(), "<https://example.com>"),
                ("b.md".to_string(), "[b][ref]"),
            ]
        );
    }

    #[test]
    fn parses_drafts_despite_invalid_frontmatter() {
        let input = "---\ntags: [\n---\n# A\n[a](a.md)\n";
        let draft = Draft::parse(Path::new("n.md"), input);

        assert_eq!(draft.title, "A");
        assert_eq!(draft.links[0].target, Some("a.md".into()));
        assert_eq!(&input[draft.body_start..], "\n# A\n[a](a.md)\n");
    }

    #[test]
    fn parses_titles_and_headings_of_drafts() {
        let input = "---\ntitle: Title\n---\n# One\n\n## Two\n\n#\n\n### Three *x*\n";
        let draft = Draft::parse(Path::new("dir/n.md"), input);

        assert_eq!(draft.title, "Title");
        let headings: Vec<_> = draft
            .headings
            .iter()
            .map(|heading| {
                (
                    heading.depth,
                    heading.text.as_str(),
                    &input[heading.span.clone()],
                )
            })
            .collect();
        assert_eq!(
            headings,
            [
                (1, "One", "# One"),
                (2, "Two", "## Two"),
                (3, "Three x", "### Three *x*"),
            ]
        );

        assert_eq!(Draft::parse(Path::new("dir/n.md"), "text").title, "n");
        assert_eq!(Draft::parse(Path::new("n.md"), "text").body_start, 0);
    }

    #[test]
    fn builds_urls_that_resolve_to_the_target() {
        for (note, target, url) in [
            ("a.md", "b.md", "b.md"),
            ("a.md", "dir/b.md", "dir/b.md"),
            ("dir/a.md", "b.md", "../b.md"),
            ("dir/sub/a.md", "dir/other/b.md", "../other/b.md"),
            ("a.md", "Grüne Notiz (1).md", "Grüne%20Notiz%20%281%29.md"),
            ("a.md", "50% #1?.md", "50%25%20%231%3F.md"),
        ] {
            assert_eq!(link_url(Path::new(note), Path::new(target)), url);
            assert_eq!(resolve(note, url), Some(target.into()), "{url}");
        }
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
