use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, anyhow, bail};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};

use crate::paths::normalize;

/// The `.zl` directory of a notebook, holding its configuration, templates and index
pub(crate) struct ConfigDir(PathBuf);

impl ConfigDir {
    const INDEX_FILE: &str = "index.db";

    /// `.zl` of the current notebook, the working directory
    pub(crate) fn current() -> Self {
        Self::of(Path::new(""))
    }

    /// `.zl` of the notebook at `root`
    pub(crate) fn of(root: &Path) -> Self {
        ConfigDir(root.join(".zl"))
    }

    pub(crate) fn exists(&self) -> bool {
        self.0.is_dir()
    }

    pub(crate) fn config_file(&self) -> PathBuf {
        self.0.join("config.toml")
    }

    pub(crate) fn templates(&self) -> PathBuf {
        self.0.join("templates")
    }

    pub(crate) fn index_file(&self) -> PathBuf {
        self.0.join(Self::INDEX_FILE)
    }

    pub(crate) fn gitignore(&self) -> PathBuf {
        self.0.join(".gitignore")
    }

    /// Content of `.gitignore`: config and templates are meant to be committed, the index is a cache
    pub(crate) fn gitignore_content() -> String {
        format!("{}*\n", Self::INDEX_FILE)
    }
}

/// Missing sections and fields fall back to their defaults, unknown ones are rejected to catch typos
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Config {
    /// Settings for new notes that don't belong to a group
    note: NoteConfig,

    /// Template variables for new notes, available as `extra.<key>`
    extra: BTreeMap<String, toml::Value>,

    /// Groups overriding `note` and `extra` for new notes in certain folders
    group: BTreeMap<String, GroupConfig>,

    list: FormatConfig,
}

/// Settings for a new note after applying its group, if any, on top of the root config
pub(crate) struct NoteSettings<'a> {
    pub(crate) group: Option<&'a str>,
    pub(crate) template: &'a str,
    pub(crate) title: &'a str,
    pub(crate) filename: &'a str,
    pub(crate) extension: &'a str,
    pub(crate) id_charset: &'a str,
    pub(crate) id_length: usize,
    pub(crate) extra: BTreeMap<&'a str, &'a toml::Value>,
}

impl Config {
    /// Loads the config of the current notebook
    pub(crate) fn load() -> anyhow::Result<Self> {
        let path = ConfigDir::current().config_file();
        if !path.exists() {
            bail!(
                "{} is not a zl notebook, {} is missing. Run `zl init` to create it.",
                notebook_dir().display(),
                path.display()
            );
        }

        let config_str = fs::read_to_string(&path).context("Failed to read config file")?;
        let mut config: Config =
            toml::from_str(&config_str).context("Failed to parse config file")?;

        for (name, group) in &mut config.group {
            group.matcher = build_matcher(&group.paths)
                .with_context(|| format!("Invalid paths in group `{}`", name))?;
        }

        Ok(config)
    }

    /// Like `load`, but falls back to the default config if the notebook has none,
    /// for commands that also work outside of an initialized zettelkasten
    pub(crate) fn load_or_default() -> anyhow::Result<Self> {
        if ConfigDir::current().config_file().exists() {
            Self::load()
        } else {
            Ok(Self::default())
        }
    }

    /// Settings for a new note in `folder`, taken from the group named `group`, otherwise from the
    /// group whose paths match the folder. Settings the group doesn't set come from the root config.
    pub(crate) fn note_settings(
        &self,
        folder: &Path,
        group: Option<&str>,
    ) -> anyhow::Result<NoteSettings<'_>> {
        let group = match group {
            Some(name) => Some(self.group.get_key_value(name).ok_or_else(|| {
                anyhow!(
                    "Unknown group `{}`, configured groups: {}",
                    name,
                    self.group_names().join(", ")
                )
            })?),
            None => self.matching_group(folder)?,
        };

        let mut settings = NoteSettings {
            group: None,
            template: &self.note.default_template,
            title: &self.note.default_title,
            filename: &self.note.filename,
            extension: &self.note.extension,
            id_charset: &self.note.id_charset,
            id_length: self.note.id_length,
            extra: self
                .extra
                .iter()
                .map(|(key, value)| (key.as_str(), value))
                .collect(),
        };

