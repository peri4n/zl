use std::fs;

use crate::config::Config;

pub(crate) fn run(path: Option<&str>) -> Result<(), String> {
    let directory = path.unwrap_or(".");

    fs::create_dir_all(format!("{}/.zl/templates", directory))
        .map_err(|e| format!("Failed to create templates directory: {}", e))?;

    fs::write(format!("{}/.zl/config.toml", directory), toml::to_string_pretty(&Config::default())
        .map_err(|e| format!("Failed to serialize config: {}", e))?)
        .map_err(|e| format!("Failed to write config file: {}", e))?;

    println!("Initialized zettelkasten directory with default configuration.");

    Ok(())
}
