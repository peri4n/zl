//! Runs the `zl` binary against small notebooks in temporary directories

use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use tempfile::TempDir;

/// An initialized notebook with a few linked notes:
///
/// - `hub.md` links to `projects/zl.md`, `rust.md` and an external URL
/// - `rust.md` links back to `hub.md` and has two tasks
/// - `projects/zl.md` links to `hub.md`
/// - `lonely.md` has no links and nothing links to it
struct Notebook {
    dir: TempDir,
}

impl Notebook {
    fn new() -> Self {
        let notebook = Self::empty();
        notebook.ok(&["init"]);

        notebook.write(
            "hub.md",
            "# Hub\n\n[zl](projects/zl.md) [rust](rust.md#intro) [web](https://example.com)\n",
        );
        notebook.write(
            "rust.md",
            "---\ntitle: Rust errors\ntags: [rust, Errors]\n---\n- [ ] open task\n- [x] done task\n\nSee [hub](./hub.md).\n",
        );
        notebook.write(
            "projects/zl.md",
            "+++\ntags = [\"rust\", \"project\"]\n+++\n# zl Project\n\n[up](../hub.md)\n",
        );
        notebook.write("lonely.md", "# Lonely\n\nNo links here.\n");

        notebook
    }

    /// A directory without `.zl`
    fn empty() -> Self {
        Notebook {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn write(&self, path: &str, content: &str) {
        let path = self.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn read(&self, path: &str) -> String {
        fs::read_to_string(self.path().join(path)).unwrap()
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    /// zl with `args`, run in the notebook
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_zl"));
        command
            .args(args)
            .current_dir(self.path())
            .env_remove("ZL_NOTEBOOK_DIR");
        command
    }

    /// Runs zl, expecting success, and returns its stdout
    fn ok(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "zl {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    /// Runs zl, expecting failure, and returns its stdout and stderr
    fn fails(&self, args: &[&str]) -> (String, String) {
        let output = self.run(args);
        assert!(
            !output.status.success(),
            "zl {:?} unexpectedly succeeded",
            args
        );
        (
            String::from_utf8(output.stdout).unwrap(),
            String::from_utf8(output.stderr).unwrap(),
        )
    }
}

#[test]
fn init_creates_config_and_never_overwrites_it() {
    let notebook = Notebook::empty();
    notebook.ok(&["init"]);

    assert!(notebook.path().join(".zl/templates").is_dir());
    assert_eq!(notebook.read(".zl/.gitignore"), "index.db*\n");
    assert!(
        notebook
            .read(".zl/config.toml")
            .contains("default_template = \"default\"")
    );

    notebook.write(".zl/config.toml", "# custom\n");
    let (_, stderr) = notebook.fails(&["init"]);
    assert!(stderr.contains("already initialized"), "{stderr}");
    assert_eq!(notebook.read(".zl/config.toml"), "# custom\n");
}

#[test]
fn config_shows_defaults_outside_of_notebooks() {
    let notebook = Notebook::empty();

    let output = notebook.run(&["config"]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("default_template = \"default\""));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("has no .zl/config.toml, showing the defaults"),
        "{stderr}"
    );
    assert!(stderr.contains("zl init"), "{stderr}");
}

#[test]
fn new_requires_a_notebook() {
    let notebook = Notebook::empty();

    let (_, stderr) = notebook.fails(&["new", "a.md"]);
    assert!(stderr.contains("is not a zl notebook"), "{stderr}");
    assert!(stderr.contains("zl init"), "{stderr}");
    assert!(!notebook.path().join("a.md").exists());
}

#[test]
fn new_renders_template_with_groups() {
    let notebook = Notebook::empty();
    notebook.ok(&["init"]);
    notebook.write(
        ".zl/config.toml",
        "[extra]\nauthor = \"me\"\n\n[group.journal]\npaths = [\"journal/**\"]\n\n[group.journal.note]\ndefault_template = \"daily\"\n",
    );
    notebook.write(
        ".zl/templates/default.md",
        "default {{ title }} {{ filename }} {{ extra.author }} {{ tags }} {{ \"2026-10-31\" | date(format=\"%A\") }}\n",
    );
    notebook.write(".zl/templates/daily.md", "daily {{ title }}\n");

    let stdout = notebook.ok(&["new", "a.md", "--title", "A", "--var", "tags=x"]);
    assert_eq!(stdout, "Created a.md from template `default`\n");
    assert_eq!(notebook.read("a.md"), "default A a me x Saturday\n");

    let stdout = notebook.ok(&["new", "journal/2026/today.md"]);
    assert_eq!(
        stdout,
        "Created journal/2026/today.md from template `daily` of group `journal`\n"
    );
    assert_eq!(notebook.read("journal/2026/today.md"), "daily Untitled\n");

    let (_, stderr) = notebook.fails(&["new", "a.md", "--var", "tags=x"]);
    assert!(stderr.contains("Note already exists"), "{stderr}");
    let (_, stderr) = notebook.fails(&["new", "b.md", "--var", "title=x"]);
    assert!(stderr.contains("built-in variable `title`"), "{stderr}");
}

#[test]
fn new_names_notes_by_the_scheme_of_their_group() {
    let notebook = Notebook::empty();
    notebook.ok(&["init"]);
    notebook.write(
        ".zl/config.toml",
        r#"
[note]
filename = "{{ title | slug }}"

[group.journal]
paths = ["journal/**"]

[group.journal.note]
filename = "{{ date }}"
default_template = "daily"

[group.inbox]
paths = ["inbox"]

[group.inbox.note]
filename = "{{ random_id }}"
id_charset = "hex"
id_length = 6
extension = "markdown"

[group.escaping]
paths = []

[group.escaping.note]
filename = "../{{ id }}"
"#,
    );
    notebook.write(
        ".zl/templates/default.md",
        "# {{ title }} ({{ filename }})\n",
    );
    notebook.write(".zl/templates/daily.md", "daily {{ filename }}\n");

    // The notebook root by default, numbered if the name is taken
    let title = ["new", "--title", "Retrieval Practice!"];
    assert_eq!(
        notebook.ok(&title),
        "Created retrieval-practice.md from template `default`\n"
    );
    assert_eq!(
        notebook.read("retrieval-practice.md"),
        "# Retrieval Practice! (retrieval-practice)\n"
    );
    assert_eq!(
        notebook.ok(&title),
        "Created retrieval-practice-2.md from template `default`\n"
    );

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    assert_eq!(
        notebook.ok(&["new", "journal/"]),
        format!("Created journal/{today}.md from template `daily` of group `journal`\n")
    );
    assert_eq!(
        notebook.read(&format!("journal/{today}.md")),
        format!("daily {today}\n")
    );

    // Existing folders don't need a trailing `/`
    fs::create_dir(notebook.path().join("inbox")).unwrap();
    let stdout = notebook.ok(&["new", "inbox"]);
    let name = stdout
        .strip_prefix("Created inbox/")
        .and_then(|rest| rest.split_once(".markdown from template `default` of group `inbox`\n"))
        .map(|(id, _)| id)
        .unwrap_or_else(|| panic!("{stdout}"));
    assert!(
        name.len() == 6 && name.chars().all(|c| c.is_ascii_hexdigit()),
        "{name}"
    );

    // Paths of notes keep their name and get the extension if they have none
    assert_eq!(
        notebook.ok(&["new", "ideas/Some Idea"]),
        "Created ideas/Some Idea.md from template `default`\n"
    );

    let (_, stderr) = notebook.fails(&["new", "--group", "escaping"]);
    assert!(stderr.contains("Invalid file name `../"), "{stderr}");
}

#[test]
fn new_opens_notes_in_the_editor() {
    let notebook = Notebook::empty();
    notebook.ok(&["init"]);
    notebook.write(".zl/templates/default.md", "# {{ title }}\n");
    let root = notebook.path().canonicalize().unwrap();
    let outside = tempfile::tempdir().unwrap();

    // $VISUAL wins over $EDITOR, its arguments are kept, the path is absolute even with -C
    let output = notebook
        .command(&["-C", root.to_str().unwrap(), "new", "dir/a.md", "--edit"])
        .current_dir(outside.path())
        .env("VISUAL", "echo opened")
        .env("EDITOR", "false")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!(
            "Created dir/a.md from template `default`\nopened {}\n",
            root.join("dir/a.md").display()
        )
    );

    let output = notebook
        .command(&["new", "b.md", "-e"])
        .env_remove("VISUAL")
        .env("EDITOR", "false")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stderr).contains("Editor `false` failed"));
    assert!(notebook.path().join("b.md").exists());

