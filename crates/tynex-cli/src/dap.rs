//! Stdio DAP location bridge for backends without native source-map support.
use crate::bundle::{DebugMap, FileMap};
use clap::Args;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    io::{self, BufReader, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
};

#[derive(Args)]
pub struct Options {
    #[arg(long)]
    map: PathBuf,
    #[arg(long)]
    backend: PathBuf,
    #[arg(last = true)]
    args: Vec<OsString>,
}
struct Bridge {
    map: DebugMap,
    breakpoints: BTreeMap<u64, usize>,
    reverse_requests: BTreeMap<u64, u64>,
    sequence: u64,
    line_base: usize,
    column_base: usize,
    uri: bool,
}
fn path_key(path: &str) -> String {
    let path = url::Url::parse(path)
        .ok()
        .filter(|u| u.scheme() == "file")
        .and_then(|u| u.to_file_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.into());
    let path = path.replace('\\', "/");
    if cfg!(windows) || path.as_bytes().get(1) == Some(&b':') {
        path.to_lowercase()
    } else {
        path
    }
}
impl Bridge {
    fn new(map: DebugMap) -> Self {
        Self {
            map,
            breakpoints: BTreeMap::new(),
            reverse_requests: BTreeMap::new(),
            sequence: 0,
            line_base: 1,
            column_base: 1,
            uri: false,
        }
    }
    fn file(&self, path: &Value, generated: bool) -> Option<usize> {
        let key = path_key(path.as_str()?);
        self.map.files.iter().position(|f| {
            path_key(&if generated { &f.generated } else { &f.original }.to_string_lossy()) == key
        })
    }
    fn path(&self, path: &std::path::Path) -> String {
        if self.uri {
            url::Url::from_file_path(path)
                .map(|u| u.to_string())
                .unwrap_or_else(|_| path.to_string_lossy().into_owned())
        } else {
            path.to_string_lossy().into_owned()
        }
    }
    fn request(&mut self, message: &mut Value) -> Option<Value> {
        if message["type"] == "response" {
            if let Some(original) = message["request_seq"]
                .as_u64()
                .and_then(|s| self.reverse_requests.remove(&s))
            {
                message["request_seq"] = original.into();
            }
            return None;
        }
        if message["command"] == "initialize" {
            self.line_base = usize::from(message["arguments"]["linesStartAt1"] != false);
            self.column_base = usize::from(message["arguments"]["columnsStartAt1"] != false);
            self.uri = message["arguments"]["pathFormat"] == "uri";
        }
        let index = self.file(&message["arguments"]["source"]["path"], false)?;
        let file = &self.map.files[index];
        if message["command"] == "source" {
            let content = fs::read_to_string(&file.original);
            return Some(match content {
                Ok(content) => {
                    json!({"type":"response","command":"source","request_seq":message["seq"],"success":true,"body":{"content":content,"mimeType":"text/x-lua"}})
                }
                Err(error) => {
                    json!({"type":"response","command":"source","request_seq":message["seq"],"success":false,"message":error.to_string()})
                }
            });
        }
        if !matches!(
            message["command"].as_str(),
            Some("setBreakpoints" | "breakpointLocations" | "gotoTargets")
        ) {
            return None;
        }
        let args = &mut message["arguments"];
        args["source"]["path"] = self.path(&file.generated).into();
        args["source"]["sourceReference"] = 0.into();
        if let Some(name) = file.generated.file_name() {
            args["source"]["name"] = name.to_string_lossy().into_owned().into();
        }
        if let Some(points) = args["breakpoints"].as_array_mut() {
            for point in points {
                self.forward_location(point, file);
            }
        }
        if let Some(lines) = args["lines"].as_array_mut() {
            for line in lines {
                if let Some(n) = line.as_u64() {
                    *line = self.forward_line(file, n as usize).into();
                }
            }
        }
        self.forward_location(args, file);
        if let Some(seq) = message["seq"].as_u64() {
            self.breakpoints.insert(seq, index);
        }
        None
    }
    fn forward_line(&self, file: &FileMap, line: usize) -> usize {
        let original = line.saturating_sub(self.line_base) + 1;
        file.lines
            .iter()
            .position(|p| p.as_ref().is_some_and(|p| p.line >= original))
            .unwrap_or(file.lines.len())
            + self.line_base
    }
    fn forward_location(&self, object: &mut Value, file: &FileMap) {
        for key in ["line", "endLine"] {
            if let Some(line) = object[key].as_u64() {
                object[key] = self.forward_line(file, line as usize).into();
            }
        }
        // Lua backends stop at line granularity; do not invent exact lowered columns.
        for key in ["column", "endColumn"] {
            if object.get(key).is_some() {
                object[key] = self.column_base.into();
            }
        }
    }
    fn reverse_location(&self, object: &mut Value, fallback: Option<usize>) {
        let index = self.file(&object["source"]["path"], true).or(fallback);
        let Some(index) = index else { return };
        let file = &self.map.files[index];
        let Some(line) = object["line"].as_u64() else {
            return;
        };
        let Some(Some(location)) = file
            .lines
            .get((line as usize).saturating_sub(self.line_base))
        else {
            return;
        };
        object["line"] = (location.line - 1 + self.line_base).into();
        object["column"] = (location.column - 1 + self.column_base).into();
        if let Some(end) = object["endLine"].as_u64() {
            if let Some(Some(end)) = file
                .lines
                .get((end as usize).saturating_sub(self.line_base))
            {
                object["endLine"] = (end.line - 1 + self.line_base).into();
                object["endColumn"] = (end.column - 1 + self.column_base).into();
            } else if let Some(o) = object.as_object_mut() {
                o.remove("endLine");
                o.remove("endColumn");
            }
        }
        object["source"] = json!({"path":self.path(&file.original),"name":file.original.file_name().map(|name|name.to_string_lossy()),"sourceReference":0});
    }
    fn text(&self, text: &str) -> String {
        let mut result = text.to_owned();
        for file in &self.map.files {
            // Restrict replacements to a known generated path followed by a line
            // number. This also preserves drive-letter colons and Unicode paths.
            let prefix = format!("{}:", file.generated.display());
            let mut next = String::new();
            let mut rest = result.as_str();
            while let Some(index) = rest.find(&prefix) {
                next.push_str(&rest[..index]);
                let suffix = &rest[index + prefix.len()..];
                let digits = suffix.bytes().take_while(u8::is_ascii_digit).count();
                let mapped = suffix[..digits]
                    .parse::<usize>()
                    .ok()
                    .filter(|n| *n > 0)
                    .and_then(|n| file.lines.get(n - 1))
                    .and_then(|p| p.as_ref());
                if let Some(location) = mapped {
                    next.push_str(&format!("{}:{}", file.original.display(), location.line));
                } else {
                    next.push_str(&prefix);
                    next.push_str(&suffix[..digits]);
                }
                rest = &suffix[digits..];
            }
            next.push_str(rest);
            result = next;
        }
        result
    }
    fn response(&mut self, message: &mut Value) {
        let fallback = if message["type"] == "response" {
            message["request_seq"]
                .as_u64()
                .and_then(|seq| self.breakpoints.remove(&seq))
        } else {
            None
        };
        if let Some(frames) = message
            .get_mut("body")
            .and_then(|body| body.get_mut("stackFrames"))
            .and_then(Value::as_array_mut)
        {
            for frame in frames {
                self.reverse_location(frame, None);
            }
        }
        if let Some(points) = message
            .get_mut("body")
            .and_then(|body| body.get_mut("breakpoints"))
            .and_then(Value::as_array_mut)
        {
            for point in points {
                self.reverse_location(point, fallback);
            }
        }
        if let Some(targets) = message
            .get_mut("body")
            .and_then(|body| body.get_mut("targets"))
            .and_then(Value::as_array_mut)
        {
            for target in targets {
                self.reverse_location(target, fallback);
            }
        }
        if message["event"] == "breakpoint" {
            self.reverse_location(&mut message["body"]["breakpoint"], None);
        }
        if message["event"] == "output" {
            self.reverse_location(&mut message["body"], None);
            if let Some(text) = message["body"]["output"].as_str() {
                message["body"]["output"] = self.text(text).into();
            }
        }
        if message["command"] == "exceptionInfo" {
            for pointer in [
                "/body/description",
                "/body/details/message",
                "/body/details/stackTrace",
            ] {
                if let Some(text) = message.pointer(pointer).and_then(Value::as_str) {
                    let text = self.text(text);
                    *message.pointer_mut(pointer).unwrap() = text.into();
                }
            }
        }
    }
    fn send(&mut self, out: &mut impl Write, mut message: Value) -> Result<(), String> {
        self.sequence += 1;
        if message["type"] == "request"
            && let Some(original) = message["seq"].as_u64()
        {
            self.reverse_requests.insert(self.sequence, original);
        }
        message["seq"] = self.sequence.into();
        send(out, &message)
    }
}
fn send(out: &mut impl Write, message: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(message).map_err(|e| e.to_string())?;
    write!(out, "Content-Length: {}\r\n\r\n", bytes.len())
        .and_then(|_| out.write_all(&bytes))
        .and_then(|_| out.flush())
        .map_err(|e| e.to_string())
}
enum Event {
    Client(Result<Option<Value>, String>),
    Backend(Result<Option<Value>, String>),
}
fn read(reader: &mut impl io::BufRead) -> Result<Option<Value>, String> {
    crate::lsp::read_frame(reader)
        .map_err(|e| e.to_string())?
        .map(|bytes| serde_json::from_slice(&bytes).map_err(|e| e.to_string()))
        .transpose()
}
pub fn serve(options: Options) -> Result<u8, String> {
    let map: DebugMap = serde_json::from_slice(&fs::read(options.map).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if map.version != 1 {
        return Err("unsupported debug-map version".into());
    }
    if map.files.iter().any(|f| {
        f.lines
            .iter()
            .flatten()
            .any(|p| p.line == 0 || p.column == 0)
    }) {
        return Err("invalid debug-map location".into());
    }
    let mut bridge = Bridge::new(map);
    let mut child = Command::new(options.backend)
        .args(options.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut input = child.stdin.take().unwrap();
    let output = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::sync_channel(256);
    let backend_tx = tx.clone();
    thread::spawn(move || {
        let mut reader = BufReader::new(output);
        loop {
            let result = read(&mut reader);
            let end = !matches!(result, Ok(Some(_)));
            if backend_tx.send(Event::Backend(result)).is_err() || end {
                break;
            }
        }
    });
    thread::spawn(move || {
        let mut reader = io::stdin().lock();
        loop {
            let result = read(&mut reader);
            let end = !matches!(result, Ok(Some(_)));
            if tx.send(Event::Client(result)).is_err() || end {
                break;
            }
        }
    });
    let mut out = io::stdout().lock();
    let mut terminating = false;
    let result = (|| {
        while let Ok(event) = rx.recv() {
            match event {
                Event::Client(Ok(Some(mut message))) => {
                    terminating |= matches!(
                        message["command"].as_str(),
                        Some("disconnect" | "terminate")
                    );
                    if let Some(response) = bridge.request(&mut message) {
                        bridge.send(&mut out, response)?;
                    } else {
                        send(&mut input, &message)?;
                    }
                }
                Event::Backend(Ok(Some(mut message))) => {
                    terminating |= message["event"] == "terminated";
                    bridge.response(&mut message);
                    bridge.send(&mut out, message)?;
                }
                Event::Client(Err(e)) | Event::Backend(Err(e)) => return Err(e),
                Event::Client(Ok(None)) => break,
                Event::Backend(Ok(None)) if terminating => break,
                Event::Backend(Ok(None)) => {
                    return Err(
                        "debug adapter closed its output before terminating the session".into(),
                    );
                }
            }
        }
        Ok(0)
    })();
    drop(input);
    let _ = child.kill();
    let _ = child.wait();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bridge() -> Bridge {
        Bridge::new(serde_json::from_value(json!({"version":1,"files":[{"original":"/src/한글.tlj","generated":"/out/module.lua","lines":[null,null,{"line":1,"column":1},{"line":2,"column":3},{"line":2,"column":3},{"line":4,"column":1}]}]})).unwrap())
    }
    #[test]
    fn original_breakpoints_and_expanded_stack() {
        let mut b = bridge();
        let mut request = json!({"type":"request","seq":7,"command":"setBreakpoints","arguments":{"source":{"path":"/src/한글.tlj"},"breakpoints":[{"line":2,"condition":"n == 3","hitCondition":"2","logMessage":"{n}"}]}});
        b.request(&mut request);
        assert_eq!(request["arguments"]["breakpoints"][0]["line"], 4);
        assert_eq!(
            request["arguments"]["breakpoints"][0]["condition"],
            "n == 3"
        );
        let mut reply = json!({"type":"response","request_seq":7,"body":{"breakpoints":[{"line":4,"verified":true}]}});
        b.response(&mut reply);
        assert_eq!(reply["body"]["breakpoints"][0]["line"], 2);
        let mut stack = json!({"body":{"stackFrames":[{"source":{"path":"/out/module.lua","sourceReference":4},"line":5,"column":1},{"source":{"path":"/out/module.lua"},"line":1}]}});
        b.response(&mut stack);
        assert_eq!(stack["body"]["stackFrames"][0]["line"], 2);
        assert_eq!(
            stack["body"]["stackFrames"][0]["source"]["sourceReference"],
            0
        );
        assert_eq!(
            stack["body"]["stackFrames"][1]["source"]["path"],
            "/out/module.lua"
        );
    }
    #[test]
    fn zero_based_protocol_and_error_stack() {
        let mut b = bridge();
        b.request(&mut json!({"command":"initialize","arguments":{"linesStartAt1":false,"columnsStartAt1":false}}));
        let mut point = json!({"source":{"path":"/out/module.lua"},"line":3});
        b.reverse_location(&mut point, None);
        assert_eq!(point["line"], 1);
        assert_eq!(point["column"], 2);
        assert_eq!(
            b.text("/out/module.lua:4: failed\n /out/module.lua:6"),
            "/src/한글.tlj:2: failed\n /src/한글.tlj:4"
        );
    }
    #[test]
    fn windows_path_and_reverse_request_sequence() {
        assert_eq!(path_key("C:\\Work\\File.lua"), path_key("c:/work/file.lua"));
        let mut b = bridge();
        let mut output = vec![];
        b.send(
            &mut output,
            json!({"type":"request","seq":50,"command":"runInTerminal"}),
        )
        .unwrap();
        let mut reply = json!({"type":"response","request_seq":1});
        b.request(&mut reply);
        assert_eq!(reply["request_seq"], 50);
    }
}
