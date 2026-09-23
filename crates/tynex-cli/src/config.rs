use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tynex_compiler::Mode;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    type_mode: Option<String>,
    #[serde(default)]
    overrides: Vec<Override>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    module_paths: Option<Vec<PathBuf>>,
    #[serde(default)]
    dynamic_modules: Vec<String>,
    #[serde(default)]
    external_modules: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Override {
    files: Vec<String>,
    mode: String,
}
pub struct Config {
    pub root: Option<PathBuf>,
    pub file: Option<PathBuf>,
    pub module_paths: Vec<PathBuf>,
    pub dynamic_modules: Vec<String>,
    pub include: GlobSet,
    pub exclude: GlobSet,
    pub external: GlobSet,
    default: Option<Mode>,
    overrides: Vec<(GlobSet, Mode)>,
}

impl Config {
    pub fn load(entry: &Path, explicit: Option<&Path>) -> Result<Self, String> {
        let file = match explicit {
            Some(p) => Some(
                crate::pathutil::canonicalize(p).map_err(|e| format!("{}: {e}", p.display()))?,
            ),
            None => entry
                .parent()
                .into_iter()
                .flat_map(Path::ancestors)
                .map(|d| d.join("tynex.toml"))
                .find(|p| p.is_file()),
        };
        let Some(file) = file else {
            return Self::from_data(None, FileConfig::default());
        };
        let source = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let data: FileConfig =
            toml::from_str(&source).map_err(|e| format!("{}: {e}", file.display()))?;
        Self::from_data(Some(file), data)
    }
    fn from_data(file: Option<PathBuf>, data: FileConfig) -> Result<Self, String> {
        let default = data.type_mode.map(|s| s.parse()).transpose()?;
        let mut overrides = Vec::new();
        for rule in data.overrides {
            if rule.files.is_empty() {
                return Err("override.files must not be empty".into());
            }
            let mut builder = GlobSetBuilder::new();
            for pattern in rule.files {
                builder
                    .add(Glob::new(&pattern).map_err(|e| format!("invalid override glob: {e}"))?);
            }
            overrides.push((
                builder.build().map_err(|e| e.to_string())?,
                rule.mode.parse()?,
            ));
        }
        let include = data
            .include
            .unwrap_or_else(|| vec!["**/*.arky".into(), "**/*.lua".into()]);
        if include.is_empty() {
            return Err("include must not be empty".into());
        }
        let exclude = data.exclude.unwrap_or_else(|| {
            [".git/**", ".deps/**", "build/**", "target/**", "archive/**"]
                .map(str::to_owned)
                .to_vec()
        });
        let module_paths = data
            .module_paths
            .unwrap_or_else(|| vec![PathBuf::from(".")]);
        if module_paths.is_empty() {
            return Err("module_paths must not be empty".into());
        }
        for name in &data.dynamic_modules {
            validate_module_name(name)?;
        }
        let mut external: Vec<String> = [
            "bit",
            "ffi",
            "jit",
            "jit.*",
            "table",
            "string",
            "math",
            "coroutine",
            "os",
            "io",
            "package",
            "debug",
            "_G",
        ]
        .map(str::to_owned)
        .to_vec();
        external.extend(data.external_modules);
        Ok(Self {
            root: file
                .as_ref()
                .and_then(|f| f.parent().map(Path::to_path_buf)),
            file,
            default,
            overrides,
            module_paths,
            dynamic_modules: data.dynamic_modules,
            include: patterns(&include)?,
            exclude: patterns(&exclude)?,
            external: patterns(&external)?,
        })
    }

    pub fn mode(&self, path: &Path, source: &[u8], cli: Option<Mode>) -> Result<Mode, String> {
        let extension = path.extension().and_then(|s| s.to_str());
        let mut mode = match extension {
            Some("lua") => Mode::Legacy,
            Some("arky") => Mode::Gradual,
            Some("tl") => return Err("Teal .tl support has been retired. Migrate explicitly to the documented .arky subset; renaming alone does not convert Teal syntax.".into()),
            _ => return Err("expected a .lua or .arky source file".into()),
        };
        if let Some(relative) = self.root.as_ref().and_then(|r| path.strip_prefix(r).ok()) {
            mode = self.default.unwrap_or(mode);
            for (glob, value) in &self.overrides {
                if glob.is_match(relative) {
                    mode = *value;
                }
            }
        }
        // Only leading line comments/blank lines are directives. Never scan strings.
        for (index, bytes) in source.split(|b| *b == b'\n').enumerate() {
            let Ok(line) = std::str::from_utf8(bytes) else {
                break;
            };
            let line = line.trim();
            if index == 0 && line.starts_with("#!") {
                continue;
            }
            if line.is_empty() {
                continue;
            }
            if let Some(value) = line.strip_prefix("--!typecheck") {
                mode = value.trim().parse()?;
            } else if !line.starts_with("--") || line.starts_with("--[") {
                break;
            }
        }
        mode = cli.unwrap_or(mode);
        if extension == Some("arky") && mode == Mode::Legacy {
            return Err("legacy mode requires .lua; use gradual for .arky".into());
        }
        Ok(mode)
    }
}

fn patterns(values: &[String]) -> Result<GlobSet, String> {
    let mut builder = GlobSetBuilder::new();
    for value in values {
        builder.add(Glob::new(value).map_err(|e| format!("invalid project glob: {e}"))?);
    }
    builder.build().map_err(|e| e.to_string())
}
pub fn validate_module_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || !name.split('.').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        })
    {
        return Err(format!(
            "invalid module name '{name}'; use dotted names without path separators"
        ));
    }
    Ok(())
}
