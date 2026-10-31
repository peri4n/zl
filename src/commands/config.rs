use anyhow::Context;

use crate::config::{CONFIG_DIR, Config};

pub(crate) fn run() -> anyhow::Result<()> {
    let config = Config::from_path(&format!("{CONFIG_DIR}/config.toml"))?;
    let output = toml::to_string_pretty(&config).context("Failed to serialize config")?;

    println!("{}", output);

    Ok(())
}
