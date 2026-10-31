use std::{fs, path::{Path, PathBuf}};

use markdown::{ParseOptions, to_mdast};

pub(crate) fn run(path: &str) -> Result<(), String> {
    let path = Path::new(path);

    if !path.exists() {
        return Err("File not found".to_string());
    }

    let parse_options = ParseOptions {
        constructs: markdown::Constructs {
            frontmatter: true,
            ..markdown::Constructs::default()
        },
        ..ParseOptions::default()
    };

    if path.is_dir() {
        println!("Parsing all markdown files in directory: {}", path.display());
        for file in walk_dir(path) {

            if file.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }

            let input = fs::read_to_string(file).map_err(|_| "File not found")?;
            let _ = to_mdast(&input, &parse_options).expect("Failed to parse markdown");

        }
        return Ok(());
    }

    let input = fs::read_to_string(path).map_err(|_| "File not found")?;
    let ast = to_mdast(&input, &parse_options).expect("Failed to parse markdown");
    println!("{:#?}", ast);

    Ok(())
}

fn walk_dir(path: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    if path.is_file() {
        files.push(path.to_path_buf());
    }

    // Recursively walk the directory unless it starts with a dot
    if path.is_dir() && !path.file_name().unwrap().to_str().unwrap().starts_with('.') {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();

            if path.is_dir() {
                files.extend(walk_dir(&path));
            } else {
                files.push(path);
            }
        }
    }

    files
}
