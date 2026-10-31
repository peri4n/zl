use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use anyhow::{Context, anyhow, bail};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::{
    config::ConfigDir,
    notes::{self, Link, Note, Task},
    paths::normalize,
};

/// Bumped on every schema change, a database with another version is rebuilt from scratch
const SCHEMA_VERSION: i32 = 2;

const SCHEMA: &str = "
    CREATE TABLE notes (
        id        INTEGER PRIMARY KEY,
        path      TEXT    NOT NULL UNIQUE,  -- relative to the notebook root, '/' separated
        title     TEXT    NOT NULL,
        modified  INTEGER NOT NULL,         -- mtime in ns since the epoch, together with size used
        size      INTEGER NOT NULL          -- to detect changed files
    );

    CREATE TABLE tags (
        note_id   INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
        position  INTEGER NOT NULL,         -- order in the frontmatter
        name      TEXT    NOT NULL,         -- as written, e.g. 'Übung'
        key       TEXT    NOT NULL,         -- Unicode lowercase of name, as NOCASE only folds ASCII
        PRIMARY KEY (note_id, position)
    );
    CREATE INDEX tags_by_key ON tags(key);

    CREATE TABLE links (
        source_id   INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
        position    INTEGER NOT NULL,       -- order in the note
        line        INTEGER NOT NULL,
        url         TEXT    NOT NULL,       -- as written, e.g. '../a.md#intro'
        -- Resolved note path, NULL for external links. Not a foreign key, as links may point to
        -- notes that don't exist (yet).
        target_path TEXT,
        PRIMARY KEY (source_id, position)
    );
    CREATE INDEX links_by_target ON links(target_path);

    CREATE TABLE tasks (
        note_id   INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
        position  INTEGER NOT NULL,         -- order in the note
        line      INTEGER NOT NULL,
        checked   INTEGER NOT NULL,         -- 1 for `- [x]`, 0 for `- [ ]`
        text      TEXT    NOT NULL,
        PRIMARY KEY (note_id, position)
    );
";

/// Cache of the parsed notes of the notebook in `.zl/index.db`, refreshed on every `open` for files
/// that changed since. It is never a source of truth: deleting it only means a full rebuild.
/// Outside an initialized notebook the index is kept in memory.
pub(crate) struct Index {
    connection: Connection,
    /// Notes that couldn't be parsed, they're not indexed
    skipped: Vec<(PathBuf, anyhow::Error)>,
}

pub(crate) struct BrokenLink {
    /// Note containing the link
    pub(crate) path: PathBuf,
    pub(crate) line: usize,
    pub(crate) url: String,
}

impl Index {
    pub(crate) fn open() -> anyhow::Result<Self> {
        let config_dir = ConfigDir::current();
        let connection = if config_dir.exists() {
            open_file(&config_dir.index_file())?
        } else {
            let connection = Connection::open_in_memory()?;
            prepare(&connection)?;
            connection
        };

        let mut index = Index { connection, skipped: Vec::new() };
        index.refresh().context("Failed to update the index")?;

        Ok(index)
    }

    /// Prints a warning for every note that couldn't be parsed, so a single broken note doesn't hide
    /// all others
    pub(crate) fn warn_skipped(&self) {
        for (path, error) in &self.skipped {
            eprintln!("warning: skipping {}: {:#}", path.display(), error);
        }
    }

    /// Notes at or below `path` that couldn't be parsed and therefore aren't indexed, with the reason
    pub(crate) fn skipped(&self, path: &Path) -> anyhow::Result<impl Iterator<Item = &(PathBuf, anyhow::Error)>> {
        let prefix = relative_to_root(path)?;
        Ok(self.skipped.iter().filter(move |(path, _)| path.starts_with(&prefix)))
    }

