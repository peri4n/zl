use std::{fs, path::Path};

use clap::ValueEnum;
use markdown::{mdast::{ListItem, Node}, to_mdast};

use crate::notes;

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

struct Task {
    line: usize,
    checked: bool,
    text: String,
}

pub(crate) fn run(path: &str, status: TaskStatus) -> Result<(), String> {
    let parse_options = notes::parse_options();

    for file in notes::collect(Path::new(path))? {
        let input = fs::read_to_string(&file)
            .map_err(|e| format!("Failed to read {}: {}", file.display(), e))?;
        let ast = to_mdast(&input, &parse_options)
            .map_err(|e| format!("Failed to parse {}: {}", file.display(), e))?;

        let mut tasks = Vec::new();
        collect_tasks(&ast, &input, &mut tasks);

        for task in tasks.iter().filter(|t| status.matches(t.checked)) {
            let marker = if task.checked { "[x]" } else { "[ ]" };
            println!("{}:{}  {} {}", file.display(), task.line, marker, task.text);
        }
    }

    Ok(())
}

/// Walks the syntax tree and collects all task list items, including nested ones
fn collect_tasks(node: &Node, input: &str, tasks: &mut Vec<Task>) {
    if let Node::ListItem(item) = node
        && let Some(checked) = item.checked
    {
        tasks.push(Task {
            line: item.position.as_ref().map_or(0, |p| p.start.line),
            checked,
            text: task_text(item, input),
        });
    }

    for child in node.children().into_iter().flatten() {
        collect_tasks(child, input, tasks);
    }
}

/// Raw markdown of the task's first paragraph, so links and inline code stay intact
fn task_text(item: &ListItem, input: &str) -> String {
    let Some(position) = item.children.first().and_then(Node::position) else {
        return String::new();
    };

    input[position.start.offset..position.end.offset]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
