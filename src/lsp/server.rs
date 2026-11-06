use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use anyhow::Context;
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId, Response};
use lsp_types::{
    ClientCapabilities, CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams,
    CompletionResponse, CompletionTextEdit, Diagnostic, DiagnosticSeverity,
    DidChangeTextDocumentParams, DidChangeWatchedFilesParams,
    DidChangeWatchedFilesRegistrationOptions, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse,
    FileChangeType, FileSystemWatcher, GlobPattern, GotoDefinitionParams, GotoDefinitionResponse,
    Hover, HoverContents, HoverParams, HoverProviderCapability, InitializeParams, Location,
    MarkupContent, MarkupKind, OneOf, Position, PublishDiagnosticsParams, Range, ReferenceParams,
    Registration, RegistrationParams, ServerCapabilities, SymbolInformation, SymbolKind,
    TextDocumentPositionParams, TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Uri,
    WorkspaceSymbolParams, WorkspaceSymbolResponse,
    notification::{
        DidChangeTextDocument, DidChangeWatchedFiles, DidCloseTextDocument, DidOpenTextDocument,
        Initialized, Notification as _, PublishDiagnostics,
    },
    request::{
        Completion, DocumentSymbolRequest, GotoDefinition, HoverRequest, References,
        RegisterCapability, Request as _, WorkspaceSymbolRequest,
    },
};
use url::Url;

use super::{db::Workspace, line_index::LineIndex};
use crate::{
    config::ConfigDir,
    notes::{self, Heading, Link},
    paths::normalize,
};

/// Lines of a linked note shown on hover
const HOVER_LINES: usize = 20;

/// Runs the language server on stdin and stdout until the client shuts it down
pub(crate) fn run() -> anyhow::Result<()> {
    let (connection, io_threads) = Connection::stdio();

    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec!["(".into(), "/".into()]),
            ..CompletionOptions::default()
        }),
        document_symbol_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        ..ServerCapabilities::default()
    };
    let params: InitializeParams =
        serde_json::from_value(connection.initialize(serde_json::to_value(capabilities)?)?)?;

    // Like `-C`, so the notes are found and links resolved relative to the notebook root
    let root = notebook_root(&params)?;
    std::env::set_current_dir(&root)
        .with_context(|| format!("Failed to open notebook directory {}", root.display()))?;

    let mut server = Server {
        connection,
        root: std::env::current_dir()?.canonicalize()?,
        capabilities: params.capabilities,
        workspace: Workspace::new(),
        in_notebook: ConfigDir::current().exists(),
        open: HashSet::new(),
        published: HashMap::new(),
    };
    // Editors may start the server for a markdown file anywhere, e.g. in the home directory, which
    // shouldn't be read completely. Outside of notebooks only open notes are known.
    if server.in_notebook {
        server.workspace.load_all()?;
    } else {
        eprintln!(
            "{} is not a zl notebook, only open notes are indexed",
            root.display()
        );
    }
    server.main_loop()?;

    // The writer thread only stops once the connection is dropped
    drop(server);
    io_threads.join()?;

    Ok(())
}

/// The first workspace folder, otherwise the root of the client, otherwise the working directory
#[allow(deprecated)] // `root_uri` is deprecated in favor of workspace folders, but still common
fn notebook_root(params: &InitializeParams) -> anyhow::Result<PathBuf> {
    let uri = params
        .workspace_folders
        .as_ref()
        .and_then(|folders| folders.first())
        .map(|folder| &folder.uri)
        .or(params.root_uri.as_ref());

    match uri.and_then(to_file_path) {
        Some(root) => Ok(root),
        None => Ok(std::env::current_dir()?),
    }
}