    // Without editor the note isn't created
    let output = notebook
        .command(&["new", "c.md", "-e"])
        .env_remove("VISUAL")
        .env("EDITOR", " ")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Set $VISUAL or $EDITOR"));
    assert!(!notebook.path().join("c.md").exists());
}

#[test]
fn list_shows_all_notes() {
    let notebook = Notebook::new();

    assert_eq!(
        notebook.ok(&["list"]),
        "hub.md  Hub\nlonely.md  Lonely\nprojects/zl.md  zl Project  rust, project\nrust.md  Rust errors  rust, Errors\n"
    );
    assert_eq!(
        notebook.ok(&["list", "projects"]),
        "projects/zl.md  zl Project  rust, project\n"
    );
}

#[test]
fn list_filters_by_tags_and_title() {
    let notebook = Notebook::new();

    assert_eq!(
        notebook.ok(&["list", "-t", "RUST", "--format", "{path}"]),
        "projects/zl.md\nrust.md\n"
    );
    assert_eq!(
        notebook.ok(&["list", "-t", "rust", "-t", "#errors", "--format", "{path}"]),
        "rust.md\n"
    );
    assert_eq!(
        notebook.ok(&["list", "--title", "^(Hub|Lonely)$", "--format", "{path}"]),
        "hub.md\nlonely.md\n"
    );
}

