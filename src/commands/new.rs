use std::{
    fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context as _, anyhow, bail};
use chrono::{Local, SecondsFormat};
use tera::{Context, Kwargs, State, Tera};

use crate::{
    config::{Config, ConfigDir, NoteSettings},
    paths::normalize,
};

/// Variables provided to every template, which therefore can't be set with --var
const BUILTIN_VARIABLES: &[&str] = &[
    "title",
    "id",
    "random_id",
    "date",
    "time",
    "datetime",
    "filename",
    "extra",
];

#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// Path of the new note, or a folder to create it in, named by note.filename of the folder's
    /// group. Folders are existing directories and paths ending with `/`. Paths without extension
    /// get note.extension.
    #[arg(default_value = ".")]
    path: String,

    /// Template of the new markdown file [default: note.default_template of the note's group,
    /// otherwise of the root config]
    #[arg(short, long)]
    template: Option<String>,

    /// Title of the new note [default: note.default_title of the note's group, otherwise of the
    /// root config]
    #[arg(long)]
    title: Option<String>,

    /// Group whose settings to apply, instead of the group whose paths match the note's folder
    #[arg(short, long)]
    group: Option<String>,

    /// Additional template variable, can be repeated
    #[arg(long = "var", value_name = "KEY=VALUE", value_parser = parse_key_value)]
    vars: Vec<(String, String)>,

    /// Open the new note, or the existing one, in $VISUAL, otherwise $EDITOR, e.g. `nvim` or
    /// `code --wait`
    #[arg(short, long)]
    edit: bool,
}

pub(crate) fn run(args: &Args) -> anyhow::Result<()> {
    let config = Config::load()?;
    // Before creating the note, so it isn't created if it can't be opened
    let editor = args.edit.then(editor).transpose()?;

    let target = Path::new(&args.path);
    let is_folder = args.path.ends_with('/') || target.is_dir();
    let folder = match is_folder {
        true => target,
        false => target.parent().unwrap_or(Path::new("")),
    };

    let settings = config.note_settings(folder, args.group.as_deref())?;
    let template = args.template.as_deref().unwrap_or(settings.template);
    let title = args.title.as_deref().unwrap_or(settings.title);

    let mut templates = Tera::new();
    templates.register_filter("slug", |text: &str, _: Kwargs, _: &State| slug(text));
    templates.register_filter("date", tera_contrib::dates::date);
    templates
        .load_from_glob(
            &ConfigDir::current()
                .templates()
                .join("**/*")
                .to_string_lossy(),
        )
        .context("Failed to load templates")?;

    let mut context = build_context(title, &settings, &args.vars)?;
    let path = match is_folder {
        true => generate_path(&templates, &context, folder, &settings)?,
        false if target.extension().is_none() => normalize(&with_extension(
            &target.to_string_lossy(),
            settings.extension,
        )),
        false => normalize(target),
    };

    // Before rendering the template, so a broken template doesn't keep the note from being opened
    if path.is_file() {
        println!("Using existing note {}", path.display());
        if let Some(editor) = editor {
            open_in_editor(&editor, &path)?;
        }
        return Ok(());
    }

    let filename = path
        .file_stem()
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();
    context.insert("filename", &filename);

    let content = templates
        .render(&format!("{}.md", template), &context)
        .with_context(|| format!("Failed to render template `{}`", template))?;

    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }

    // `create_new` fails if the file exists, so an existing note is never overwritten
    let mut file = match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            bail!("Note already exists: {}", path.display())
        }
        Err(e) => return Err(e).with_context(|| format!("Failed to create {}", path.display())),
    };

    file.write_all(content.as_bytes())
        .with_context(|| format!("Failed to write {}", path.display()))?;

    match settings.group {
        Some(group) => println!(
            "Created {} from template `{}` of group `{}`",
            path.display(),
            template,
            group
        ),
        None => println!("Created {} from template `{}`", path.display(), template),
    }

    if let Some(editor) = editor {
        open_in_editor(&editor, &path)?;
    }

    Ok(())
}

/// The editor command from $VISUAL, otherwise $EDITOR: the program and its arguments
fn editor() -> anyhow::Result<Vec<String>> {
    let command = ["VISUAL", "EDITOR"]
        .into_iter()
        .filter_map(|variable| std::env::var(variable).ok())
        .find(|command| !command.trim().is_empty())
        .ok_or_else(|| anyhow!("Set $VISUAL or $EDITOR to open the note with --edit"))?;

    Ok(command.split_whitespace().map(str::to_string).collect())
}

