use std::{fs, sync::LazyLock};

use tera::{Context, Tera};

static TEMPLATES: LazyLock<Tera> = LazyLock::new(|| {
    match Tera::new(".zl/templates/**/*") {
        Ok(t) => t,
        Err(e) => {
            println!("Parsing error(s): {}", e);
            ::std::process::exit(1);
        }
    }
});

pub(crate) fn run(path: &str, template: &str) -> Result<(), String> {
    let mut context = Context::new();
    context.insert("title", "test");
    let expr = TEMPLATES.render(&format!("{}.md", template), &context)
        .map_err(|e| e.to_string())?;

    fs::write(path, expr)
        .map_err(|e| format!("Can not write file: {}", e))
}
