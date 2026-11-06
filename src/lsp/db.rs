use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
};

use rayon::prelude::*;
use salsa::Setter;

use crate::{
    notes::{self, Draft, Link},
    paths::normalize,
};

/// A note of the notebook, with the text of its open buffer or otherwise of the file on disk
#[salsa::input]
pub(crate) struct SourceFile {
    /// Path relative to the notebook root
    #[returns(ref)]
    pub(crate) path: PathBuf,
    #[returns(ref)]
    pub(crate) text: String,
}

/// All notes of the notebook, sorted by path
#[salsa::input]
pub(crate) struct Notebook {
    #[returns(ref)]
    files: Vec<SourceFile>,
}

/// The parsed note, parsed again whenever its text changes
#[salsa::tracked(returns(ref))]
fn draft(db: &dyn salsa::Database, file: SourceFile) -> Draft {
    Draft::parse(file.path(db), file.text(db))
}

/// The notes `file` links to. Without spans, so edits that don't change any link keep it equal and
/// salsa doesn't recompute [`backlinks`].
#[salsa::tracked(returns(ref))]
fn link_targets(db: &dyn salsa::Database, file: SourceFile) -> Vec<PathBuf> {
    let mut targets: Vec<_> = draft(db, file)
        .links
        .iter()
        .filter_map(|link| link.target.clone())
        .collect();
    targets.sort();
    targets.dedup();

    targets
}

/// Every linked note, existing or not, with the notes linking to it
#[salsa::tracked(returns(ref))]
fn backlinks(db: &dyn salsa::Database, notebook: Notebook) -> BTreeMap<PathBuf, Vec<SourceFile>> {
    let mut backlinks = BTreeMap::<_, Vec<_>>::new();
    for &file in notebook.files(db) {
        for target in link_targets(db, file) {
            backlinks.entry(target.clone()).or_default().push(file);
        }
    }

    backlinks
}

#[salsa::db]
#[derive(Clone, Default)]
pub(crate) struct Database {
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl salsa::Database for Database {}

/// The notes of a notebook, kept up to date by the language server and queried incrementally
pub(crate) struct Workspace {
    db: Database,
    notebook: Notebook,
    files: HashMap<PathBuf, SourceFile>,
}

impl Workspace {
    pub(crate) fn new() -> Self {
        Self::with_db(Database::default())
    }

    fn with_db(db: Database) -> Self {
        let notebook = Notebook::new(&db, Vec::new());
        Workspace {
            db,
            notebook,
            files: HashMap::new(),
        }
    }

    pub(crate) fn db(&self) -> &Database {
        &self.db
    }

    /// Reads all notes below the working directory, the notebook root. Unreadable notes are skipped.
    pub(crate) fn load_all(&mut self) -> anyhow::Result<()> {
        let texts: Vec<_> = notes::markdown_files(Path::new("."))?
            .into_par_iter()
            .filter_map(|path| match fs::read_to_string(&path) {
                Ok(text) => Some((normalize(&path), text)),
                Err(e) => {
                    eprintln!("Skipping {}: {e}", path.display());
                    None
                }
            })
            .collect();

        for (path, text) in texts {
            let file = SourceFile::new(&self.db, path.clone(), text);
            self.files.insert(path, file);
        }
        self.update_notebook();
        self.warm_up();

        Ok(())
    }

    /// Parses all notes in parallel, so the first request doesn't have to parse them one by one.
    /// Every thread queries its own handle of the database, they share the memoized results.
    fn warm_up(&self) {
        let files = self.notebook.files(&self.db);
        files
            .par_iter()
            .for_each_with(self.db.clone(), |db, &file| {
                link_targets(db, file);
            });

        backlinks(&self.db, self.notebook);
    }

    pub(crate) fn file(&self, path: &Path) -> Option<SourceFile> {
        self.files.get(path).copied()
    }

    /// All notes, sorted by path
    pub(crate) fn files(&self) -> &[SourceFile] {
        self.notebook.files(&self.db)
    }

    pub(crate) fn draft(&self, file: SourceFile) -> &Draft {
        draft(&self.db, file)
    }

    /// Sets the text of the note at `path`, adding it if it's new
    pub(crate) fn set_text(&mut self, path: PathBuf, text: String) {
        match self.files.get(&path) {
            Some(file) => {
                // Saving a buffer triggers a file change with the same text, keep the revision then
                if file.text(&self.db) != &text {
                    file.set_text(&mut self.db).to(text);
                }
            }
            None => {
                let file = SourceFile::new(&self.db, path.clone(), text);
                self.files.insert(path, file);
                self.update_notebook();
            }
        }
    }

    /// Re-reads the note at `path` from disk, removing it if it doesn't exist anymore
    pub(crate) fn reload(&mut self, path: PathBuf) {
        match fs::read_to_string(&path) {
            Ok(text) => self.set_text(path, text),
            Err(_) => self.remove(&path),
        }
    }