struct Server {
    connection: Connection,
    /// Canonicalized notebook root, the working directory
    root: PathBuf,
    capabilities: ClientCapabilities,
    workspace: Workspace,
    /// Whether the root is a zl notebook, otherwise the workspace only knows the open notes
    in_notebook: bool,
    /// Notes open in the client, their text comes from the client instead of the disk
    open: HashSet<PathBuf>,
    /// The diagnostics last published for each open note, absent if there were none
    published: HashMap<PathBuf, Vec<Diagnostic>>,
}

impl Server {
    fn main_loop(&mut self) -> anyhow::Result<()> {
        while let Ok(message) = self.connection.receiver.recv() {
            match message {
                Message::Request(request) => {
                    if self.connection.handle_shutdown(&request)? {
                        return Ok(());
                    }
                    let response = self.handle_request(request);
                    self.connection.sender.send(response.into())?;
                }
                Message::Notification(notification) => self.handle_notification(notification)?,
                // Only answers to our registration requests, nothing to do with them
                Message::Response(_) => {}
            }
        }

        Ok(())
    }

    fn handle_request(&self, request: Request) -> Response {
        match request.method.as_str() {
            GotoDefinition::METHOD => {
                respond::<GotoDefinition>(request, |params| self.definition(params))
            }
            References::METHOD => respond::<References>(request, |params| self.references(params)),
            HoverRequest::METHOD => respond::<HoverRequest>(request, |params| self.hover(params)),
            Completion::METHOD => respond::<Completion>(request, |params| self.completion(params)),
            DocumentSymbolRequest::METHOD => {
                respond::<DocumentSymbolRequest>(request, |params| self.document_symbols(params))
            }
            WorkspaceSymbolRequest::METHOD => {
                respond::<WorkspaceSymbolRequest>(request, |params| self.workspace_symbols(params))
            }
            _ => Response::new_err(
                request.id,
                ErrorCode::MethodNotFound as i32,
                format!("Unsupported request {}", request.method),
            ),
        }
    }

    fn handle_notification(&mut self, notification: Notification) -> anyhow::Result<()> {
        match notification.method.as_str() {
            Initialized::METHOD => self.watch_notes()?,
            DidOpenTextDocument::METHOD => {
                let params: DidOpenTextDocumentParams =
                    notification.extract(DidOpenTextDocument::METHOD)?;
                if let Some(path) = self.note_path(&params.text_document.uri) {
                    self.open.insert(path.clone());
                    self.workspace.set_text(path, params.text_document.text);
                }
            }
            DidChangeTextDocument::METHOD => {
                let params: DidChangeTextDocumentParams =
                    notification.extract(DidChangeTextDocument::METHOD)?;
                // With full sync the last change holds the whole text
                if let Some(path) = self.note_path(&params.text_document.uri)
                    && let Some(change) = params.content_changes.into_iter().last()
                {
                    self.workspace.set_text(path, change.text);
                }
            }
            DidCloseTextDocument::METHOD => {
                let params: DidCloseTextDocumentParams =
                    notification.extract(DidCloseTextDocument::METHOD)?;
                // Unsaved changes are gone, the note is what's on disk again
                if let Some(path) = self.note_path(&params.text_document.uri) {
                    self.open.remove(&path);
                    if self.published.remove(&path).is_some() {
                        self.send_diagnostics(&path, Vec::new())?;
                    }
                    self.workspace.reload(path);
                }
            }
            DidChangeWatchedFiles::METHOD => {
                let params: DidChangeWatchedFilesParams =
                    notification.extract(DidChangeWatchedFiles::METHOD)?;
                for change in params.changes {
                    let Some(path) = self.note_path(&change.uri) else {
                        continue;
                    };
                    if self.open.contains(&path) {
                        continue;
                    }

                    if change.typ == FileChangeType::DELETED {
                        self.workspace.remove(&path);
                    } else {
                        self.workspace.reload(path);
                    }
                }
            }
            _ => {}
        }

        self.publish_diagnostics()
    }

