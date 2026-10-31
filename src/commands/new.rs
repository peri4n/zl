use std::{fs, io::{ErrorKind, Write}, path::Path};

use anyhow::{Context as _, bail};
use chrono::{Local, SecondsFormat};
use tera::{Context, Tera};

use crate::config::{CONFIG_DIR, Config};

/// Variables provided to every template, which therefore can't be set with --var
const BUILTIN_VARIABLES: &[&str] = &["title", "id", "date", "time", "datetime", "filename"];

pub(crate) fn run(
    path: &str,
    template: Option<&str>,
    title: Option<&str>,
    vars: &[(String, String)],
) -> anyhow::Result<()> {
    let config = Config::from_path(&format!("{CONFIG_DIR}/config.toml"))?;
    let path = Path::new(path);

    let template = template.unwrap_or_else(|| config.template_for(path));
    let title = title.unwrap_or(config.default_title());

    let templates = Tera::new(&format!("{CONFIG_DIR}/templates/**/*"))
        .context("Failed to load templates")?;

    let context = build_context(path, title, vars)?;
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

    println!("Created {} from template `{}`", path.display(), template);

    Ok(())
}

fn build_context(path: &Path, title: &str, vars: &[(String, String)]) -> anyhow::Result<Context> {
    let now = Local::now();
    let filename = path.file_stem().map(|s| s.to_string_lossy()).unwrap_or_default();

    let mut context = Context::new();
    context.insert("title", title);
    context.insert("id", &now.format("%Y%m%d%H%M%S").to_string());
    context.insert("date", &now.format("%Y-%m-%d").to_string());
    context.insert("time", &now.format("%H:%M").to_string());
    context.insert("datetime", &now.to_rfc3339_opts(SecondsFormat::Secs, false));
    context.insert("filename", &filename);

    for (key, value) in vars {
        if BUILTIN_VARIABLES.contains(&key.as_str()) {
            bail!("Can not override built-in variable `{}` with --var", key);
        }
        context.insert(key, value);
    }

    Ok(context)
}
