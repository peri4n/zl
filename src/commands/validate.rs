use std::path::Path;

use anyhow::bail;

use crate::notes::{self, Note};

pub(crate) fn run(path: &str) -> anyhow::Result<()> {
    let files = notes::collect(Path::new(path))?;
    let parse_options = notes::parse_options();

    let mut failed = 0;
    for file in &files {
        // Loading a note parses its markdown and frontmatter, so it fails on everything we check
        match Note::load(file, &parse_options) {
            Ok(_) => println!("ok    {}", file.display()),
            Err(reason) => {
                failed += 1;
                println!("FAIL  {}: {:#}", file.display(), reason);
            }
        }
    }

    println!("\n{} file(s) checked, {} failed", files.len(), failed);

    if failed > 0 {
        bail!("{} file(s) failed validation", failed);
    }

    Ok(())
}