    /// Publishes the broken links of every open note whose diagnostics changed. Any change can
    /// break or fix links of open notes, e.g. deleting a note on disk. Outside of notebooks only
    /// the open notes are known, every other link would look broken, so there are none.
    fn publish_diagnostics(&mut self) -> anyhow::Result<()> {
        if !self.in_notebook {
            return Ok(());
        }

        for path in &self.open {
            let diagnostics = self.diagnostics(path);
            let published = self.published.get(path).map_or(&[][..], Vec::as_slice);
            if diagnostics == published {
                continue;
            }

            self.send_diagnostics(path, diagnostics.clone())?;
            if diagnostics.is_empty() {
                self.published.remove(path);
            } else {
                self.published.insert(path.clone(), diagnostics);
            }
        }

        Ok(())
    }

    fn diagnostics(&self, path: &Path) -> Vec<Diagnostic> {
        let Some(file) = self.workspace.file(path) else {
            return Vec::new();
        };
        let index = LineIndex::new(file.text(self.workspace.db()));

        self.workspace
            .broken_links(file)
            .filter_map(|link| {
                let target = link.target.as_ref()?;
                Some(Diagnostic {
                    range: index.range(&link.span),
                    severity: Some(DiagnosticSeverity::WARNING),
                    source: Some("zl".into()),
                    message: format!("Note {} doesn't exist", target.display()),
                    ..Diagnostic::default()
                })
            })
            .collect()
    }

    fn send_diagnostics(&self, path: &Path, diagnostics: Vec<Diagnostic>) -> anyhow::Result<()> {
        let params = PublishDiagnosticsParams::new(self.uri(path), diagnostics, None);
        let notification = Notification::new(PublishDiagnostics::METHOD.into(), params);
        self.connection.sender.send(notification.into())?;

        Ok(())
    }

    /// Asks the client to report changes of notes made outside of it, e.g. by `zl new` or git
    fn watch_notes(&self) -> anyhow::Result<()> {
        let supported = self
            .capabilities
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.did_change_watched_files.as_ref())
            .and_then(|watched| watched.dynamic_registration)
            .unwrap_or(false);
        if !supported {
            return Ok(());
        }

        let options = DidChangeWatchedFilesRegistrationOptions {
            watchers: vec![FileSystemWatcher {
                glob_pattern: GlobPattern::String("**/*.md".into()),
                kind: None,
            }],
        };
        let params = RegistrationParams {
            registrations: vec![Registration {
                id: "watch-notes".into(),
                method: DidChangeWatchedFiles::METHOD.into(),
                register_options: Some(serde_json::to_value(options)?),
            }],
        };
        let request = Request::new(
            RequestId::from("watch-notes".to_string()),
            RegisterCapability::METHOD.into(),
            params,
        );
        self.connection.sender.send(request.into())?;

