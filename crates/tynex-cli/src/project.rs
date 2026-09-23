use crate::{
    config::{self, Config},
    pathutil,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
use tynex_compiler::{Diagnostic, Interface, Mode, declaration, prepare};

#[derive(Serialize)]
pub struct Issue {
    #[serde(skip)]
    pub span: Option<std::ops::Range<usize>>,
    pub code: String,
    pub severity: &'static str,
    pub file: PathBuf,
    pub line: usize,
    pub column: usize,
    pub mode: String,
    pub message: String,
}
pub struct Unit {
    pub source: Vec<u8>,
    pub mode: Mode,
    pub lua: Option<String>,
    pub source_map: Option<tynex_compiler::source_map::SourceMap>,
    pub interface: Interface,
    pub declaration: bool,
    pub dynamic_imports: usize,
}
pub struct Project {
    pub config: Config,
    pub root: PathBuf,
    pub search: Vec<PathBuf>,
    pub units: BTreeMap<PathBuf, Unit>,
    pub names: BTreeMap<String, PathBuf>,
    pub issues: Vec<Issue>,
    /// Open editor buffers, keyed by canonical (or canonical-parent) paths.
    pub overlays: BTreeMap<PathBuf, Vec<u8>>,
    /// Sources actually used, including units with errors.
    pub sources: BTreeMap<PathBuf, Vec<u8>>,
    explicit: Option<PathBuf>,
    entry: Option<PathBuf>,
    cli_mode: Option<Mode>,
    visiting: Vec<PathBuf>,
    failed: BTreeSet<PathBuf>,
}
impl Project {
    pub fn new(
        entry: Option<PathBuf>,
        explicit: Option<&Path>,
        cli_mode: Option<Mode>,
    ) -> Result<Self, String> {
        let anchor = entry.clone().unwrap_or(
            std::env::current_dir()
                .map_err(|e| e.to_string())?
                .join("__project__.luax"),
        );
        let config = Config::load(&anchor, explicit)?;
        if entry.is_none() && config.file.is_none() {
            return Err("check without FILE requires a tynex.toml project (use --project)".into());
        }
        if entry.is_none() && cli_mode.is_some() {
            return Err("--mode applies only to an explicit entry FILE; use project overrides for a project check".into());
        }
        let root = config
            .root
            .clone()
            .unwrap_or_else(|| anchor.parent().unwrap().to_path_buf());
        let search = config
            .module_paths
            .iter()
            .map(|p| {
                let path = root.join(p);
                let path = pathutil::canonicalize(&path)
                    .map_err(|e| format!("module_paths {}: {e}", path.display()))?;
                if !path.is_dir() {
                    return Err(format!(
                        "module_paths {} is not a directory",
                        path.display()
                    ));
                }
                Ok(path)
            })
            .collect::<Result<Vec<_>, String>>()?;
        let explicit = explicit
            .map(pathutil::canonicalize)
            .transpose()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            config,
            root,
            search,
            units: BTreeMap::new(),
            names: BTreeMap::new(),
            issues: vec![],
            overlays: BTreeMap::new(),
            sources: BTreeMap::new(),
            explicit,
            entry,
            cli_mode,
            visiting: vec![],
            failed: BTreeSet::new(),
        })
    }
    pub fn build(&mut self) -> Result<(), String> {
        let roots = if let Some(entry) = &self.entry {
            vec![entry.clone()]
        } else {
            self.files()?
        };
        if roots.is_empty() {
            return Err("project include/exclude patterns selected no source files".into());
        }
        for name in self.config.dynamic_modules.clone() {
            match self.resolve(&name) {
                Ok(Some(path)) => {
                    self.names.insert(name, path.clone());
                    self.visit(&path)?;
                }
                Ok(None) => {}
                Err(message) => self.issue(
                    self.config.file.clone().unwrap_or(self.root.clone()),
                    "project",
                    "M101",
                    message,
                    (1, 1),
                ),
            }
        }
        for path in roots {
            self.visit(&path)?;
        }
        Ok(())
    }
    fn files(&self) -> Result<Vec<PathBuf>, String> {
        let mut files = vec![];
        self.walk(&self.root, &mut files)?;
        files.sort();
        files.dedup();
        Ok(files)
    }
    fn walk(&self, directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in fs::read_dir(directory).map_err(|e| format!("{}: {e}", directory.display()))? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let relative = path.strip_prefix(&self.root).unwrap();
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_symlink() {
                continue;
            }
            if self.config.exclude.is_match(relative) {
                continue;
            }
            if kind.is_dir() {
                if !self.config.exclude.is_match(relative.join("__directory__")) {
                    self.walk(&path, files)?;
                }
            } else if kind.is_file()
                && self.config.include.is_match(relative)
                && matches!(
                    path.extension().and_then(|x| x.to_str()),
                    Some("lua" | "luax")
                )
            {
                files.push(pathutil::canonicalize(&path).map_err(|e| e.to_string())?);
                if files.len() > 4096 {
                    return Err(
                        "project scan exceeds 4096 source files; narrow include/exclude".into(),
                    );
                }
            }
        }
        Ok(())
    }
    fn resolve(&self, name: &str) -> Result<Option<PathBuf>, String> {
        config::validate_module_name(name)?;
        let stem = name.replace('.', "/");
        for root in &self.search {
            let candidates = [
                format!("{stem}.luax"),
                format!("{stem}/init.luax"),
                format!("{stem}.lua"),
                format!("{stem}/init.lua"),
            ];
            let candidates: Vec<_> = candidates
                .iter()
                .map(|s| root.join(s))
                .filter(|p| self.exists(p))
                .collect();
            if candidates.len() > 1 {
                return Err(format!(
                    "ambiguous module '{name}': {}",
                    candidates
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if let Some(path) = candidates.first() {
                let path = pathutil::source_path(path).map_err(|e| e.to_string())?;
                if is_declaration(&path) {
                    return Err(format!(
                        "module '{name}' resolves to a declaration without an implementation"
                    ));
                }
                return Ok(Some(path));
            }
        }
        if self.config.external.is_match(name) {
            Ok(None)
        } else {
            Err(format!(
                "module '{name}' not found in module_paths; declare host/C/preloaded modules in external_modules"
            ))
        }
    }
    fn visit(&mut self, path: &Path) -> Result<Option<Interface>, String> {
        if let Some(unit) = self.units.get(path) {
            return Ok(Some(unit.interface.clone()));
        }
        if self.failed.contains(path) {
            return Ok(None);
        }
        if self.visiting.len() >= 128 {
            return Err("module dependency depth exceeds 128".into());
        }
        if self.units.len() + self.failed.len() >= 4096 {
            return Err("module graph exceeds 4096 files".into());
        }
        let source = match self.overlays.get(path) {
            Some(source) => source.clone(),
            None => fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?,
        };
        self.sources.insert(path.into(), source.clone());
        let declaration_file = is_declaration(path);
        let configuration = Config::load(path, self.explicit.as_deref())?;
        let mode = if declaration_file {
            Mode::Strict
        } else {
            configuration.mode(
                path,
                &source,
                if self.entry.as_deref() == Some(path) {
                    self.cli_mode
                } else {
                    None
                },
            )?
        };
        self.visiting.push(path.to_path_buf());
        let before = self.issues.len();
        let mut interface = Interface::unknown();
        let mut lua = None;
        let mut source_map = None;
        let mut dynamic_imports = 0;
        if declaration_file {
            let text =
                std::str::from_utf8(&source).map_err(|e| format!("{}: {e}", path.display()))?;
            match declaration(text) {
                Ok(value) => interface = value,
                Err(errors) => self.diagnostics(path, mode, text, errors),
            }
        } else if mode != Mode::Legacy {
            let text =
                std::str::from_utf8(&source).map_err(|e| format!("{}: {e}", path.display()))?;
            match prepare(text, path.extension().is_some_and(|s| s == "luax")) {
                Err(errors) => self.diagnostics(path, mode, text, errors),
                Ok(prepared) => {
                    let mut imports = BTreeMap::new();
                    let mut processed = BTreeSet::new();
                    for round in 0..128 {
                        let found = prepared.imports_with(mode, &imports);
                        dynamic_imports = found.iter().filter(|i| i.name.is_none()).count();
                        let mut found = found;
                        found.sort_by_key(|i| !i.static_import);
                        let pending: Vec<_> = found
                            .into_iter()
                            .filter(|i| i.name.as_ref().is_some_and(|n| !processed.contains(n)))
                            .collect();
                        if pending.is_empty() {
                            break;
                        }
                        if round == 127 {
                            self.issue(
                                path.into(),
                                &mode.to_string(),
                                "M109",
                                "module discovery exceeds 128 passes".into(),
                                (1, 1),
                            );
                            break;
                        }
                        for import in pending {
                            let name = import.name.unwrap();
                            processed.insert(name.clone());
                            let location = Diagnostic {
                                code: "M101",
                                message: String::new(),
                                span: import.span,
                            }
                            .location(text);
                            match self.resolve(&name) {
                                Err(message) => self.issue(
                                    path.into(),
                                    &mode.to_string(),
                                    "M101",
                                    message,
                                    location,
                                ),
                                Ok(None) => {
                                    if import.static_import {
                                        self.issue(path.into(), &mode.to_string(), "M110",
                                            format!("static import '{name}' requires a project implementation; use require for host/C modules"), location);
                                    }
                                    imports.insert(name, Interface::unknown());
                                }
                                Ok(Some(dependency)) => {
                                    self.names.insert(name.clone(), dependency.clone());
                                    if let Some(start) =
                                        self.visiting.iter().position(|p| p == &dependency)
                                    {
                                        let chain = self.visiting[start..]
                                            .iter()
                                            .chain(std::iter::once(&dependency))
                                            .map(|p| p.display().to_string())
                                            .collect::<Vec<_>>()
                                            .join(" -> ");
                                        self.issue(
                                            path.into(),
                                            &mode.to_string(),
                                            "M103",
                                            format!("cyclic module dependency: {chain}"),
                                            location,
                                        );
                                    } else if let Some(value) = self.visit(&dependency)? {
                                        imports.insert(name, value);
                                    }
                                }
                            }
                        }
                        if self.issues.len() != before {
                            break;
                        }
                    }
                    if self.issues.len() == before {
                        match prepared.compile(mode, &imports) {
                            Ok(output) => {
                                source_map = Some(output.source_map);
                                lua = Some(output.lua);
                                interface = output.interface;
                            }
                            Err(errors) => self.diagnostics(path, mode, text, errors),
                        }
                    }
                }
            }
        }
        // Declarations describe opaque legacy exports; they never execute or
        // pretend that the legacy implementation has been statically verified.
        if mode == Mode::Legacy && !declaration_file {
            let stub = path.with_extension("d.luax");
            if self.exists(&stub) {
                let stub = pathutil::source_path(&stub).map_err(|e| e.to_string())?;
                if let Some(value) = self.visit(&stub)? {
                    interface = value;
                }
            }
        } else if !declaration_file && self.exists(&path.with_extension("d.luax")) {
            self.issue(path.into(), &mode.to_string(), "M104", "sidecar declarations are only supported for legacy .lua implementations; typed exports are inferred".into(), (1,1));
        }
        self.visiting.pop();
        if self.issues.len() != before {
            self.failed.insert(path.to_path_buf());
            return Ok(None);
        }
        self.units.insert(
            path.into(),
            Unit {
                source,
                mode,
                lua,
                source_map,
                interface: interface.clone(),
                declaration: declaration_file,
                dynamic_imports,
            },
        );
        Ok(Some(interface))
    }
    fn exists(&self, path: &Path) -> bool {
        path.is_file() || pathutil::source_path(path).is_ok_and(|p| self.overlays.contains_key(&p))
    }
    fn diagnostics(&mut self, path: &Path, mode: Mode, text: &str, errors: Vec<Diagnostic>) {
        for error in errors {
            let location = error.location(text);
            self.issue(
                path.into(),
                &mode.to_string(),
                error.code,
                error.message,
                location,
            );
            self.issues.last_mut().unwrap().span = Some(error.span);
        }
    }
    pub fn issue(
        &mut self,
        file: PathBuf,
        mode: &str,
        code: &str,
        message: String,
        (line, column): (usize, usize),
    ) {
        self.issues.push(Issue {
            span: None,
            code: code.into(),
            severity: "error",
            file,
            line,
            column,
            mode: mode.into(),
            message,
        });
    }
}
pub fn is_declaration(path: &Path) -> bool {
    path.file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.ends_with(".d.luax"))
}