#[test]
fn list_filters_by_links() {
    let notebook = Notebook::new();

    assert_eq!(
        notebook.ok(&["list", "--no-links", "--format", "{path}"]),
        "lonely.md\n"
    );
    assert_eq!(
        notebook.ok(&["list", "--links-to", "hub.md", "--format", "{path}"]),
        "projects/zl.md\nrust.md\n"
    );
    assert_eq!(
        notebook.ok(&[
            "list",
            "--links-to",
            "./projects/zl.md",
            "--format",
            "{path}"
        ]),
        "hub.md\n"
    );
    assert_eq!(
        notebook.ok(&["list", "--orphan", "--format", "{path}"]),
        "lonely.md\n"
    );
}

#[test]
fn list_formats_notes() {
    let notebook = Notebook::new();

    assert_eq!(
        notebook.ok(&[
            "list",
            "rust.md",
            "--format",
            "- {mdlink}\\t{filename}|{dir}|{{{tags}}}"
        ]),
        "- [Rust errors](rust.md)\trust||{rust, Errors}\n"
    );
    let (_, stderr) = notebook.fails(&["list", "--format", "{titel}"]);
    assert!(stderr.contains("unknown placeholder `{titel}`"), "{stderr}");
}

#[test]
fn list_follows_changes_to_notes() {
    let notebook = Notebook::new();
    notebook.ok(&["list"]);

    notebook.write("lonely.md", "# Not lonely\n\n[hub](hub.md)\n");
    fs::remove_file(notebook.path().join("projects/zl.md")).unwrap();

    assert_eq!(
        notebook.ok(&["list", "--links-to", "hub.md", "--format", "{path} {title}"]),
        "lonely.md Not lonely\nrust.md Rust errors\n"
    );
}

#[test]
fn list_works_outside_of_notebooks() {
    let notebook = Notebook::empty();
    notebook.write("a.md", "# A\n\n[b](b.md)\n");
    notebook.write("b.md", "# B\n");

    assert_eq!(notebook.ok(&["list", "--links-to", "b.md"]), "a.md  A\n");
    assert!(!notebook.path().join(".zl").exists());
}

