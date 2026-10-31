use std::{fs, path::Path};

use markdown::{ParseOptions, to_mdast};

use crate::notes;

pub(crate) fn run(path: &str) -> Result<(), String> {
    let files = notes::collect(Path::new(path))?;
    let parse_options = notes::parse_options();

    let mut failed = 0;
    for file in &files {
        match validate_file(file, &parse_options) {
            Ok(()) => println!("ok    {}", file.display()),
            Err(reason) => {
                failed += 1;
                println!("FAIL  {}: {}", file.display(), reason);
            }
        }
    }

    println!("\n{} file(s) checked, {} failed", files.len(), failed);

    if failed > 0 {
        return Err(format!("{} file(s) failed validation", failed));
    }

    Ok(())
}

fn validate_file(path: &Path, parse_options: &ParseOptions) -> Result<(), String> {
    let input = fs::read_to_string(path).map_err(|e| e.to_string())?;
    to_mdast(&input, parse_options).map_err(|e| e.to_string())?;

    Ok(())
}