        Ok(())
    }

    /// Opens the note the link under the cursor points to
    fn definition(&self, params: GotoDefinitionParams) -> Option<GotoDefinitionResponse> {
        let (_, link) = self.cursor(&params.text_document_position_params)?;
        let target = link?.target.as_ref()?;
        self.workspace.file(target)?;

        Some(GotoDefinitionResponse::Scalar(self.location(target)))
    }

    /// The links to the note under the cursor: the target of the link under it, otherwise the note
    /// itself
    fn references(&self, params: ReferenceParams) -> Option<Vec<Location>> {
        let (path, link) = self.cursor(&params.text_document_position)?;
        let target = match link {
            Some(link) => link.target.clone()?,
            None => path,
        };

        let mut locations = Vec::new();
        if params.context.include_declaration && self.workspace.file(&target).is_some() {
            locations.push(self.location(&target));
        }

        let db = self.workspace.db();
        let links = self.workspace.links_to(&target);
        for links in links.chunk_by(|(a, _), (b, _)| a == b) {
            let source = links[0].0;
            let index = LineIndex::new(source.text(db));
            let uri = self.uri(source.path(db));

            locations.extend(
                links
                    .iter()
                    .map(|(_, link)| Location::new(uri.clone(), index.range(&link.span))),
            );
        }

        Some(locations)
    }

    /// The start of the note the link under the cursor points to
    fn hover(&self, params: HoverParams) -> Option<Hover> {
        let position = &params.text_document_position_params;
        let (path, link) = self.cursor(position)?;
        let link = link?;
        let target = self.workspace.file(link.target.as_ref()?)?;

        let db = self.workspace.db();
        let draft = self.workspace.draft(target);
        let body = target.text(db)[draft.body_start..].trim();
        let mut preview = match body {
            "" => draft.title.clone(),
            body => body
                .lines()
                .take(HOVER_LINES)
                .collect::<Vec<_>>()
                .join("\n"),
        };
        if body.lines().nth(HOVER_LINES).is_some() {
            preview.push_str("\n\n…");
        }

        let file = self.workspace.file(&path)?;
        let index = LineIndex::new(file.text(db));
        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: preview,
            }),
            range: Some(index.range(&link.span)),
        })
    }

    /// The notes to link to, when the cursor is in the destination of a link like `[text](`
    fn completion(&self, params: CompletionParams) -> Option<CompletionResponse> {
        let position = params.text_document_position;
        let path = self.note_path(&position.text_document.uri)?;
        let file = self.workspace.file(&path)?;

        let db = self.workspace.db();
        let text = file.text(db);
        let index = LineIndex::new(text);
        let cursor = index.offset(position.position);
        let line_start = index.offset(Position::new(position.position.line, 0));
        let start = line_start + destination_start(&text[line_start..cursor])?;
        let range = index.range(&(start..cursor));

        let items = self
            .workspace
            .files()
            .iter()
            .filter(|note| note.path(db) != &path)
            .map(|&note| {
                let url = notes::link_url(&path, note.path(db));
                CompletionItem {
                    label: url.clone(),
                    kind: Some(CompletionItemKind::FILE),
                    detail: Some(self.workspace.draft(note).title.clone()),
                    text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(range, url))),
                    ..CompletionItem::default()
                }
            })
            .collect();

        Some(CompletionResponse::Array(items))
    }

    /// The headings of the note as an outline
    fn document_symbols(&self, params: DocumentSymbolParams) -> Option<DocumentSymbolResponse> {
        let path = self.note_path(&params.text_document.uri)?;
        let file = self.workspace.file(&path)?;
        let text = file.text(self.workspace.db());
        let index = LineIndex::new(text);

        let headings = &self.workspace.draft(file).headings;
        Some(DocumentSymbolResponse::Nested(sections(
            headings,
            text.len(),
            &index,
        )))
    }

    /// The notes whose title or path contains every word of the query, ignoring case
    #[allow(deprecated)] // `SymbolInformation::deprecated` has to be set, but is never used
    fn workspace_symbols(&self, params: WorkspaceSymbolParams) -> Option<WorkspaceSymbolResponse> {
        let words: Vec<_> = params
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();

        let db = self.workspace.db();
        let symbols = self
            .workspace
            .files()
            .iter()
            .filter_map(|&file| {
                let path = file.path(db);
                let title = &self.workspace.draft(file).title;
                let haystack = format!("{title} {}", path.display()).to_lowercase();

                words
                    .iter()
                    .all(|word| haystack.contains(word))
                    .then(|| SymbolInformation {
                        name: title.clone(),
                        kind: SymbolKind::FILE,
                        tags: None,
                        deprecated: None,
                        location: self.location(path),
                        container_name: None,
                    })
            })
            .collect();

        Some(WorkspaceSymbolResponse::Flat(symbols))
    }

    /// The note at the cursor and the link under it, if any. Notes the workspace doesn't know, like
    /// ones outside of notebooks that aren't open, have no links.
    fn cursor(&self, position: &TextDocumentPositionParams) -> Option<(PathBuf, Option<&Link>)> {
        let path = self.note_path(&position.text_document.uri)?;
        let link = self.workspace.file(&path).and_then(|file| {
            let offset = LineIndex::new(file.text(self.workspace.db())).offset(position.position);
            self.workspace.link_at(file, offset)
        });

        Some((path, link))
    }

    /// Path relative to the notebook root of the note at `uri`. `None` for anything that isn't a
    /// note: files outside of the notebook, other than markdown or hidden like the templates.
    fn note_path(&self, uri: &Uri) -> Option<PathBuf> {
        let path = to_file_path(uri)?;
        if path.extension().is_none_or(|ext| ext != "md") {
            return None;
        }

        // Canonicalize only the folder, a new note may not exist on disk yet. The root is
        // canonicalized as well, so symlinks like /var -> /private/var on macOS don't matter.
        let folder = path.parent()?.canonicalize().ok()?;
        let path = normalize(
            &folder
                .strip_prefix(&self.root)
                .ok()?
                .join(path.file_name()?),
        );

        let hidden = path
            .components()
            .any(|component| component.as_os_str().to_string_lossy().starts_with('.'));

        (!hidden).then_some(path)
    }

    fn uri(&self, note: &Path) -> Uri {
        let url = Url::from_file_path(self.root.join(note)).expect("root is absolute");
        url.as_str().parse().expect("file URLs are valid URIs")
    }

    /// The start of `note`
    fn location(&self, note: &Path) -> Location {
        Location::new(self.uri(note), Range::default())
    }
}

