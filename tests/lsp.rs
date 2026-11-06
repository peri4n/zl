//! Runs `zl lsp` against a small notebook in a temporary directory, talking to it like an editor

use std::{
    collections::HashMap,
    fs,
    io::BufReader,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

use lsp_server::{Message, Notification, Request, RequestId};
use serde_json::{Value, json};
use tempfile::TempDir;
use url::Url;

/// An initialized notebook with a few linked notes:
///
/// - `hub.md` links to `projects/zl.md`, `rust.md` and an external URL
/// - `rust.md` and `projects/zl.md` link back to `hub.md`
/// - `Grüne Notiz.md` links to `rust.md` without extension
/// - `lonely.md` has no links and nothing links to it
fn notebook() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".zl")).unwrap();

    for (path, content) in [
        (
            "hub.md",
            "# Hub\n\n[zl](projects/zl.md) [rust](rust.md#intro) [web](https://example.com)\n",
        ),
        ("rust.md", "---\ntitle: Rust\n---\nSee [hub](./hub.md).\n"),
        ("projects/zl.md", "# zl\n\n[up](../hub.md)\n"),
        ("Grüne Notiz.md", "🦀 [rust](rust)\n"),
        ("lonely.md", "# Lonely\n"),
    ] {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    dir
}

/// A language server running for a notebook, with the notebook as workspace folder
struct Client {
    dir: TempDir,
    server: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i32,
    /// The diagnostics last published for each URI
    diagnostics: HashMap<String, Value>,
}

impl Client {
    fn start() -> Self {
        Self::start_in(notebook())
    }

    fn start_in(dir: TempDir) -> Self {
        let mut server = Command::new(env!("CARGO_BIN_EXE_zl"))
            .arg("lsp")
            .env_remove("ZL_NOTEBOOK_DIR")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();

        let mut client = Client {
            stdin: server.stdin.take().unwrap(),
            stdout: BufReader::new(server.stdout.take().unwrap()),
            server,
            dir,
            next_id: 0,
            diagnostics: HashMap::new(),
        };

        let root = client.uri("");
        client.request(
            "initialize",
            json!({
                "capabilities": {},
                "workspaceFolders": [{ "uri": root, "name": "notebook" }],
            }),
        );
        client.notify("initialized", json!({}));

        client
    }

