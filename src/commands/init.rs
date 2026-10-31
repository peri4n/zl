use std::{fs, io::{ErrorKind, Write}, path::Path};

use anyhow::{Context, bail};

use crate::config::{CONFIG_DIR, Config};

#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// Directory to initialize [default: the notebook directory]
    path: Option<String>,
}

pub(crate) fn run(args: &Args) -> anyhow::Result<()> {
    let root_directory = Path::new(args.path.as_deref().unwrap_or("."));
    let config_dir = root_directory.join(CONFIG_DIR);
    let config_path = config_dir.join("config.toml");

    fs::create_dir_all(config_dir.join("templates"))
        .context("Failed to create templates directory")?;

    let config = toml::to_string_pretty(&Config::default()).context("Failed to serialize config")?;

    // `create_new` fails if the file exists, so an existing config is never overwritten
    let mut file = match fs::OpenOptions::new().write(true).create_new(true).open(&config_path) {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            bail!("Zettelkasten already initialized: {} exists", config_path.display())
        }
        Err(e) => return Err(e).context("Failed to create config file"),
    };

    file.write_all(config.as_bytes()).context("Failed to write config file")?;

    // Config and templates are meant to be committed, the index is a cache rebuilt from the notes.
    // An existing .gitignore is the user's, so it's left alone.
    let gitignore = config_dir.join(".gitignore");
    if !gitignore.exists() {
        fs::write(&gitignore, "index.db*\n").context("Failed to write .gitignore")?;
    }

    println!("Initialized zettelkasten directory with default configuration.");

    Ok(())
}
