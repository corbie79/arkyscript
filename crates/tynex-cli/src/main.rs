mod bundle;
mod config;
mod dap;
mod lsp;
mod pathutil;
mod project;

use clap::{Args, Parser, Subcommand, ValueEnum};
use std::{
    ffi::OsString,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};
use tynex_compiler::Mode;

#[derive(Parser)]
#[command(
    name = "tynex",
    version,
    about = "Tynex — Typed Lua Development Kit: compiler, language server, and debugging tools"
)]
struct Cli {
    #[command(subcommand)]
    command: Action,
}
#[derive(Subcommand)]
enum Action {
    /// Serve editor diagnostics using the Language Server Protocol over stdio.
    Lsp,
    Check(Input),
    Gen(Input),
    Run(Execute),
    Debug(Execute),
    /// Emit a checked module bundle, generated Lua, and source maps.
    Bundle {
        #[command(flatten)]
        input: Input,
        #[arg(long)]
        out: PathBuf,
    },
    /// Execute a previously checked bundle (use --debug for an editor connection).
    ExecBundle {
        directory: PathBuf,
        #[arg(long)]
        debug: bool,
        #[arg(last = true)]
        args: Vec<OsString>,
    },
    /// Translate source locations for a stdio Debug Adapter Protocol backend.
    Dap(dap::Options),
}
#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Json,
}
#[derive(Args)]
struct Input {
    file: Option<PathBuf>,
    #[arg(long)]
    mode: Option<Mode>,
    #[arg(long)]
    project: Option<PathBuf>,
    #[arg(long, value_enum, default_value = "text")]
    format: Format,
}
#[derive(Args)]
struct Execute {
    #[command(flatten)]
    input: Input,
    #[arg(last = true)]
    args: Vec<OsString>,
}

fn main() -> ExitCode {
    match execute(Cli::parse()) {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("tynex: {message}");
            ExitCode::FAILURE
        }
    }
}

fn runtime() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("TYNEX_LUAJIT") {
        return pathutil::canonicalize(&PathBuf::from(path))
            .map_err(|e| format!("TYNEX_LUAJIT: {e}"));
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let filename = if cfg!(windows) {
        "luajit.exe"
    } else {
        "luajit"
    };
    let sibling = exe.parent().unwrap().join(filename);
    if sibling.is_file() {
        return Ok(sibling);
    }
    // A packaged CLI uses a sibling engine. Repository builds discover the pin
    // next to their target directory; no baked-in developer machine paths.
    for ancestor in exe.ancestors().skip(1) {
        let pin = ancestor.join("dependencies.json");
        if !pin.is_file() {
            continue;
        }
        let data: serde_json::Value =
            serde_json::from_slice(&fs::read(&pin).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let sha = data["luajit"]["revision"]
            .as_str()
            .ok_or("missing LuaJIT revision")?;
        if sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("invalid pinned LuaJIT revision".into());
        }
        let path = if cfg!(windows) {
            ancestor.join("build/windows-x64").join(sha).join(filename)
        } else {
            ancestor
                .join("build/native")
                .join(sha)
                .join("src")
                .join(filename)
        };
        if path.is_file() {
            return Ok(path);
        }
        break;
    }
    Err("LuaJIT runtime not found. Run make setup, place the engine beside tynex, or set TYNEX_LUAJIT.".into())
}