    /// URI of `note` as the server reports it, below the canonicalized notebook root
    fn uri(&self, note: &str) -> String {
        let root = self.dir.path().canonicalize().unwrap();
        Url::from_file_path(root.join(note)).unwrap().to_string()
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = RequestId::from(self.next_id);
        let request = Request::new(id.clone(), method.into(), params);
        Message::from(request).write(&mut self.stdin).unwrap();

        loop {
            match Message::read(&mut self.stdout).unwrap().unwrap() {
                Message::Response(response) if response.id == id => {
                    return response.response_result.unwrap();
                }
                Message::Notification(notification)
                    if notification.method == "textDocument/publishDiagnostics" =>
                {
                    let uri = notification.params["uri"].as_str().unwrap().to_string();
                    self.diagnostics
                        .insert(uri, notification.params["diagnostics"].clone());
                }
                _ => continue,
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) {
        let notification = Notification::new(method.into(), params);
        Message::from(notification).write(&mut self.stdin).unwrap();
    }

    fn open(&mut self, note: &str, text: &str) {
        let params = json!({ "textDocument": {
            "uri": self.uri(note), "languageId": "markdown", "version": 1, "text": text,
        }});
        self.notify("textDocument/didOpen", params);
    }

    fn change(&mut self, note: &str, text: &str) {
        let params = json!({
            "textDocument": { "uri": self.uri(note), "version": 2 },
            "contentChanges": [{ "text": text }],
        });
        self.notify("textDocument/didChange", params);
    }

    /// The diagnostics of `note` as `(line, start character, end character, message)`, after the
    /// server handled every message sent so far
    fn diagnostics(&mut self, note: &str) -> Vec<(u64, u64, u64, String)> {
        // Answered after the notifications before it, so their diagnostics have arrived by then
        self.request("workspace/symbol", json!({ "query": "" }));

        let Some(diagnostics) = self.diagnostics.get(&self.uri(note)) else {
            return Vec::new();
        };
        diagnostics
            .as_array()
            .unwrap()
            .iter()
            .map(|diagnostic| {
                let range = &diagnostic["range"];
                (
                    range["start"]["line"].as_u64().unwrap(),
                    range["start"]["character"].as_u64().unwrap(),
                    range["end"]["character"].as_u64().unwrap(),
                    diagnostic["message"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    fn position(&self, note: &str, line: u32, character: u32) -> Value {
        json!({
            "textDocument": { "uri": self.uri(note) },
            "position": { "line": line, "character": character },
        })
    }

    fn definition(&mut self, note: &str, line: u32, character: u32) -> Value {
        let params = self.position(note, line, character);
        self.request("textDocument/definition", params)
    }

    /// The references as `(note, start line, start character, end character)`
    fn references(
        &mut self,
        note: &str,
        line: u32,
        character: u32,
    ) -> Vec<(String, u64, u64, u64)> {
        let mut params = self.position(note, line, character);
        params["context"] = json!({ "includeDeclaration": false });
        let root = format!("{}/", self.uri("").trim_end_matches('/'));

        let locations = self.request("textDocument/references", params);
        locations
            .as_array()
            .unwrap()
            .iter()
            .map(|location| {
                let range = &location["range"];
                (
                    location["uri"]
                        .as_str()
                        .unwrap()
                        .strip_prefix(&root)
                        .unwrap()
                        .to_string(),
                    range["start"]["line"].as_u64().unwrap(),
                    range["start"]["character"].as_u64().unwrap(),
                    range["end"]["character"].as_u64().unwrap(),
                )
            })
            .collect()
    }

    fn shutdown(mut self) {
        self.request("shutdown", Value::Null);
        self.notify("exit", Value::Null);
        assert!(self.server.wait().unwrap().success());
    }
}

fn reference(note: &str, line: u64, start: u64, end: u64) -> (String, u64, u64, u64) {
    (note.to_string(), line, start, end)
}

#[test]
fn goes_to_linked_notes() {
    let mut client = Client::start();

    let location = client.definition("hub.md", 2, 22);
    assert_eq!(location["uri"], client.uri("rust.md"));
    assert_eq!(
        location["range"]["start"],
        json!({ "line": 0, "character": 0 })
    );

    // Without extension, from a note whose name has to be percent-encoded
    let location = client.definition("Grüne Notiz.md", 0, 4);
    assert_eq!(location["uri"], client.uri("rust.md"));

    // Not on a link, on an external link and on a link to a missing note
    assert_eq!(client.definition("hub.md", 0, 0), Value::Null);
    assert_eq!(client.definition("hub.md", 2, 45), Value::Null);
    client.notify(
        "textDocument/didOpen",
        json!({ "textDocument": {
            "uri": client.uri("lonely.md"), "languageId": "markdown", "version": 1,
            "text": "[gone](gone.md)\n",
        }}),
    );
    assert_eq!(client.definition("lonely.md", 0, 1), Value::Null);

    client.shutdown();
}

#[test]
fn lists_links_to_notes() {
    let mut client = Client::start();

    // Outside of a link: links to the note itself. `🦀 ` is 3 UTF-16 code units.
    assert_eq!(
        client.references("rust.md", 0, 0),
        [
            reference("Gr%C3%BCne%20Notiz.md", 0, 3, 15),
            reference("hub.md", 2, 21, 42),
        ]
    );

    // On a link: links to its target
    assert_eq!(
        client.references("hub.md", 2, 22),
        client.references("rust.md", 0, 0)
    );
    assert_eq!(
        client.references("rust.md", 3, 5),
        [
            reference("projects/zl.md", 2, 0, 15),
            reference("rust.md", 3, 4, 19),
        ]
    );
    assert_eq!(client.references("lonely.md", 0, 0), []);

    client.shutdown();
}

#[test]
fn uses_unsaved_changes() {
    let mut client = Client::start();
    let uri = client.uri("lonely.md");

    client.notify(
        "textDocument/didOpen",
        json!({ "textDocument": {
            "uri": uri, "languageId": "markdown", "version": 1, "text": "# Lonely\n",
        }}),
    );
    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": uri, "version": 2 },
            "contentChanges": [{ "text": "# Lonely\n\n[rust](rust.md)\n" }],
        }),
    );
    assert!(
        client
            .references("rust.md", 0, 0)
            .contains(&reference("lonely.md", 2, 0, 15))
    );

    // Closing without saving discards the link
    client.notify(
        "textDocument/didClose",
        json!({ "textDocument": { "uri": uri } }),
    );
    assert!(
        !client
            .references("rust.md", 0, 0)
            .iter()
            .any(|(note, ..)| note == "lonely.md")
    );

    client.shutdown();
}

#[test]
fn picks_up_new_notes_on_disk() {
    let mut client = Client::start();

    fs::write(client.dir.path().join("new.md"), "[hub](hub.md)\n").unwrap();
    client.notify(
        "workspace/didChangeWatchedFiles",
        json!({ "changes": [{ "uri": client.uri("new.md"), "type": 1 }] }),
    );
    assert!(
        client
            .references("hub.md", 0, 0)
            .contains(&reference("new.md", 0, 0, 13))
    );

    fs::remove_file(client.dir.path().join("new.md")).unwrap();
    client.notify(
        "workspace/didChangeWatchedFiles",
        json!({ "changes": [{ "uri": client.uri("new.md"), "type": 3 }] }),
    );
    assert!(
        !client
            .references("hub.md", 0, 0)
            .iter()
            .any(|(note, ..)| note == "new.md")
    );

    client.shutdown();
}

#[test]
fn ignores_files_outside_of_the_notebook() {
    let mut client = Client::start();
    let outside = tempfile::tempdir().unwrap();
    let uri = Url::from_file_path(outside.path().join("other.md"))
        .unwrap()
        .to_string();

    client.notify(
        "textDocument/didOpen",
        json!({ "textDocument": {
            "uri": uri, "languageId": "markdown", "version": 1, "text": "[hub](hub.md)\n",
        }}),
    );
    assert_eq!(client.references("hub.md", 0, 0).len(), 2);

    // Hidden folders like the templates aren't notes either
    fs::create_dir_all(client.dir.path().join(".zl/templates")).unwrap();
    client.notify(
        "textDocument/didOpen",
        json!({ "textDocument": {
            "uri": client.uri(".zl/templates/default.md"), "languageId": "markdown", "version": 1,
            "text": "[hub](../../hub.md)\n",
        }}),
    );
    assert_eq!(client.references("hub.md", 0, 0).len(), 2);

    client.shutdown();
}

#[test]
fn only_knows_open_notes_outside_of_notebooks() {
    let dir = notebook();
    fs::remove_dir(dir.path().join(".zl")).unwrap();
    let mut client = Client::start_in(dir);

    assert_eq!(client.references("hub.md", 0, 0), []);
    client.notify(
        "textDocument/didOpen",
        json!({ "textDocument": {
            "uri": client.uri("lonely.md"), "languageId": "markdown", "version": 1,
            "text": "[hub](hub.md)\n",
        }}),
    );
    assert_eq!(
        client.references("hub.md", 0, 0),
        [reference("lonely.md", 0, 0, 13)]
    );
    // Every link would look broken, as only open notes are known
    client.open("rust.md", "[gone](gone.md)\n");
    assert_eq!(client.diagnostics("rust.md"), []);

    client.shutdown();
}

#[test]
fn reports_broken_links_of_open_notes() {
    let mut client = Client::start();
    let broken =
        |line, start, end, note: &str| (line, start, end, format!("Note {note} doesn't exist"));

    client.open(
        "lonely.md",
        "[hub](hub.md)\n[gone](gone.md) [new](sub/New%20Note)\n",
    );
    assert_eq!(
        client.diagnostics("lonely.md"),
        [
            broken(1, 0, 15, "gone.md"),
            broken(1, 16, 37, "sub/New Note.md")
        ]
    );

    client.change("lonely.md", "[gone](gone.md)\n");
    assert_eq!(
        client.diagnostics("lonely.md"),
        [broken(0, 0, 15, "gone.md")]
    );

    // Creating the note on disk fixes the link
    fs::write(client.dir.path().join("gone.md"), "# Gone\n").unwrap();
    client.notify(
        "workspace/didChangeWatchedFiles",
        json!({ "changes": [{ "uri": client.uri("gone.md"), "type": 1 }] }),
    );
    assert_eq!(client.diagnostics("lonely.md"), []);

    // Closing clears the diagnostics, as the server doesn't know about the unsaved changes anymore
    client.change("lonely.md", "[gone](missing.md)\n");
    assert_eq!(client.diagnostics("lonely.md").len(), 1);
    client.notify(
        "textDocument/didClose",
        json!({ "textDocument": { "uri": client.uri("lonely.md") } }),
    );
    assert_eq!(client.diagnostics("lonely.md"), []);

    client.shutdown();
}

#[test]
fn completes_links_to_notes() {
    let mut client = Client::start();

    client.open("lonely.md", "See [x](\n");
    let completion = client.request(
        "textDocument/completion",
        client.position("lonely.md", 0, 8),
    );
    let items: Vec<_> = completion
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                item["label"].as_str().unwrap(),
                item["detail"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        items,
        [
            ("Grüne%20Notiz.md", "Grüne Notiz"),
            ("hub.md", "Hub"),
            ("projects/zl.md", "zl"),
            ("rust.md", "Rust"),
        ]
    );

    // Replaces what's typed of the destination, relative to the note
    client.open("projects/zl.md", "# zl\n\n[up](../h\n");
    let completion = client.request(
        "textDocument/completion",
        client.position("projects/zl.md", 2, 9),
    );
    let hub = completion
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["label"] == "../hub.md")
        .unwrap();
    assert_eq!(
        hub["textEdit"],
        json!({
            "newText": "../hub.md",
            "range": { "start": { "line": 2, "character": 5 }, "end": { "line": 2, "character": 9 } },
        })
    );

    // Not in a link
    assert_eq!(
        client.request("textDocument/completion", client.position("hub.md", 0, 3)),
        Value::Null
    );

    client.shutdown();
}

#[test]
fn shows_linked_notes_on_hover() {
    let mut client = Client::start();

    // Without the frontmatter
    let hover = client.request("textDocument/hover", client.position("hub.md", 2, 22));
    assert_eq!(hover["contents"]["value"], "See [hub](./hub.md).");
    assert_eq!(
        hover["range"],
        json!({ "start": { "line": 2, "character": 21 }, "end": { "line": 2, "character": 42 } })
    );

    let long = (1..=30).map(|i| format!("line {i}\n")).collect::<String>();
    fs::write(client.dir.path().join("lonely.md"), &long).unwrap();
    client.notify(
        "workspace/didChangeWatchedFiles",
        json!({ "changes": [{ "uri": client.uri("lonely.md"), "type": 2 }] }),
    );
    client.open("rust.md", "[lonely](lonely.md)\n");
    let hover = client.request("textDocument/hover", client.position("rust.md", 0, 1));
    let preview = hover["contents"]["value"].as_str().unwrap();
    assert!(preview.starts_with("line 1\nline 2\n"), "{preview}");
    assert!(preview.ends_with("line 20\n\n…"), "{preview}");

    // Not on a link and on an external link
    assert_eq!(
        client.request("textDocument/hover", client.position("hub.md", 0, 0)),
        Value::Null
    );
    assert_eq!(
        client.request("textDocument/hover", client.position("hub.md", 2, 45)),
        Value::Null
    );

    client.shutdown();
}

#[test]
fn lists_headings_and_notes_as_symbols() {
    let mut client = Client::start();

    client.open("lonely.md", "# A\n\n## B\n\ntext\n\n# C\n");
    let symbols = client.request(
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": client.uri("lonely.md") } }),
    );
    let outline: Vec<_> = symbols
        .as_array()
        .unwrap()
        .iter()
        .map(|symbol| {
            let children: Vec<_> = symbol["children"]
                .as_array()
                .unwrap()
                .iter()
                .map(|child| child["name"].as_str().unwrap())
                .collect();
            (symbol["name"].as_str().unwrap(), children)
        })
        .collect();
    assert_eq!(outline, [("A", vec!["B"]), ("C", vec![])]);
    // A section ends where the next one of the same level starts
    assert_eq!(
        symbols[0]["range"],
        json!({ "start": { "line": 0, "character": 0 }, "end": { "line": 6, "character": 0 } })
    );

    let mut names = |query: &str| -> Vec<String> {
        let symbols = client.request("workspace/symbol", json!({ "query": query }));
        symbols
            .as_array()
            .unwrap()
            .iter()
            .map(|symbol| symbol["name"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(names("RUST"), ["Rust"]);
    assert_eq!(names("projects zl"), ["zl"]);
    assert_eq!(names("").len(), 5);

    client.shutdown();
}
