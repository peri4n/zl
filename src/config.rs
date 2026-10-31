use std::{collections::BTreeMap, fs, path::{Component, Path, PathBuf}};

use anyhow::{Context, anyhow, bail};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};

/// Name of the directory holding the zettelkasten configuration and templates
pub(crate) const CONFIG_DIR: &str = ".zl";

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
    pub(crate) extra: BTreeMap<&'a str, &'a toml::Value>,
}

impl Config {
    pub(crate) fn from_path(path: &str) -> anyhow::Result<Self> {
        if !Path::new(path).exists() {
            bail!("Config file not found at path: {}", path);
        }

        let config_str = fs::read_to_string(path).context("Failed to read config file")?;
        let mut config: Config = toml::from_str(&config_str).context("Failed to parse config file")?;

        for (name, group) in &mut config.group {
            group.matcher = build_matcher(&group.paths)
                .with_context(|| format!("Invalid paths in group `{}`", name))?;
        }

        Ok(config)
    }

    /// Like `from_path`, but falls back to the default config if the file doesn't exist,
    /// for commands that also work outside of an initialized zettelkasten
    pub(crate) fn from_path_or_default(path: &str) -> anyhow::Result<Self> {
        if Path::new(path).exists() {
            Self::from_path(path)
        } else {
            Ok(Self::default())
        }
    }

    /// Settings for a new note at `note_path`, taken from the group named `group`, otherwise from the
    /// group whose paths match the note's folder. Settings the group doesn't set come from the root config.
    pub(crate) fn note_settings(&self, note_path: &Path, group: Option<&str>) -> anyhow::Result<NoteSettings<'_>> {
        let group = match group {
            Some(name) => Some(self.group.get_key_value(name).ok_or_else(|| {
                anyhow!("Unknown group `{}`, configured groups: {}", name, self.group_names().join(", "))
            })?),
            None => self.matching_group(note_path)?,
        };

        let mut settings = NoteSettings {
            group: None,
            template: &self.note.default_template,
            title: &self.note.default_title,
            extra: self.extra.iter().map(|(key, value)| (key.as_str(), value)).collect(),
        };

        if let Some((name, group)) = group {
            settings.group = Some(name);
            settings.template = group.note.default_template.as_deref().unwrap_or(settings.template);
            settings.title = group.note.default_title.as_deref().unwrap_or(settings.title);
            settings.extra.extend(group.extra.iter().map(|(key, value)| (key.as_str(), value)));
        }

        Ok(settings)
    }

    pub(crate) fn list_format(&self) -> &str {
        &self.list.format
    }

    fn matching_group(&self, note_path: &Path) -> anyhow::Result<Option<(&String, &GroupConfig)>> {
        let note_path = normalize(note_path);
        let folder = note_path.parent().unwrap_or(Path::new(""));

        let matches: Vec<_> = self.group.iter()
            .filter(|(_, group)| group.matcher.is_match(folder))
            .collect();

        match matches.as_slice() {
            [] => Ok(None),
            [group] => Ok(Some(*group)),
            _ => {
                let names: Vec<_> = matches.iter().map(|(name, _)| name.as_str()).collect();
                bail!("{} matches the paths of several groups ({}), choose one with --group",
                    note_path.display(), names.join(", "))
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

/// Compiles the path globs of a group. `*` doesn't cross folders, `**` does, and `<folder>/**`
/// also matches `<folder>` itself, so a single pattern covers a folder and everything below it.
fn build_matcher(paths: &[String]) -> anyhow::Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();

    for path in paths {
        let pattern = normalize(Path::new(path.trim_end_matches('/'))).to_string_lossy().into_owned();
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

/// Drops `.` components so that `./journal/a.md` and `journal/a.md` are matched the same way
fn normalize(path: &Path) -> PathBuf {
    path.components()
        .filter(|c| !matches!(c, Component::CurDir))
        .collect()
}