    pub(crate) fn remove(&mut self, path: &Path) {
        if self.files.remove(path).is_some() {
            self.update_notebook();
        }
    }

    fn update_notebook(&mut self) {
        let mut files: Vec<_> = self.files.iter().collect();
        files.sort_by_key(|(path, _)| *path);
        let files = files.into_iter().map(|(_, &file)| file).collect();

        self.notebook.set_files(&mut self.db).to(files);
    }

    /// The link of `file` that contains byte `offset`
    pub(crate) fn link_at(&self, file: SourceFile, offset: usize) -> Option<&Link> {
        self.draft(file)
            .links
            .iter()
            .find(|link| link.span.contains(&offset))
    }

    /// All links to the note at `target`, in the order of their notes and within them
    pub(crate) fn links_to(&self, target: &Path) -> Vec<(SourceFile, &Link)> {
        let Some(sources) = backlinks(&self.db, self.notebook).get(target) else {
            return Vec::new();
        };

        sources
            .iter()
            .flat_map(|&source| {
                self.draft(source)
                    .links
                    .iter()
                    .filter(|link| link.target.as_deref() == Some(target))
                    .map(move |link| (source, link))
            })
            .collect()
    }

    /// The links of `file` to notes that don't exist
    pub(crate) fn broken_links(&self, file: SourceFile) -> impl Iterator<Item = &Link> {
        self.draft(file).links.iter().filter(|link| {
            link.target
                .as_ref()
                .is_some_and(|target| !self.files.contains_key(target))
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    /// A workspace that records which queries are executed
    fn logged_workspace() -> (Workspace, Arc<Mutex<Vec<String>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let db = Database {
            storage: salsa::Storage::new(Some(Box::new({
                let log = log.clone();
                move |event| {
                    if let salsa::EventKind::WillExecute { database_key } = event.kind {
                        log.lock().unwrap().push(format!("{database_key:?}"));
                    }
                }
            }))),
        };

        (Workspace::with_db(db), log)
    }

    fn sources(workspace: &Workspace, target: &str) -> Vec<(String, String)> {
        workspace
            .links_to(Path::new(target))
            .into_iter()
            .map(|(source, link)| {
                let path = source.path(workspace.db()).display().to_string();
                (path, link.url.clone())
            })
            .collect()
    }

    #[test]
    fn finds_links_at_offsets() {
        let mut workspace = Workspace::new();
        workspace.set_text("a.md".into(), "see [b](b.md)".into());
        let file = workspace.file(Path::new("a.md")).unwrap();

        assert_eq!(workspace.link_at(file, 3), None);
        assert_eq!(
            workspace.link_at(file, 4).unwrap().target,
            Some("b.md".into())
        );

        workspace.set_text("a.md".into(), "[c](c.md)".into());
        assert_eq!(
            workspace.link_at(file, 4).unwrap().target,
            Some("c.md".into())
        );
    }

    #[test]
    fn updates_backlinks() {
        let mut workspace = Workspace::new();
        workspace.set_text("a.md".into(), "[b](b.md) [b again](b)".into());
        workspace.set_text("dir/c.md".into(), "[b](../b.md)".into());
        workspace.set_text("b.md".into(), "# B".into());

        assert_eq!(
            sources(&workspace, "b.md"),
            [
                ("a.md".into(), "b.md".into()),
                ("a.md".into(), "b".into()),
                ("dir/c.md".into(), "../b.md".into()),
            ]
        );

        workspace.set_text("a.md".into(), "no links".into());
        workspace.remove(Path::new("dir/c.md"));
        assert_eq!(sources(&workspace, "b.md"), []);
    }

    #[test]
    fn finds_broken_links() {
        let mut workspace = Workspace::new();
        workspace.set_text(
            "a.md".into(),
            "[b](b.md) [web](https://x.org) [c](c)".into(),
        );
        let file = workspace.file(Path::new("a.md")).unwrap();
        let broken = |workspace: &Workspace| -> Vec<String> {
            workspace
                .broken_links(file)
                .map(|link| link.url.clone())
                .collect()
        };

        assert_eq!(broken(&workspace), ["b.md", "c"]);
        workspace.set_text("c.md".into(), "# C".into());
        assert_eq!(broken(&workspace), ["b.md"]);
    }

    #[test]
    fn keeps_backlinks_when_links_stay_the_same() {
        let (mut workspace, log) = logged_workspace();
        workspace.set_text("a.md".into(), "[b](b.md)".into());
        workspace.set_text("b.md".into(), "# B".into());
        sources(&workspace, "b.md");
        log.lock().unwrap().clear();

        workspace.set_text("a.md".into(), "Text before [b](b.md)".into());
        assert_eq!(sources(&workspace, "b.md").len(), 1);

        let log = log.lock().unwrap();
        assert!(
            log.iter().any(|query| query.starts_with("draft")),
            "{log:?}"
        );
        assert!(
            !log.iter().any(|query| query.starts_with("backlinks")),
            "{log:?}"
        );
    }
}
