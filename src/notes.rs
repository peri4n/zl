use std::{fs, path::{Path, PathBuf}};

use anyhow::{Context, anyhow, bail};
use markdown::{Constructs, ParseOptions, mdast::Node, to_mdast};
use serde::Deserialize;

/// Metadata of a note, taken from its frontmatter and content
#[derive(Debug)]
pub(crate) struct Note {
    pub(crate) path: PathBuf,
    /// Frontmatter `title`, otherwise the first level 1 heading, otherwise the file name
    pub(crate) title: String,
    /// Frontmatter `tags`
    pub(crate) tags: Vec<String>,
}

impl Note {
    pub(crate) fn load(path: &Path, parse_options: &ParseOptions) -> anyhow::Result<Self> {
        let input = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let ast = to_mdast(&input, parse_options)
            .map_err(|e| anyhow!("Failed to parse {}: {}", path.display(), e))?;

        let frontmatter = Frontmatter::from_ast(&ast)
            .with_context(|| format!("Invalid frontmatter in {}", path.display()))?;

        let title = frontmatter.title
            .or_else(|| first_heading(&ast))
            .unwrap_or_else(|| path.file_stem().unwrap_or_default().to_string_lossy().into_owned());

        Ok(Note {
            path: path.to_path_buf(),
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
#[serde(untagged)]
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

/// Returns `path` itself if it is a file, otherwise all markdown files below it
pub(crate) fn collect(path: &Path) -> anyhow::Result<Vec<PathBuf>> {
    if !path.exists() {
        bail!("Path not found: {}", path.display());
    }

    if path.is_dir() {
        markdown_files(path)
            .with_context(|| format!("Failed to read directory {}", path.display()))
    } else {
        Ok(vec![path.to_path_buf()])
    }
}

/// Recursively collects all markdown files below `dir`, skipping hidden files and directories.
fn markdown_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
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