        if let Some((name, group)) = group {
            settings.group = Some(name);
            settings.template = group
                .note
                .default_template
                .as_deref()
                .unwrap_or(settings.template);
            settings.title = group
                .note
                .default_title
                .as_deref()
                .unwrap_or(settings.title);
            settings.filename = group.note.filename.as_deref().unwrap_or(settings.filename);
            settings.extension = group
                .note
                .extension
                .as_deref()
                .unwrap_or(settings.extension);
            settings.id_charset = group
                .note
                .id_charset
                .as_deref()
                .unwrap_or(settings.id_charset);
            settings.id_length = group.note.id_length.unwrap_or(settings.id_length);
            settings
                .extra
                .extend(group.extra.iter().map(|(key, value)| (key.as_str(), value)));
        }

        Ok(settings)
    }

    pub(crate) fn list_format(&self) -> &str {
        &self.list.format
    }

    fn matching_group(&self, folder: &Path) -> anyhow::Result<Option<(&String, &GroupConfig)>> {
        let folder = normalize(folder);

        let matches: Vec<_> = self
            .group
            .iter()
            .filter(|(_, group)| group.matcher.is_match(&folder))
            .collect();

        match matches.as_slice() {
            [] => Ok(None),
            [group] => Ok(Some(*group)),
            _ => {
                let names: Vec<_> = matches.iter().map(|(name, _)| name.as_str()).collect();
                bail!(
                    "Folder `{}` matches the paths of several groups ({}), choose one with --group",
                    folder.display(),
                    names.join(", ")
                )
            }
        }
    }

    fn group_names(&self) -> Vec<&str> {
        self.group.keys().map(String::as_str).collect()
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct NoteConfig {
    default_template: String,

    default_title: String,

    /// File name of new notes without extension, a Tera template with the variables of the note
    /// templates and `random_id`. `/` creates folders, e.g. "{{ date }}/{{ title | slug }}".
    filename: String,

    /// Extension of new notes
    extension: String,

    /// Characters of `random_id`: alphanum, hex, letters, numbers, or the characters themselves
    id_charset: String,

    /// Number of characters of `random_id`
    id_length: usize,
}

impl Default for NoteConfig {
    fn default() -> Self {
        NoteConfig {
            default_template: "default".to_string(),
            default_title: "Untitled".to_string(),
            filename: "{{ id }}".to_string(),
            extension: "md".to_string(),
            id_charset: "alphanum".to_string(),
            id_length: 4,
        }
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct GroupConfig {
    /// Globs matched against the folder of a new note, relative to the zettelkasten root
    paths: Vec<String>,

    note: GroupNoteConfig,

    extra: BTreeMap<String, toml::Value>,

    /// Compiled `paths`, built when the config is loaded
    #[serde(skip)]
    matcher: GlobSet,
}

/// Same as `NoteConfig`, but every setting is optional and falls back to the root config
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct GroupNoteConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    default_template: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    default_title: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    filename: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    extension: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    id_charset: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    id_length: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct FormatConfig {
    /// Output format of `zl list` when no --format is given
    format: String,
}

impl Default for FormatConfig {
    fn default() -> Self {
        FormatConfig {
            format: "{path}  {title}  {tags}".to_string(),
        }
    }
}

/// Absolute path of the notebook, i.e. the working directory after `-C`, for messages to the user
pub(crate) fn notebook_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Compiles the path globs of a group. `*` doesn't cross folders, `**` does, and `<folder>/**`
/// also matches `<folder>` itself, so a single pattern covers a folder and everything below it.
fn build_matcher(paths: &[String]) -> anyhow::Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();

    for path in paths {
        let pattern = normalize(Path::new(path.trim_end_matches('/')))
            .to_string_lossy()
            .into_owned();
        let mut patterns = vec![pattern.as_str()];
        if let Some(folder) = pattern.strip_suffix("/**") {
            patterns.push(folder);
        }

        for pattern in patterns {
            let glob = GlobBuilder::new(pattern)
                .literal_separator(true)
                .build()
                .with_context(|| format!("Invalid path pattern `{}`", path))?;
            builder.add(glob);
        }
    }

    Ok(builder.build()?)
}
