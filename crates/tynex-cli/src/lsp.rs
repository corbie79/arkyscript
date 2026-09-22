//! A diagnostics-only LSP. Each update rebuilds open roots with a shared snapshot
//! of unsaved sources; the CLI resolver and compiler remain the source of truth.
use crate::{pathutil, project::Project};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, BufRead, Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
};
use tynex_compiler::Mode;
use url::Url;

const MAX_MESSAGE: usize = 16 * 1024 * 1024;
#[derive(Deserialize)]
struct DocumentId {
    uri: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocumentParams {
    text_document: DocumentId,
}
#[derive(Deserialize)]
struct OpenDocument {
    uri: String,
    version: i32,
    text: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenParams {
    text_document: OpenDocument,
}
#[derive(Deserialize)]
struct VersionedId {
    uri: String,
    version: i32,
}
#[derive(Deserialize)]
struct Change {
    text: String,
    range: Option<Value>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChangeParams {
    text_document: VersionedId,
    content_changes: Vec<Change>,
}
#[derive(Default)]
struct Server {
    initialized: bool,
    shutdown: bool,
    watch_supported: bool,
    documents: BTreeMap<PathBuf, OpenDocument>,
    published: BTreeSet<String>,
}

pub fn serve() -> Result<u8, String> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    let mut server = Server::default();
    while let Some(bytes) = read_frame(&mut input).map_err(|e| e.to_string())? {
        let message: Value = match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(error) => {
                send(
                    &mut output,
                    error_response(Value::Null, -32700, error.to_string()),
                )?;
                continue;
            }
        };
        if let Some(code) = server.handle(message, &mut output)? {
            return Ok(code);
        }
    }
    Ok(if server.shutdown { 0 } else { 1 })
}

impl Server {
    fn handle(&mut self, message: Value, out: &mut impl Write) -> Result<Option<u8>, String> {
        let id = message.get("id").cloned();
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return Ok(None); // Responses to client-side requests aren't used.
        };
        if method == "exit" {
            return Ok(Some(if self.shutdown { 0 } else { 1 }));
        }
        if method == "initialize" && !self.initialized && id.is_some() {
            self.initialized = true;
            self.watch_supported = message
                .pointer("/params/capabilities/workspace/didChangeWatchedFiles/dynamicRegistration")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            send(
                out,
                json!({"jsonrpc":"2.0", "id":id, "result":{
                    "capabilities":{
                        "positionEncoding":"utf-16",
                        "textDocumentSync":{"openClose":true,"change":1,"save":{"includeText":false}}
                    },
                    "serverInfo":{"name":"Tynex","version":env!("CARGO_PKG_VERSION")}
                }}),
            )?;
            return Ok(None);
        }
        if !self.initialized || self.shutdown {
            if let Some(id) = id {
                let code = if self.shutdown { -32600 } else { -32002 };
                send(
                    out,
                    error_response(id, code, "server is not running".into()),
                )?;
            }
            return Ok(None);
        }
        if let Some(id) = id {
            if method == "shutdown" {
                self.shutdown = true;
                send(out, json!({"jsonrpc":"2.0","id":id,"result":null}))?;
            } else {
                send(
                    out,
                    error_response(id, -32601, format!("unsupported method: {method}")),
                )?;
            }
            return Ok(None);
        }
        if method == "initialized" && self.watch_supported {
            self.watch_supported = false;
            send(
                out,
                json!({"jsonrpc":"2.0","id":"tynex-watch","method":"client/registerCapability","params":{
                    "registrations":[{"id":"tynex-files","method":"workspace/didChangeWatchedFiles","registerOptions":{
                        "watchers":[{"globPattern":"**/*.tlj","kind":7},{"globPattern":"**/*.lua","kind":7},{"globPattern":"**/tynex.toml","kind":7}]
                    }}]
                }}),
            )?;
        }
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        match self.update(method, params) {
            Ok(true) => self.diagnose(out)?,
            Ok(false) => {}
            Err(message) => send(
                out,
                json!({"jsonrpc":"2.0","method":"window/logMessage","params":{"type":1,"message":message}}),
            )?,
        }
        Ok(None)
    }
    fn update(&mut self, method: &str, params: Value) -> Result<bool, String> {
        match method {
            "textDocument/didOpen" => {
                let p: OpenParams = decode(params)?;
                let path = document_path(&p.text_document.uri)?;
                if self.documents.len() >= 256 && !self.documents.contains_key(&path) {
                    return Err("Tynex supports at most 256 open documents".into());
                }
                self.documents.insert(path, p.text_document);
            }
            "textDocument/didChange" => {
                let p: ChangeParams = decode(params)?;
                let path = document_path(&p.text_document.uri)?;
                let doc = self
                    .documents
                    .get_mut(&path)
                    .ok_or("change for unopened document")?;
                if p.text_document.version <= doc.version {
                    return Ok(false);
                }
                if p.content_changes.iter().any(|c| c.range.is_some()) {
                    return Err("Tynex negotiated full document synchronization; ranged changes are unsupported".into());
                }
                if let Some(change) = p.content_changes.into_iter().last() {
                    doc.text = change.text;
                    doc.version = p.text_document.version;
                }
            }
            "textDocument/didClose" => {
                let p: DocumentParams = decode(params)?;
                self.documents.remove(&document_path(&p.text_document.uri)?);
            }
            "textDocument/didSave"
            | "workspace/didChangeWatchedFiles"
            | "workspace/didChangeConfiguration" => {}
            _ => return Ok(false),
        }
        Ok(true)
    }
    fn diagnose(&mut self, out: &mut impl Write) -> Result<(), String> {
        let overlays: BTreeMap<_, _> = self
            .documents
            .iter()
            .map(|(p, d)| (p.clone(), d.text.as_bytes().to_vec()))
            .collect();
        let mut diagnostics: BTreeMap<PathBuf, Vec<Value>> =
            self.documents.keys().map(|p| (p.clone(), vec![])).collect();
        let mut checked_legacy = BTreeSet::new();
        for path in self.documents.keys() {
            let mut project = match Project::new(Some(path.clone()), None, None) {
                Ok(project) => project,
                Err(message) => {
                    diagnostics
                        .entry(path.clone())
                        .or_default()
                        .push(simple_diagnostic("CONFIG", message, 1));
                    continue;
                }
            };
            project.overlays = overlays.clone();
            if let Err(message) = project.build() {
                diagnostics
                    .entry(path.clone())
                    .or_default()
                    .push(simple_diagnostic("PROJECT", message, 1));
            }
            for issue in &project.issues {
                let source = project
                    .sources
                    .get(&issue.file)
                    .and_then(|s| std::str::from_utf8(s).ok())
                    .unwrap_or("");
                let (start, end) = if let Some(span) = &issue.span {
                    (position(source, span.start), position(source, span.end))
                } else {
                    let byte = location_byte(source, issue.line, issue.column);
                    (position(source, byte), position(source, byte))
                };
                diagnostics
                    .entry(issue.file.clone())
                    .or_default()
                    .push(json!({
                        "range":{"start":start,"end":end},"severity":1,"code":issue.code,
                        "source":"tynex","message":issue.message
                    }));
            }
            for (file, unit) in &project.units {
                if unit.mode == Mode::Legacy
                    && !unit.declaration
                    && checked_legacy.insert(file.clone())
                {
                    match legacy_syntax(&unit.source) {
                        Ok(Some((line, message))) => {
                            let source = String::from_utf8_lossy(&unit.source);
                            let pos = position(&source, location_byte(&source, line, 1));
                            diagnostics.entry(file.clone()).or_default().push(json!({"range":{"start":pos,"end":pos},"severity":1,"source":"tynex","code":"LUA_SYNTAX","message":message}));
                        }
                        Err(message) => diagnostics
                            .entry(file.clone())
                            .or_default()
                            .push(simple_diagnostic("LUA_RUNTIME", message, 2)),
                        Ok(None) => {}
                    }
                }
            }
        }
        let mut published = BTreeSet::new();
        for (path, mut errors) in diagnostics {
            // Multiple open roots may report the same dependency error.
            let mut seen = BTreeSet::new();
            errors.retain(|e| seen.insert(e.to_string()));
            let doc = self.documents.get(&path);
            let uri = match doc {
                Some(doc) => doc.uri.clone(),
                None => Url::from_file_path(&path)
                    .map_err(|_| "invalid diagnostic file path")?
                    .to_string(),
            };
            let mut params = json!({"uri":uri,"diagnostics":errors});
            if let Some(doc) = doc {
                params["version"] = json!(doc.version);
            }
            send(
                out,
                json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":params}),
            )?;
            published.insert(uri);
        }
        for uri in self.published.difference(&published) {
            send(
                out,
                json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":uri,"diagnostics":[]}}),
            )?;
        }
        self.published = published;
        Ok(())
    }
}
fn legacy_syntax(source: &[u8]) -> Result<Option<(usize, String)>, String> {
    let engine = crate::runtime().map_err(|e| format!("Legacy syntax check unavailable: {e}"))?;
    let mut child = Command::new(engine)
        .args([
            "-e",
            "local f,e=loadfile(); if not f then io.stderr:write(e); os.exit(1) end",
        ])
        .env_remove("LUA_INIT")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let written = child.stdin.take().unwrap().write_all(source);
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    written.map_err(|e| e.to_string())?;
    if output.status.success() {
        return Ok(None);
    }
    let message = String::from_utf8_lossy(&output.stderr).into_owned();
    let line = message.split(':').find_map(|s| s.parse().ok()).unwrap_or(1);
    Ok(Some((line, message)))
}
fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|e| format!("invalid LSP notification: {e}"))
}
fn document_path(uri: &str) -> Result<PathBuf, String> {
    let url = Url::parse(uri).map_err(|e| e.to_string())?;
    if url.query().is_some() || url.fragment().is_some() {
        return Err("file URI cannot contain query or fragment".into());
    }
    let path = url
        .to_file_path()
        .map_err(|_| "only local file documents are supported")?;
    if !matches!(
        path.extension().and_then(|x| x.to_str()),
        Some("lua" | "tlj")
    ) {
        return Err("expected .lua or .tlj document".into());
    }
    pathutil::source_path(&path).map_err(|e| e.to_string())
}
fn position(source: &str, byte: usize) -> Value {
    let mut byte = byte.min(source.len());
    while !source.is_char_boundary(byte) {
        byte -= 1;
    }
    let prefix = &source[..byte];
    let line = prefix.bytes().filter(|b| *b == b'\n').count();
    let character = prefix
        .rsplit('\n')
        .next()
        .unwrap_or("")
        .trim_end_matches('\r')
        .encode_utf16()
        .count();
    json!({"line":line,"character":character})
}
fn location_byte(source: &str, line: usize, column: usize) -> usize {
    let mut offset = 0;
    for (index, text) in source.split_inclusive('\n').enumerate() {
        if index + 1 == line {
            return offset
                + text
                    .char_indices()
                    .nth(column.saturating_sub(1))
                    .map(|(i, _)| i)
                    .unwrap_or(text.trim_end_matches(['\n', '\r']).len());
        }
        offset += text.len();
    }
    source.len()
}
fn simple_diagnostic(code: &str, message: String, severity: u8) -> Value {
    json!({"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"severity":severity,"code":code,"source":"tynex","message":message})
}
fn error_response(id: Value, code: i32, message: String) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
fn send(out: &mut impl Write, message: Value) -> Result<(), String> {
    let data = serde_json::to_vec(&message).map_err(|e| e.to_string())?;
    write!(out, "Content-Length: {}\r\n\r\n", data.len())
        .and_then(|_| out.write_all(&data))
        .and_then(|_| out.flush())
        .map_err(|e| e.to_string())
}
pub(crate) fn read_frame(input: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut length = None;
    let mut total = 0;
    loop {
        let mut line = String::new();
        let read = input.take(8193).read_line(&mut line)?;
        if read == 0 && total == 0 {
            return Ok(None);
        }
        total += read;
        if read == 0 || total > 8192 {
            return Err(io::Error::other("invalid LSP headers"));
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| io::Error::other("invalid LSP header"))?;
        if key.eq_ignore_ascii_case("Content-Length") {
            if length.is_some() {
                return Err(io::Error::other("duplicate Content-Length"));
            }
            length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| io::Error::other("invalid Content-Length"))?,
            );
        }
    }
    let length = length
        .filter(|n| *n <= MAX_MESSAGE)
        .ok_or_else(|| io::Error::other("missing or oversized Content-Length"))?;
    let mut body = vec![0; length];
    input.read_exact(&mut body)?;
    Ok(Some(body))
}