fn execute(cli: Cli) -> Result<u8, String> {
    let mut bundle_out = None;
    let (action, input, args) = match cli.command {
        Action::Lsp => return lsp::serve(),
        Action::Dap(options) => return dap::serve(options),
        Action::ExecBundle {
            directory,
            debug,
            args,
        } => return bundle::execute(&directory, debug, args),
        Action::Bundle { input, out } => {
            bundle_out = Some(out);
            ("bundle", input, vec![])
        }
        Action::Check(i) => ("check", i, vec![]),
        Action::Gen(i) => ("gen", i, vec![]),
        Action::Run(e) => ("run", e.input, e.args),
        Action::Debug(e) => ("debug", e.input, e.args),
    };
    if !matches!(input.format, Format::Text) && action != "check" {
        return Err("--format json is available for check only".into());
    }
    if action != "check" && input.file.is_none() {
        return Err(format!("{action} requires an entry FILE"));
    }
    let entry = input
        .file
        .as_ref()
        .map(|p| pathutil::canonicalize(p).map_err(|e| format!("{}: {e}", p.display())))
        .transpose()?;
    if action != "check" && entry.as_ref().is_some_and(|p| project::is_declaration(p)) {
        return Err(
            "declaration files can only be checked; they do not execute or generate Lua".into(),
        );
    }
    let mut project = project::Project::new(entry.clone(), input.project.as_deref(), input.mode)?;
    project.build()?;
    if !project.issues.is_empty() {
        report(&input, &project, entry.as_deref());
        return Ok(1);
    }
    // Legacy modules remain opaque to Rust: ask the official VM to parse them,
    // with LUA_INIT removed and without executing their bodies or require calls.
    let legacy: Vec<_> = project
        .units
        .iter()
        .filter(|(_, u)| u.mode == Mode::Legacy && !u.declaration)
        .map(|(p, _)| p.clone())
        .collect();
    let temporary = tempfile::tempdir().map_err(|e| e.to_string())?;
    let engine = if !legacy.is_empty() || action == "run" || action == "debug" {
        Some(runtime()?)
    } else {
        None
    };
    if !legacy.is_empty() {
        let checker = temporary.path().join("syntax.lua");
        fs::write(
            &checker,
            "local f,e=loadfile(arg[1]); if not f then io.stderr:write(e, '\\n'); os.exit(1) end",
        )
        .map_err(|e| e.to_string())?;
        for file in legacy {
            let output = Command::new(engine.as_ref().unwrap())
                .arg(&checker)
                .arg(&file)
                .env_remove("LUA_INIT")
                .output()
                .map_err(|e| e.to_string())?;
            if !output.status.success() {
                let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
                let line = message
                    .split(':')
                    .find_map(|s| s.parse::<usize>().ok())
                    .unwrap_or(1);
                project.issue(file, "legacy", "LUA_SYNTAX", message, (line, 1));
            }
        }
    }
    if !project.issues.is_empty() {
        report(&input, &project, entry.as_deref());
        return Ok(1);
    }
    if action == "check" {
        report(&input, &project, entry.as_deref());
        return Ok(0);
    }
    let entry = entry.unwrap();
    if let Some(out) = bundle_out {
        bundle::write(&project, &entry, &out)?;
        println!("Bundle: {}", out.display());
        return Ok(0);
    }
    if action == "gen" {
        let unit = &project.units[&entry];
        io::stdout()
            .write_all(
                unit.lua
                    .as_ref()
                    .map(String::as_bytes)
                    .unwrap_or(&unit.source),
            )
            .map_err(|e| e.to_string())?;
        return Ok(0);
    }
    let manifest = write_manifest(&project, &entry, temporary.path(), false)?;
    let runner = temporary.path().join("runner.lua");
    fs::write(&runner, include_str!("../../../runtime/runner.lua")).map_err(|e| e.to_string())?;
    let engine = engine.unwrap();
    let status = Command::new(&engine)
        .arg(runner)
        .arg(action)
        .arg(manifest)
        .args(args)
        .env("TYNEX_LUAJIT_DIR", engine.parent().unwrap())
        .status()
        .map_err(|e| format!("{}: {e}", engine.display()))?;
    Ok(status
        .code()
        .and_then(|n| u8::try_from(n).ok())
        .unwrap_or(1))
}
fn report(input: &Input, project: &project::Project, entry: Option<&Path>) {
    let mode = entry
        .and_then(|p| project.units.get(p))
        .map(|u| u.mode.to_string());
    let modules: Vec<_> = project.units.iter().map(|(p,u)|serde_json::json!({"file":p,"mode":u.mode.to_string(),"declaration":u.declaration,"exports":u.interface.to_string(),"dynamic_imports":u.dynamic_imports})).collect();
    let scope = if mode.as_deref() == Some("legacy") {
        "syntax only; type checking skipped for legacy bodies"
    } else {
        "static module graph checked; dynamic boundaries may be unchecked"
    };
    if matches!(input.format, Format::Json) {
        println!(
            "{}",
            serde_json::json!({"ok":project.issues.is_empty(),"file":entry,"mode":mode,"project":project.root,"scope":scope,"modules":modules,"diagnostics":project.issues})
        );
    } else if project.issues.is_empty() {
        println!(
            "OK ({}; {scope}; {} files): {}",
            mode.as_deref().unwrap_or("project"),
            project.units.len(),
            entry.unwrap_or(&project.root).display()
        );
    } else {
        for d in &project.issues {
            eprintln!(
                "{}:{}:{}: error[{}]: {}",
                d.file.display(),
                d.line,
                d.column,
                d.code,
                d.message
            );
        }
    }
}
fn lua_string(text: &str) -> String {
    // Decimal byte escapes are valid Lua 5.1, including UTF-8 and Windows paths.
    let mut out = String::from("\"");
    for byte in text.bytes() {
        out.push_str(&format!("\\{byte:03}"));
    }
    out.push('"');
    out
}
fn lua_path(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(lua_string)
        .ok_or_else(|| format!("runtime manifest requires a UTF-8 path: {}", path.display()))
}
fn write_manifest(
    project: &project::Project,
    entry: &Path,
    directory: &Path,
    mapped: bool,
) -> Result<PathBuf, String> {
    let mut definitions = std::collections::BTreeMap::new();
    for (index, (path, unit)) in project.units.iter().enumerate() {
        if unit.declaration {
            continue;
        }
        let generated = if let Some(lua) = &unit.lua {
            let file = directory.join(format!("module-{index}.lua"));
            if !mapped {
                fs::write(&file, lua).map_err(|e| e.to_string())?;
            }
            lua_path(&file)?
        } else {
            "false".into()
        };
        definitions.insert(
            path,
            format!(
                "{{source={},generated={generated},mapped={mapped}}}",
                lua_path(path)?
            ),
        );
    }
    let mut manifest = format!(
        "return {{entry={},roots={{",
        definitions[&entry.to_path_buf()]
    );
    for root in &project.search {
        manifest.push_str(&lua_path(root)?);
        manifest.push(',');
    }
    manifest.push_str("},modules={");
    for (name, path) in &project.names {
        if let Some(definition) = definitions.get(path) {
            manifest.push_str(&format!("[{}]={definition},", lua_string(name)));
        }
    }
    manifest.push_str("},locations={");
    if mapped {
        for (index, (path, unit)) in project.units.iter().enumerate() {
            let Some(map) = &unit.source_map else {
                continue;
            };
            let generated = directory.join(format!("module-{index}.lua"));
            manifest.push_str(&format!(
                "[{}]={{source={},lines={{",
                lua_path(&generated)?,
                lua_path(path)?
            ));
            let mut lines = std::collections::BTreeMap::new();
            for mapping in &map.mappings {
                if let Some(original) = mapping.original {
                    lines
                        .entry(mapping.generated.line + 3)
                        .or_insert(original.line + 1);
                }
            }
            for (generated, original) in lines {
                manifest.push_str(&format!("[{generated}]={original},"));
            }
            manifest.push_str("}},");
        }
    }
    manifest.push_str("}}\n");
    let path = directory.join("manifest.lua");
    fs::write(&path, manifest).map_err(|e| e.to_string())?;
    Ok(path)
}
