use crate::config::Config;

pub(crate) fn run() -> Result<(), String> {
    let config = Config::from_path(".zl/config.toml")?;
    let output = toml::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;

    println!("{}", output);

    Ok(())
}
