use std::{fs, io::{ErrorKind, Write}, path::Path};

use crate::config::{CONFIG_DIR, Config};

pub(crate) fn run(path: Option<&str>) -> Result<(), String> {
    let root_directory = Path::new(path.unwrap_or("."));
    let config_dir = root_directory.join(CONFIG_DIR);
    let config_path = config_dir.join("config.toml");

    fs::create_dir_all(config_dir.join("templates"))
        .map_err(|e| format!("Failed to create templates directory: {}", e))?;

    let config = toml::to_string_pretty(&Config::default())
        .map_err(|e| format!("Failed to serialize config: {}", e))?;

    // `create_new` fails if the file exists, so an existing config is never overwritten
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&config_path)
        .map_err(|e| match e.kind() {
            ErrorKind::AlreadyExists => format!("Zettelkasten already initialized: {} exists", config_path.display()),
            _ => format!("Failed to create config file: {}", e),
        })?;

    file.write_all(config.as_bytes())
        .map_err(|e| format!("Failed to write config file: {}", e))?;

    println!("Initialized zettelkasten directory with default configuration.");

    Ok(())
}