/// Byte offset in `line`, the text of a line before the cursor, at which the destination of the
/// link being typed starts, e.g. right after `[text](`. `None` if the cursor isn't in one.
fn destination_start(line: &str) -> Option<usize> {
    let start = line.rfind("](")? + 2;
    let typed = &line[start..];

    (!typed.contains(|c: char| c == ')' || c.is_whitespace())).then_some(start)
}

/// `headings` as a tree in which every heading contains the deeper ones following it. The range
/// of a heading is its whole section, up to the next heading of the same or a higher level or `end`.
#[allow(deprecated)] // `DocumentSymbol::deprecated` has to be set, but is never used
fn sections(headings: &[Heading], end: usize, index: &LineIndex) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();
    let mut rest = headings;
    while let Some((heading, after)) = rest.split_first() {
        let deeper = after
            .iter()
            .take_while(|next| next.depth > heading.depth)
            .count();
        let (children, next) = after.split_at(deeper);
        let section_end = next.first().map_or(end, |next| next.span.start);

        symbols.push(DocumentSymbol {
            name: heading.text.clone(),
            detail: None,
            kind: SymbolKind::STRING,
            tags: None,
            deprecated: None,
            range: index.range(&(heading.span.start..section_end)),
            selection_range: index.range(&heading.span),
            children: Some(sections(children, section_end, index)),
        });
        rest = next;
    }

    symbols
}

fn to_file_path(uri: &Uri) -> Option<PathBuf> {
    Url::parse(uri.as_str()).ok()?.to_file_path().ok()
}

/// Answers `request` with the result of `handler`, or an error if its parameters are invalid
fn respond<R: lsp_types::request::Request>(
    request: Request,
    handler: impl FnOnce(R::Params) -> R::Result,
) -> Response {
    let id = request.id.clone();
    match request.extract::<R::Params>(R::METHOD) {
        Ok((id, params)) => Response::new_ok(id, handler(params)),
        Err(e) => Response::new_err(id, ErrorCode::InvalidParams as i32, e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_destinations_of_links_being_typed() {
        assert_eq!(destination_start("see [a]("), Some(8));
        assert_eq!(destination_start("[a](dir/no"), Some(4));
        assert_eq!(destination_start("[a](b.md) [c]("), Some(14));

        assert_eq!(destination_start("no link"), None);
        assert_eq!(destination_start("[a](b.md) text"), None);
        assert_eq!(destination_start("[a](b.md text"), None);
    }
}
