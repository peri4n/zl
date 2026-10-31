use std::{fs, io::{ErrorKind, Write}, path::Path};

use anyhow::{Context, bail};

use crate::config::{CONFIG_DIR, Config};

pub(crate) fn run(path: Option<&str>) -> anyhow::Result<()> {
    let root_directory = Path::new(path.unwrap_or("."));
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

    println!("Initialized zettelkasten directory with default configuration.");

    Ok(())
}
