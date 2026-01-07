mod commands;
mod config;

use std::{fs, path::{Path, PathBuf}, sync::LazyLock};

use clap::Parser;
use markdown::{ParseOptions, to_mdast};
use tera::{Context, Tera};

use crate::config::Config;
use crate::commands::{Cli, Command};

const TEMPLATES: LazyLock<Tera> = LazyLock::new(|| {
    let tera = match Tera::new(".zl/templates/**/*") {
        Ok(t) => t,
        Err(e) => {
            println!("Parsing error(s): {}", e);
            ::std::process::exit(1);
        }
    };
    tera
});

fn main() -> Result<(), String> {
    let cli = Cli::parse();

    match &cli.cmd {
        Command::Config => {
            let config = Config::from_path(".zl/config.toml")?;
            let output = toml::to_string_pretty(&config)
                .map_err(|e| format!("Failed to serialize config: {}", e))?;

            println!("{}", output);
        },
        Command::Init { path }=> {
            let directory = path.as_deref().unwrap_or(".");

            fs::create_dir_all(format!("{}/.zl/templates", directory))
                .map_err(|e| format!("Failed to create templates directory: {}", e))?;

            fs::write(format!("{}/.zl/config.toml", directory), toml::to_string_pretty(&Config::default())
                .map_err(|e| format!("Failed to serialize config: {}", e))?)
                .map_err(|e| format!("Failed to write config file: {}", e))?;

            println!("Initialized zettelkasten directory with default configuration.");
        },
        Command::Parse { path } => {
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
        },
        Command::New { path, template } => {
            let mut context = Context::new();
            context.insert("title", "test");
            let expr = TEMPLATES.render(&format!("{}.md", template), &context)
                .map_err(|e| e.to_string())?;

            fs::write(path, expr)
                .map_err(|e| format!("Can not write file: {}", e.to_string()))?
        },
    }

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
