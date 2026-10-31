use std::{collections::BTreeMap, path::{Component, Path, PathBuf}};

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
    pub(crate) fn from_path(path: &str) -> Result<Self, String> {
        if std::path::Path::new(path).exists() {
            let config_str = std::fs::read_to_string(path)
                .map_err(|e| format!("Failed to read config file: {}", e))?;

            toml::from_str(&config_str)
                .map_err(|e| format!("Failed to parse config file: {}", e))
        } else {
            Err(format!("Config file not found at path: {}.", path))
        }
    }

    /// Template for a new note at `note_path`: the rule of the most specific folder
    /// containing the note, or `note.default_template` if no rule matches
    pub(crate) fn template_for(&self, note_path: &Path) -> &str {
        let note_path = normalize(note_path);

        self.template.folders
            .iter()
            .map(|(folder, template)| (normalize(Path::new(folder)), template))
            .filter(|(folder, _)| note_path.parent().is_some_and(|parent| parent.starts_with(folder)))
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
    /// Folder (relative to the zettelkasten root) mapped to the template applied to new notes in it
    folders: BTreeMap<String, String>,
}

/// Drops `.` components so that `./journal/a.md` and `journal/a.md` match the same folder rule
fn normalize(path: &Path) -> PathBuf {
    path.components()
        .filter(|c| !matches!(c, Component::CurDir))
        .collect()
}