    /// All notes at or below `path`, ordered by path
    pub(crate) fn notes(&self, path: &Path) -> anyhow::Result<Vec<Note>> {
        let prefix = relative_to_root(path)?;

        let mut notes: HashMap<i64, Note> = HashMap::new();
        let mut statement = self.connection.prepare("SELECT id, path, title FROM notes")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;
        for row in rows {
            let (id, path, title) = row?;
            let path = PathBuf::from(path);
            if path.starts_with(&prefix) {
                notes.insert(id, Note { path, title, tags: Vec::new(), links: Vec::new(), tasks: Vec::new() });
            }
        }

        let mut statement = self.connection.prepare("SELECT note_id, name FROM tags ORDER BY note_id, position")?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?;
        for row in rows {
            let (id, name) = row?;
            if let Some(note) = notes.get_mut(&id) {
                note.tags.push(name);
            }
        }

        let mut statement = self.connection
            .prepare("SELECT source_id, url, line, target_path FROM links ORDER BY source_id, position")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                Link {
                    url: row.get(1)?,
                    line: row.get::<_, i64>(2)? as usize,
                    target: row.get::<_, Option<String>>(3)?.map(PathBuf::from),
                },
            ))
        })?;
        for row in rows {
            let (id, link) = row?;
            if let Some(note) = notes.get_mut(&id) {
                note.links.push(link);
            }
        }

        let mut statement = self.connection
            .prepare("SELECT note_id, line, checked, text FROM tasks ORDER BY note_id, position")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                Task { line: row.get::<_, i64>(1)? as usize, checked: row.get(2)?, text: row.get(3)? },
            ))
        })?;
        for row in rows {
            let (id, task) = row?;
            if let Some(note) = notes.get_mut(&id) {
                note.tasks.push(task);
            }
        }

        let mut notes: Vec<_> = notes.into_values().collect();
        notes.sort_by(|a, b| a.path.cmp(&b.path));

        Ok(notes)
    }

    /// Paths of the notes linking to `target`, excluding `target` itself
    pub(crate) fn backlinks(&self, target: &Path) -> anyhow::Result<HashSet<PathBuf>> {
        let target = path_key(&relative_to_root(target)?);
        self.paths(
            "SELECT DISTINCT n.path FROM links l JOIN notes n ON n.id = l.source_id
             WHERE l.target_path = ?1 AND n.path != ?1",
            [target],
        )
    }

    /// Paths of the notes no other note links to
    pub(crate) fn orphans(&self) -> anyhow::Result<HashSet<PathBuf>> {
        self.paths(
            "SELECT n.path FROM notes n WHERE NOT EXISTS (
                 SELECT 1 FROM links l WHERE l.target_path = n.path AND l.source_id != n.id)",
            [],
        )
    }

    /// Links in notes at or below `path` to notes that don't exist, ordered by note and line
    pub(crate) fn broken_links(&self, path: &Path) -> anyhow::Result<Vec<BrokenLink>> {
        let prefix = relative_to_root(path)?;
        let mut statement = self.connection.prepare(
            "SELECT n.path, l.line, l.url FROM links l
             JOIN notes n ON n.id = l.source_id
             LEFT JOIN notes t ON t.path = l.target_path
             WHERE l.target_path IS NOT NULL AND t.id IS NULL
             ORDER BY n.path, l.position",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(BrokenLink {
                path: PathBuf::from(row.get::<_, String>(0)?),
                line: row.get::<_, i64>(1)? as usize,
                url: row.get(2)?,
            })
        })?;

        let links: Vec<BrokenLink> = rows.collect::<Result<_, _>>()?;
        Ok(links.into_iter().filter(|link| link.path.starts_with(&prefix)).collect())
    }

    fn paths(&self, sql: &str, params: impl rusqlite::Params) -> anyhow::Result<HashSet<PathBuf>> {
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map(params, |row| row.get::<_, String>(0).map(PathBuf::from))?;

        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Re-parses notes whose modification time or size changed and removes deleted ones,
    /// all in one transaction
    fn refresh(&mut self) -> anyhow::Result<()> {
        let parse_options = notes::parse_options();
        let transaction = self.connection.transaction()?;

        let mut indexed: HashMap<String, (i64, i64)> = HashMap::new();
        {
            let mut statement = transaction.prepare("SELECT path, modified, size FROM notes")?;
            let rows = statement.query_map([], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?))))?;
            for row in rows {
                let (path, stamp) = row?;
                indexed.insert(path, stamp);
            }
        }

        let mut skipped = Vec::new();
        for file in notes::markdown_files(Path::new(".")).context("Failed to read the notebook directory")? {
            let path = normalize(&file);
            let key = path_key(&path);
            let stamp = file_stamp(&path).with_context(|| format!("Failed to read {}", path.display()))?;

            if indexed.remove(&key) == Some(stamp) {
                continue;
            }

            transaction.execute("DELETE FROM notes WHERE path = ?1", [&key])?;
            match Note::load(&path, &parse_options) {
                Ok(note) => insert(&transaction, &key, &note, stamp)?,
                Err(error) => skipped.push((path, error)),
            }
        }

        // Whatever is left wasn't found on disk anymore
        for key in indexed.keys() {
            transaction.execute("DELETE FROM notes WHERE path = ?1", [key])?;
        }

        transaction.commit()?;
        self.skipped = skipped;

        Ok(())
    }
}

