use std::path::Path;

use clap::ValueEnum;

use crate::index::Index;

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskStatus {
    /// Tasks not done yet: `- [ ]`
    Open,
    /// Completed tasks: `- [x]`
    Done,
    /// Open and completed tasks
    All,
}

impl TaskStatus {
    fn matches(self, checked: bool) -> bool {
        match self {
            TaskStatus::Open => !checked,
            TaskStatus::Done => checked,
            TaskStatus::All => true,
        }
    }
}

#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// Markdown file or directory to search
    #[arg(default_value = ".")]
    path: String,

    /// Only show tasks with this status
    #[arg(short, long, value_enum, default_value_t = TaskStatus::All)]
    status: TaskStatus,
}

pub(crate) fn run(args: &Args) -> anyhow::Result<()> {
    let index = Index::open()?;
    index.warn_skipped();

    for note in index.notes(Path::new(&args.path))? {
        for task in note.tasks.iter().filter(|task| args.status.matches(task.checked)) {
            let marker = if task.checked { "[x]" } else { "[ ]" };
            println!("{}:{}  {} {}", note.path.display(), task.line, marker, task.text);
        }
    }

    Ok(())
}
