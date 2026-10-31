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

pub(crate) fn run(path: &str, status: TaskStatus) -> anyhow::Result<()> {
    let index = Index::open()?;
    index.warn_skipped();

    for note in index.notes(Path::new(path))? {
        for task in note.tasks.iter().filter(|task| status.matches(task.checked)) {
            let marker = if task.checked { "[x]" } else { "[ ]" };
            println!("{}:{}  {} {}", note.path.display(), task.line, marker, task.text);
        }
    }

    Ok(())
}
