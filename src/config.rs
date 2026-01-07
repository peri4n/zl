use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Config {
    note: NoteConfig,
}

impl Config {
    pub(crate) fn from_path(path: &str) -> Result<Self, String> {
        if std::path::Path::new(path).exists() {
            let mut default_config = Config::default();

            let config_str = std::fs::read_to_string(path)
                .map_err(|e| format!("Failed to read config file: {}", e))?;
            let stored_config = toml::from_str(&config_str)
                .map_err(|e| format!("Failed to parse config file: {}", e))?;

            default_config.override_with(&stored_config);
            Ok(default_config)
        } else {
            Err(format!("Config file not found at path: {}.", path))
        }
    }

    fn override_with(&mut self, other: &Config) {
        self.note.override_with(&other.note);
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            note: NoteConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct NoteConfig {
    default_template: String,

    default_title: String,
}

impl NoteConfig {
    fn override_with(&mut self, other: &NoteConfig) {
        self.default_template = other.default_template.clone();
        self.default_title = other.default_title.clone();
    }
}

impl Default for NoteConfig {
    fn default() -> Self {
        NoteConfig {
            default_template: "default".to_string(),
            default_title: "Untitled".to_string(),
        }
    }
}
