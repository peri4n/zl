use std::{collections::BTreeMap, path::Path};

use anyhow::bail;

use crate::index::Index;

#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// Markdown file or directory to validate
    #[arg(default_value = ".")]
    path: String,

    /// Only print problems, without the files that passed and the summary
    #[arg(short, long)]
    quiet: bool,
}

/// Checks the notes at or below `path`. Updating the index parses every changed note, so notes that
/// fail to parse and links to missing notes are known afterwards without parsing anything again.
pub(crate) fn run(args: &Args) -> anyhow::Result<()> {
    let path = Path::new(&args.path);
    let index = Index::open()?;

    // Problems of every checked note, an empty list means the note is fine
    let mut problems: BTreeMap<_, Vec<String>> = BTreeMap::new();
    for note in index.notes(path)? {
        problems.entry(note.path).or_default();
    }
    for (path, error) in index.skipped(path)? {
        problems
            .entry(path.clone())
            .or_default()
            .push(format!("{}: {:#}", path.display(), error));
    }
    for link in index.broken_links(path)? {
        let problem = format!(
            "{}:{}: broken link `{}`",
            link.path.display(),
            link.line,
            link.url
        );
        problems.entry(link.path).or_default().push(problem);
    }

    let mut failed = 0;
    for (path, problems) in &problems {
        if problems.is_empty() {
            if !args.quiet {
                println!("ok    {}", path.display());
            }
        } else {
            failed += 1;
            for problem in problems {
                println!("FAIL  {}", problem);
            }
        }
    }

    if !args.quiet {
        println!("\n{} file(s) checked, {} failed", problems.len(), failed);
    }

    if failed > 0 {
        bail!("{} file(s) failed validation", failed);
    }

    Ok(())
}
