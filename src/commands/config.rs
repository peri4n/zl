use anyhow::Context;

use crate::config::{Config, ConfigDir, notebook_dir};

/// Prints the effective config. Outside of a zl notebook that's the default config, with a hint on
/// stderr so stdout stays valid TOML.
pub(crate) fn run() -> anyhow::Result<()> {
    let config_file = ConfigDir::current().config_file();
    if !config_file.exists() {
        eprintln!(
            "note: {} has no {}, showing the defaults. Run `zl init` to create it.",
            notebook_dir().display(),
            config_file.display()
        );
    }

    let config = Config::load_or_default()?;
    let output = toml::to_string_pretty(&config).context("Failed to serialize config")?;

    println!("{}", output);

    Ok(())
}