#[test]
fn list_skips_broken_notes_with_a_warning() {
    let notebook = Notebook::new();
    notebook.write("broken.md", "---\ntags: {a: 1}\n---\n");

    let output = notebook.run(&["list", "--format", "{path}"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "hub.md\nlonely.md\nprojects/zl.md\nrust.md\n"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("warning: skipping broken.md"));
}

#[test]
fn tags_counts_notes_per_tag() {
    let notebook = Notebook::new();

    assert_eq!(notebook.ok(&["tags"]), "1  Errors\n1  project\n2  rust\n");
    assert_eq!(
        notebook.ok(&["tags", "--sort", "count"]),
        "2  rust\n1  Errors\n1  project\n"
    );
}

/// Notes with nested tags: `a.md` [lang/rust, lang/go], `b.md` [lang], `c.md` [lang/rust/async, foo/]
fn nested_tags_notebook() -> Notebook {
    let notebook = Notebook::empty();
    notebook.ok(&["init"]);
    notebook.write("a.md", "---\ntags: [lang/rust, lang/go]\n---\n");
    notebook.write("b.md", "---\ntags: [lang]\n---\n");
    notebook.write("c.md", "---\ntags: [lang/rust/async, foo/]\n---\n");
    notebook
}

#[test]
fn tags_count_nested_tags_for_their_parents() {
    let notebook = nested_tags_notebook();

    assert_eq!(
        notebook.ok(&["tags"]),
        "1  foo\n3  lang\n1  lang/go\n2  lang/rust\n1  lang/rust/async\n"
    );
    assert_eq!(
        notebook.ok(&["tags", "--sort", "count"]),
        "3  lang\n2  lang/rust\n1  foo\n1  lang/go\n1  lang/rust/async\n"
    );
}

#[test]
fn tags_renders_nested_tags_as_tree() {
    let notebook = nested_tags_notebook();

    assert_eq!(
        notebook.ok(&["tags", "--tree"]),
        "foo (1)\nlang (3)\n├── go (1)\n└── rust (2)\n    └── async (1)\n"
    );
    assert_eq!(
        notebook.ok(&["tags", "--tree", "--sort", "count"]),
        "lang (3)\n├── rust (2)\n│   └── async (1)\n└── go (1)\nfoo (1)\n"
    );
}

#[test]
fn list_tag_filter_matches_nested_tags() {
    let notebook = nested_tags_notebook();

    assert_eq!(
        notebook.ok(&["list", "-t", "lang", "--format", "{path}"]),
        "a.md\nb.md\nc.md\n"
    );
    assert_eq!(
        notebook.ok(&["list", "-t", "LANG/rust/", "--format", "{path} {tags}"]),
        "a.md lang/rust, lang/go\nc.md lang/rust/async, foo\n"
    );
    assert_eq!(
        notebook.ok(&["list", "-t", "rust", "--format", "{path}"]),
        ""
    );
}

#[test]
fn tasks_filters_by_status() {
    let notebook = Notebook::new();

    assert_eq!(
        notebook.ok(&["tasks"]),
        "rust.md:5  [ ] open task\nrust.md:6  [x] done task\n"
    );
    assert_eq!(
        notebook.ok(&["tasks", "--status", "done"]),
        "rust.md:6  [x] done task\n"
    );
}

#[test]
fn tasks_skips_broken_notes_with_a_warning() {
    let notebook = Notebook::new();
    notebook.write("broken.md", "---\ntags: {a: 1}\n---\n- [ ] hidden task\n");
    notebook.write(
        "nested.md",
        "- [ ] parent\n  - [x] child with\n    two lines\n",
    );

    let output = notebook.run(&["tasks", "--status", "all"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "nested.md:1  [ ] parent\nnested.md:2  [x] child with two lines\nrust.md:5  [ ] open task\nrust.md:6  [x] done task\n"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("warning: skipping broken.md"));
}

#[test]
fn validate_reports_invalid_notes_and_broken_links() {
    let notebook = Notebook::new();
    assert!(
        notebook
            .ok(&["validate"])
            .ends_with("4 file(s) checked, 0 failed\n")
    );
    assert_eq!(notebook.ok(&["validate", "--quiet"]), "");

    notebook.write("broken.md", "---\ntags: {a: 1}\n---\n");
    notebook.write(
        "lonely.md",
        "# Lonely\n\n[gone](gone.md)\n[gone too](Gone%20Too)\n",
    );

    let (stdout, _) = notebook.fails(&["validate"]);
    assert!(
        stdout.contains("FAIL  broken.md: Invalid frontmatter"),
        "{stdout}"
    );
    assert!(
        stdout.contains("FAIL  lonely.md:3: broken link `gone.md`"),
        "{stdout}"
    );
    assert!(
        stdout.contains("FAIL  lonely.md:4: broken link `Gone%20Too`"),
        "{stdout}"
    );
    assert!(stdout.contains("ok    hub.md"), "{stdout}");
    assert!(
        stdout.ends_with("5 file(s) checked, 2 failed\n"),
        "{stdout}"
    );

    let (stdout, _) = notebook.fails(&["validate", "--quiet"]);
    assert_eq!(stdout.lines().count(), 3, "{stdout}");
    assert!(
        stdout.contains("FAIL  broken.md: Invalid frontmatter"),
        "{stdout}"
    );
    assert!(
        stdout.contains("FAIL  lonely.md:3: broken link `gone.md`"),
        "{stdout}"
    );
    assert!(
        stdout.contains("FAIL  lonely.md:4: broken link `Gone%20Too`"),
        "{stdout}"
    );
}

#[test]
fn notebook_dir_option_works_from_anywhere() {
    let notebook = Notebook::new();
    let elsewhere = Notebook::empty();

    let notebook_dir = notebook.path().to_str().unwrap();
    assert_eq!(
        elsewhere.ok(&[
            "-C",
            notebook_dir,
            "list",
            "-t",
            "project",
            "--format",
            "{path}"
        ]),
        "projects/zl.md\n"
    );
    assert_eq!(
        elsewhere.ok(&[
            "list",
            "--notebook-dir",
            notebook_dir,
            "--orphan",
            "--format",
            "{path}"
        ]),
        "lonely.md\n"
    );
}