/// Opens the note at `path` with `editor` and waits until it's closed. The path is absolute, as it's
/// relative to the notebook, which isn't the working directory of the user with `-C`.
fn open_in_editor(editor: &[String], path: &Path) -> anyhow::Result<()> {
    let path = std::env::current_dir()?.join(path);
    let (program, arguments) = editor.split_first().expect("editor commands aren't empty");

    let status = Command::new(program)
        .args(arguments)
        .arg(&path)
        .status()
        .with_context(|| format!("Failed to start editor `{}`", editor.join(" ")))?;
    if !status.success() {
        bail!("Editor `{}` failed: {}", editor.join(" "), status);
    }

    Ok(())
}

fn build_context(
    title: &str,
    settings: &NoteSettings,
    vars: &[(String, String)],
) -> anyhow::Result<Context> {
    let now = Local::now();

    let mut context = Context::new();
    context.insert("title", title);
    context.insert("id", &now.format("%Y%m%d%H%M%S").to_string());
    context.insert(
        "random_id",
        &random_id(settings.id_charset, settings.id_length)?,
    );
    context.insert("date", &now.format("%Y-%m-%d").to_string());
    context.insert("time", &now.format("%H:%M").to_string());
    context.insert("datetime", &now.to_rfc3339_opts(SecondsFormat::Secs, false));
    context.insert("extra", &settings.extra);

    for (key, value) in vars {
        if BUILTIN_VARIABLES.contains(&key.as_str()) {
            bail!("Can not override built-in variable `{}` with --var", key);
        }
        // Tera 2 needs keys convertible into `Cow<'static, str>`, so pass an owned one
        context.insert(key.clone(), value);
    }

    Ok(context)
}

/// Path of a new note in `folder` named by the rendered `settings.filename`
fn generate_path(
    templates: &Tera,
    context: &Context,
    folder: &Path,
    settings: &NoteSettings,
) -> anyhow::Result<PathBuf> {
    let name = templates
        .render_str(settings.filename, context, false)
        .with_context(|| format!("Failed to render note.filename `{}`", settings.filename))?;
    let name = name.trim();

    // The name may contain folders, but must stay in `folder`
    let relative = normalize(Path::new(name));
    if name.is_empty() || relative.has_root() || relative.starts_with("..") {
        bail!(
            "Invalid file name `{}` rendered from note.filename `{}`",
            name,
            settings.filename
        );
    }

    Ok(normalize(
        &folder.join(with_extension(name, settings.extension)),
    ))
}

/// `name` with `extension`, which may start with a dot
fn with_extension(name: &str, extension: &str) -> PathBuf {
    match extension.trim_start_matches('.') {
        "" => PathBuf::from(name),
        extension => PathBuf::from(format!("{name}.{extension}")),
    }
}

/// `length` random characters of `charset`: alphanum, hex, letters, numbers, or the characters
/// themselves
fn random_id(charset: &str, length: usize) -> anyhow::Result<String> {
    let chars: Vec<char> = match charset {
        "alphanum" => ('a'..='z').chain('0'..='9').collect(),
        "hex" => ('0'..='9').chain('a'..='f').collect(),
        "letters" => ('a'..='z').collect(),
        "numbers" => ('0'..='9').collect(),
        chars => chars.chars().collect(),
    };
    if chars.is_empty() {
        bail!("note.id_charset is empty");
    }

    Ok((0..length)
        .map(|_| chars[fastrand::usize(..chars.len())])
        .collect())
}

/// `text` for file names and URLs: lowercase letters and digits, everything else replaced by
/// single `-`, e.g. `retrieval-practice` for `Retrieval Practice!`
fn slug(text: &str) -> String {
    let mut slug = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }

    slug.trim_end_matches('-').to_string()
}

fn parse_key_value(s: &str) -> Result<(String, String), String> {
    let (key, value) = s
        .split_once('=')
        .ok_or_else(|| format!("Invalid KEY=VALUE: no `=` found in `{}`", s))?;

    if key.is_empty() {
        return Err(format!("Invalid KEY=VALUE: empty key in `{}`", s));
    }

    Ok((key.to_string(), value.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugifies_text() {
        assert_eq!(slug("Retrieval Practice!"), "retrieval-practice");
        assert_eq!(slug("  Grüße, Welt -- 2026 "), "grüße-welt-2026");
        assert_eq!(slug("?!"), "");
    }

    #[test]
    fn generates_random_ids() {
        let id = random_id("hex", 8).unwrap();
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()), "{id}");
        assert_eq!(random_id("x", 3).unwrap(), "xxx");
        assert!(random_id("", 3).is_err());
    }
}
