use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
use url::Url;
fn uri(path: &Path) -> String {
    Url::from_file_path(path).unwrap().to_string()
}
fn notification(method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","method":method,"params":params})
}
fn open(path: &Path, text: &str) -> Value {
    notification(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri(path),"languageId":"tynex","version":1,"text":text}}),
    )
}
fn change(path: &Path, version: i32, text: &str) -> Value {
    notification(
        "textDocument/didChange",
        json!({"textDocument":{"uri":uri(path),"version":version},"contentChanges":[{"text":text}]}),
    )
}
fn session(messages: Vec<Value>) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_arky"))
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut input = vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}),
        notification("initialized", json!({})),
    ];
    input.extend(messages);
    input.extend([
        json!({"jsonrpc":"2.0","id":999,"method":"shutdown"}),
        notification("exit", Value::Null),
    ]);
    for message in input {
        let text = message.to_string();
        write!(stdin, "Content-Length: {}\r\n\r\n{}", text.len(), text).unwrap();
    }
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut bytes = output.stdout.as_slice();
    let mut messages: Vec<Value> = vec![];
    while !bytes.is_empty() {
        let end = bytes.windows(4).position(|x| x == b"\r\n\r\n").unwrap();
        let length: usize = std::str::from_utf8(&bytes[..end])
            .unwrap()
            .strip_prefix("Content-Length: ")
            .unwrap()
            .parse()
            .unwrap();
        messages.push(serde_json::from_slice(&bytes[end + 4..end + 4 + length]).unwrap());
        bytes = &bytes[end + 4 + length..];
    }
    assert_eq!(
        messages[0]["result"]["capabilities"]["textDocumentSync"]["change"],
        1
    );
    messages
}
fn reports<'a>(messages: &'a [Value], path: &Path) -> Vec<&'a Value> {
    messages
        .iter()
        .filter(|m| {
            m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == uri(path)
        })
        .map(|m| &m["params"])
        .collect()
}
#[test]
fn unsaved_unicode_ranges_versions_and_close() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("한글 # 파일.arky");
    let bad = "-- header\r\nlocal emoji = '😀'; local n: number = '오류'\r\n";
    let good = "local n: number = 2";
    let messages = session(vec![
        open(&file, bad),
        change(&file, 2, good),
        change(&file, 1, bad),
        notification(
            "textDocument/didClose",
            json!({"textDocument":{"uri":uri(&file)}}),
        ),
    ]);
    let reports = reports(&messages, &file);
    assert_eq!(reports.len(), 3);
    let errors = reports[0]["diagnostics"].as_array().unwrap();
    assert!(!errors.is_empty());
    let range = &errors[0]["range"];
    assert_eq!(range["start"]["line"], 1);
    let line = bad.split('\n').nth(1).unwrap();
    let start = line.find("n:").unwrap();
    assert_eq!(
        range["start"]["character"],
        line[..start].encode_utf16().count()
    );
    assert_eq!(
        range["end"]["character"],
        line[..start + 1].encode_utf16().count()
    );
    assert_eq!(reports[1]["version"], 2);
    assert_eq!(reports[1]["diagnostics"], json!([]));
    assert_eq!(reports[2]["diagnostics"], json!([]));
    assert!(!file.exists());
}
#[test]
fn dependency_edits_recheck_importers_and_close_restores_disk() {
    let temp = tempfile::tempdir().unwrap();
    let main = temp.path().join("main.arky");
    let lib = temp.path().join("lib.arky");
    fs::write(&lib, "return 4").unwrap();
    let messages = session(vec![
        open(&main, "local n: number = require('lib')"),
        open(&lib, "return 'wrong'"),
        change(&lib, 2, "return 8"),
        change(&lib, 3, "return 'wrong'"),
        notification(
            "textDocument/didClose",
            json!({"textDocument":{"uri":uri(&lib)}}),
        ),
    ]);
    let reports = reports(&messages, &main);
    assert_eq!(reports.len(), 5);
    assert_eq!(reports[0]["diagnostics"], json!([]));
    assert!(!reports[1]["diagnostics"].as_array().unwrap().is_empty());
    assert_eq!(reports[2]["diagnostics"], json!([]));
    assert!(!reports[3]["diagnostics"].as_array().unwrap().is_empty());
    assert_eq!(reports[4]["diagnostics"], json!([]));
    assert_eq!(fs::read_to_string(&lib).unwrap(), "return 4");
}
#[test]
fn newly_created_dependency_and_error_clearing() {
    let temp = tempfile::tempdir().unwrap();
    let main = temp.path().join("main.arky");
    let lib = temp.path().join("lib.arky");
    let messages = session(vec![
        open(&main, "local n: number = require('lib')"),
        open(&lib, "return 5"),
        change(&lib, 2, "local x: number = false; return x"),
        change(&lib, 3, "return 5"),
    ]);
    let main_reports = reports(&messages, &main);
    assert_eq!(main_reports[0]["diagnostics"][0]["code"], "M101");
    assert_eq!(main_reports.last().unwrap()["diagnostics"], json!([]));
    let lib_reports = reports(&messages, &lib);
    assert!(!lib_reports[1]["diagnostics"].as_array().unwrap().is_empty());
    assert_eq!(lib_reports.last().unwrap()["diagnostics"], json!([]));
}
#[test]
fn file_policy_and_unsaved_declaration_are_shared_with_cli() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("tynex.toml"),
        "type_mode='strict'\n[[overrides]]\nfiles=['*.lua']\nmode='legacy'\n",
    )
    .unwrap();
    let main = temp.path().join("main.arky");
    let stub = temp.path().join("lib.d.arky");
    fs::write(temp.path().join("lib.lua"), "return 1").unwrap();
    let messages = session(vec![
        open(&stub, "export: string"),
        open(&main, "local n: number = require('lib')"),
        change(&stub, 2, "export: number"),
    ]);
    let reports = reports(&messages, &main);
    assert!(!reports[0]["diagnostics"].as_array().unwrap().is_empty());
    assert_eq!(reports.last().unwrap()["diagnostics"], json!([]));
}
#[test]
fn malformed_notification_does_not_stop_subsequent_checks() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("main.arky");
    let messages = session(vec![
        notification("textDocument/didOpen", json!({})),
        json!({"jsonrpc":"2.0","id":2,"method":"textDocument/hover","params":{}}),
        open(&file, "local x: number = false"),
    ]);
    assert!(messages.iter().any(|m| m["method"] == "window/logMessage"));
    assert!(
        messages
            .iter()
            .any(|m| m["id"] == 2 && m["error"]["code"] == -32601)
    );
    assert!(
        !reports(&messages, &file)[0]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
#[test]
fn abrupt_exit_and_oversized_frames_fail_cleanly() {
    for input in [
        "Content-Length: 999999999\r\n\r\n",
        "Content-Length: 33\r\n\r\n{\"jsonrpc\":\"2.0\",\"method\":\"exit\"}",
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_arky"))
            .arg("lsp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        assert!(!child.wait_with_output().unwrap().status.success());
    }
}

#[test]
fn live_transport_flushes_and_registers_watches_without_eof() {
    use std::io::{BufRead, BufReader, Read};
    use std::sync::mpsc;
    use std::time::Duration;
    let mut child = Command::new(env!("CARGO_BIN_EXE_arky"))
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header).unwrap() == 0 {
                break;
            }
            let len = header
                .trim()
                .strip_prefix("Content-Length: ")
                .unwrap()
                .parse()
                .unwrap();
            let mut blank = String::new();
            reader.read_line(&mut blank).unwrap();
            let mut bytes = vec![0; len];
            reader.read_exact(&mut bytes).unwrap();
            tx.send(serde_json::from_slice::<Value>(&bytes).unwrap())
                .unwrap();
        }
    });
    let send = |input: &mut std::process::ChildStdin, v: Value| {
        let text = v.to_string();
        write!(input, "Content-Length: {}\r\n\r\n{}", text.len(), text).unwrap();
        input.flush().unwrap();
    };
    send(
        &mut input,
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"capabilities":{"workspace":{"didChangeWatchedFiles":{"dynamicRegistration":true}}}}}),
    );
    let response = rx.recv_timeout(Duration::from_secs(5));
    if response.is_err() {
        child.kill().unwrap();
    }
    assert_eq!(response.unwrap()["id"], 0);
    send(&mut input, notification("initialized", json!({})));
    let registered = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(registered["method"], "client/registerCapability");
    send(
        &mut input,
        json!({"jsonrpc":"2.0","id":"tynex-watch","result":null}),
    );
    send(
        &mut input,
        json!({"jsonrpc":"2.0","id":1,"method":"shutdown"}),
    );
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap()["id"], 1);
    send(&mut input, notification("exit", Value::Null));
    drop(input);
    assert!(child.wait().unwrap().success());
    reader.join().unwrap();
}
