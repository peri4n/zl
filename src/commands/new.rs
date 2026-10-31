use std::{collections::BTreeMap, fs, io::{ErrorKind, Write}, path::Path};

use anyhow::{Context as _, bail};
use chrono::{Local, SecondsFormat};
use tera::{Context, Tera};

use crate::config::{Config, ConfigDir};

/// Variables provided to every template, which therefore can't be set with --var
const BUILTIN_VARIABLES: &[&str] = &["title", "id", "date", "time", "datetime", "filename", "extra"];

#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// Path of the new markdown file
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
}

pub(crate) fn run(args: &Args) -> anyhow::Result<()> {
    let config = Config::load()?;
    let path = Path::new(&args.path);

    let settings = config.note_settings(path, args.group.as_deref())?;
    let template = args.template.as_deref().unwrap_or(settings.template);
    let title = args.title.as_deref().unwrap_or(settings.title);

    let mut templates = Tera::new();
    templates.load_from_glob(&ConfigDir::current().templates().join("**/*").to_string_lossy())
        .context("Failed to load templates")?;

    let context = build_context(path, title, &settings.extra, &args.vars)?;
    let content = templates.render(&format!("{}.md", template), &context)
        .with_context(|| format!("Failed to render template `{}`", template))?;

    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }

    // `create_new` fails if the file exists, so an existing note is never overwritten
    let mut file = match fs::OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            bail!("Note already exists: {}", path.display())
        }
        Err(e) => return Err(e).with_context(|| format!("Failed to create {}", path.display())),
    };

    file.write_all(content.as_bytes())
        .with_context(|| format!("Failed to write {}", path.display()))?;

    match settings.group {
        Some(group) => println!("Created {} from template `{}` of group `{}`", path.display(), template, group),
        None => println!("Created {} from template `{}`", path.display(), template),
    }

    Ok(())
}

fn build_context(
    path: &Path,
    title: &str,
    extra: &BTreeMap<&str, &toml::Value>,
    vars: &[(String, String)],
) -> anyhow::Result<Context> {
    let now = Local::now();
    let filename = path.file_stem().map(|s| s.to_string_lossy()).unwrap_or_default();

    let mut context = Context::new();
    context.insert("title", title);
    context.insert("id", &now.format("%Y%m%d%H%M%S").to_string());
    context.insert("date", &now.format("%Y-%m-%d").to_string());
    context.insert("time", &now.format("%H:%M").to_string());
    context.insert("datetime", &now.to_rfc3339_opts(SecondsFormat::Secs, false));
    context.insert("filename", &filename);
    context.insert("extra", extra);

    for (key, value) in vars {
        if BUILTIN_VARIABLES.contains(&key.as_str()) {
            bail!("Can not override built-in variable `{}` with --var", key);
        }
        // Tera 2 needs keys convertible into `Cow<'static, str>`, so pass an owned one
        context.insert(key.clone(), value);
    }

    Ok(context)
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
