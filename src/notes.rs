use std::{fs, path::{Path, PathBuf}};

use anyhow::{Context, bail};
use markdown::{Constructs, ParseOptions};

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