/// Opens the database at `path`, rebuilding it if it's unreadable, e.g. corrupted or not a database
fn open_file(path: &Path) -> anyhow::Result<Connection> {
    let open = || -> rusqlite::Result<Connection> {
        let connection = Connection::open(path)?;
        prepare(&connection)?;
        Ok(connection)
    };

    open().or_else(|_| {
        fs::remove_file(path).with_context(|| format!("Failed to remove broken index {}", path.display()))?;
        open().with_context(|| format!("Failed to create index {}", path.display()))
    })
}

/// Enables foreign keys and (re)creates the schema if the database is new or has an old version
fn prepare(connection: &Connection) -> rusqlite::Result<()> {
    connection.pragma_update(None, "foreign_keys", true)?;

    let version: i32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != SCHEMA_VERSION {
        connection.execute_batch(&format!(
            "BEGIN;
             DROP TABLE IF EXISTS tasks;
             DROP TABLE IF EXISTS links;
             DROP TABLE IF EXISTS tags;
             DROP TABLE IF EXISTS notes;
             {SCHEMA}
             PRAGMA user_version = {SCHEMA_VERSION};
             COMMIT;"
        ))?;
    }

    // Fails on files that aren't SQLite databases, which `open` alone doesn't notice
    connection.query_row("SELECT count(*) FROM notes", [], |_| Ok(())).optional()?;

    Ok(())
}

fn insert(transaction: &Transaction, key: &str, note: &Note, (modified, size): (i64, i64)) -> rusqlite::Result<()> {
    transaction
        .prepare_cached("INSERT INTO notes (path, title, modified, size) VALUES (?1, ?2, ?3, ?4)")?
        .execute(params![key, note.title, modified, size])?;
    let id = transaction.last_insert_rowid();

    let mut insert_tag =
        transaction.prepare_cached("INSERT INTO tags (note_id, position, name, key) VALUES (?1, ?2, ?3, ?4)")?;
    for (position, tag) in note.tags.iter().enumerate() {
        insert_tag.execute(params![id, position as i64, tag, tag.to_lowercase()])?;
    }

    let mut insert_link = transaction.prepare_cached(
        "INSERT INTO links (source_id, position, line, url, target_path) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for (position, link) in note.links.iter().enumerate() {
        let target = link.target.as_deref().map(path_key);
        insert_link.execute(params![id, position as i64, link.line as i64, link.url, target])?;
    }

    let mut insert_task = transaction.prepare_cached(
        "INSERT INTO tasks (note_id, position, line, checked, text) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for (position, task) in note.tasks.iter().enumerate() {
        insert_task.execute(params![id, position as i64, task.line as i64, task.checked, task.text])?;
    }

    Ok(())
}

/// Modification time in ns and size of a file, if either changed the file has to be re-parsed
fn file_stamp(path: &Path) -> std::io::Result<(i64, i64)> {
    let metadata = fs::metadata(path)?;
    let modified = metadata.modified()?
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos() as i64);

    Ok((modified, metadata.len() as i64))
}

/// Path as stored in the database: relative to the notebook root and `/` separated on every platform
fn path_key(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// `path` relative to the notebook root, the working directory. Absolute paths have to be inside it.
fn relative_to_root(path: &Path) -> anyhow::Result<PathBuf> {
    if !path.exists() {
        bail!("Path not found: {}", path.display());
    }

    if path.is_relative() {
        return Ok(normalize(path));
    }

    // Canonicalize both, so symlinks like /var -> /private/var on macOS don't break the comparison
    let root = std::env::current_dir()?.canonicalize()?;
    let path = path.canonicalize()?;
    path.strip_prefix(&root)
        .map(Path::to_path_buf)
        .map_err(|_| anyhow!("{} is outside of the notebook {}", path.display(), root.display()))
}
