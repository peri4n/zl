use std::{collections::BTreeMap, fs, path::{Component, Path, PathBuf}};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

/// Name of the directory holding the zettelkasten configuration and templates
pub(crate) const CONFIG_DIR: &str = ".zl";

/// Missing sections and fields fall back to their defaults
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct Config {
    note: NoteConfig,

    template: TemplateConfig,
}

impl Config {
    pub(crate) fn from_path(path: &str) -> anyhow::Result<Self> {
        if !Path::new(path).exists() {
            bail!("Config file not found at path: {}", path);
        }

        let config_str = fs::read_to_string(path).context("Failed to read config file")?;
        let mut config: Config = toml::from_str(&config_str).context("Failed to parse config file")?;

        config.template.folders = config.template.folders
            .into_iter()
            .map(|(folder, template)| (normalize(&folder), template))
            .collect();

        Ok(config)
    }

    /// Template for a new note at `note_path`: the rule of the most specific folder
    /// containing the note, or `note.default_template` if no rule matches
    pub(crate) fn template_for(&self, note_path: &Path) -> &str {
        let Some(parent) = normalize(note_path).parent().map(Path::to_path_buf) else {
            return &self.note.default_template;
        };

        self.template.folders
            .iter()
            .filter(|(folder, _)| parent.starts_with(folder))
            .max_by_key(|(folder, _)| folder.components().count())
            .map_or(&self.note.default_template, |(_, template)| template)
    }

    pub(crate) fn default_title(&self) -> &str {
        &self.note.default_title
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
struct NoteConfig {
    default_template: String,

    default_title: String,
}

impl Default for NoteConfig {
    fn default() -> Self {
        NoteConfig {
            default_template: "default".to_string(),
            default_title: "Untitled".to_string(),
        }
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct TemplateConfig {
    /// Folder (relative to the zettelkasten root) mapped to the template applied to new notes in it.
    /// Folders are normalized when the config is loaded.
    folders: BTreeMap<PathBuf, String>,
}

/// Drops `.` components so that `./journal/a.md` and `journal/a.md` match the same folder rule
fn normalize(path: &Path) -> PathBuf {
    path.components()
        .filter(|c| !matches!(c, Component::CurDir))
        .collect()
}
