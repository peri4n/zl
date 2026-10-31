use std::{fs, path::{Path, PathBuf}};

use markdown::{ParseOptions, to_mdast};

pub(crate) fn run(path: &str) -> Result<(), String> {
    let path = Path::new(path);

    if !path.exists() {
        return Err(format!("Path not found: {}", path.display()));
    }

    let files = if path.is_dir() {
        markdown_files(path)
            .map_err(|e| format!("Failed to read directory {}: {}", path.display(), e))?
    } else {
        vec![path.to_path_buf()]
    };

    let parse_options = ParseOptions {
        constructs: markdown::Constructs {
            frontmatter: true,
            ..markdown::Constructs::default()
        },
        ..ParseOptions::default()
    };

    let mut failed = 0;
    for file in &files {
        match validate_file(file, &parse_options) {
            Ok(()) => println!("ok    {}", file.display()),
            Err(reason) => {
                failed += 1;
                println!("FAIL  {}: {}", file.display(), reason);
            }
        }
    }

    println!("\n{} file(s) checked, {} failed", files.len(), failed);

    if failed > 0 {
        return Err(format!("{} file(s) failed validation", failed));
    }

    Ok(())
}

fn validate_file(path: &Path, parse_options: &ParseOptions) -> Result<(), String> {
    let input = fs::read_to_string(path).map_err(|e| e.to_string())?;
    to_mdast(&input, parse_options).map_err(|e| e.to_string())?;

    Ok(())
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
