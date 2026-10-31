use std::{error::Error, fs, io::{ErrorKind, Write}, path::Path};

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
) -> Result<(), String> {
    let config = Config::from_path(&format!("{CONFIG_DIR}/config.toml"))?;
    let path = Path::new(path);

    let template = template.unwrap_or(config.default_template());
    let title = title.unwrap_or(config.default_title());

    let templates = Tera::new(&format!("{CONFIG_DIR}/templates/**/*"))
        .map_err(|e| format!("Failed to load templates: {}", error_chain(&e)))?;

    let context = build_context(path, title, vars)?;
    let content = templates.render(&format!("{}.md", template), &context)
        .map_err(|e| format!("Failed to render template `{}`: {}", template, error_chain(&e)))?;

    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
    }

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| match e.kind() {
            ErrorKind::AlreadyExists => format!("Note already exists: {}", path.display()),
            _ => format!("Failed to create {}: {}", path.display(), e),
        })?;

    file.write_all(content.as_bytes())
        .map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;

    println!("Created {}", path.display());

    Ok(())
}

fn build_context(path: &Path, title: &str, vars: &[(String, String)]) -> Result<Context, String> {
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
            return Err(format!("Can not override built-in variable `{}` with --var", key));
        }
        context.insert(key, value);
    }

    Ok(context)
}

fn error_chain(error: &dyn Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();

    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }

    message
}
