use anyhow::Context;

use crate::config::Config;

pub(crate) fn run() -> anyhow::Result<()> {
    let config = Config::load()?;
    let output = toml::to_string_pretty(&config).context("Failed to serialize config")?;

    println!("{}", output);

    Ok(())
}
