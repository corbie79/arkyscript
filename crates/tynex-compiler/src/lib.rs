//! Independent compiler core. No filesystem, Teal, Lua VM, or native dependency.
mod ast;
mod checker;
mod lexer;
mod parser;
pub mod source_map;

use std::{collections::BTreeMap, ops::Range};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Legacy,
    Gradual,
    Strict,
}

impl std::str::FromStr for Mode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "legacy" => Ok(Self::Legacy),
            "gradual" => Ok(Self::Gradual),
            "strict" => Ok(Self::Strict),
            _ => Err(format!(
                "unknown type mode '{s}'; expected legacy, gradual, or strict"
            )),
        }
    }
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Legacy => "legacy",
            Self::Gradual => "gradual",
            Self::Strict => "strict",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub message: String,
    pub span: Range<usize>,
}

impl Diagnostic {
    pub(crate) fn new(code: &'static str, message: impl Into<String>, span: Range<usize>) -> Self {
        Self {
            code,
            message: message.into(),
            span,
        }
    }

    /// One-based Unicode scalar columns. Spans use UTF-8 bytes; source maps use UTF-16 columns.
    pub fn location(&self, source: &str) -> (usize, usize) {
        let before = &source[..self.span.start.min(source.len())];
        let line = before.bytes().filter(|b| *b == b'\n').count() + 1;
        let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
        (line, column)
    }
}

/// Public module value, inferred from the first return value or a declaration.
#[derive(Clone, Debug)]
pub struct Interface {
    pub(crate) value: ast::Type,
    pub(crate) nominals: BTreeMap<u64, ast::NominalType>,
}
impl Interface {
    pub fn unknown() -> Self {
        Self {
            value: ast::Type::Unknown,
            nominals: BTreeMap::new(),
        }
    }
}
impl std::fmt::Display for Interface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.value.fmt(f)
    }
}
#[derive(Clone, Debug)]
pub struct Import {
    pub static_import: bool,
    pub name: Option<String>,
    pub span: Range<usize>,
}

pub struct Prepared {
    source: String,
    body: Vec<ast::Stmt>,
    erased_spans: Vec<Range<usize>>,
    edits: Vec<parser::Edit>,
    imports: bool,
    exceptions: bool,
    classes: bool,
    async_runtime: bool,
}
/// Parse once; discovery never executes code and does not suppress final errors.
pub fn prepare(source: &str, typed_syntax: bool) -> Result<Prepared, Vec<Diagnostic>> {
    let tokens = lexer::lex(source).map_err(|e| vec![e])?;
    let parsed = parser::parse(tokens, typed_syntax).map_err(|e| vec![e])?;
    let mut spans = parsed.erased;
    // A function type contains its own return annotation. Merge these nested
    // ranges before feeding the ordered source-map emitter.
    spans.sort_by_key(|span| span.start);
    let mut erased_spans: Vec<Range<usize>> = Vec::new();
    for span in spans {
        if let Some(previous) = erased_spans.last_mut()
            && span.start <= previous.end
        {
            previous.end = previous.end.max(span.end);
        } else {
            erased_spans.push(span);
        }
    }
    Ok(Prepared {
        source: source.to_owned(),
        body: parsed.body,
        erased_spans,
        edits: parsed.edits,
        imports: parsed.imports,
        exceptions: parsed.exceptions,
        classes: parsed.classes,
        async_runtime: parsed.async_runtime,
    })
}
pub fn declaration(source: &str) -> Result<Interface, Vec<Diagnostic>> {
    let tokens = lexer::lex(source).map_err(|e| vec![e])?;
    if tokens
        .windows(2)
        .any(|t| t[0].text == "declare" && t[1].text == "class")
    {
        let body = parser::native_declaration(tokens).map_err(|e| vec![e])?;
        let result = checker::check(&body, Mode::Strict, &BTreeMap::new());
        return if result.errors.is_empty() {
            Ok(result.interface)
        } else {
            Err(result.errors)
        };
    }
    parser::declaration(tokens)
        .map(|value| Interface {
            value,
            nominals: BTreeMap::new(),
        })
        .map_err(|e| vec![e])
}
impl Prepared {
    pub fn imports(&self, mode: Mode) -> Vec<Import> {
        self.imports_with(mode, &BTreeMap::new())
    }
    pub fn imports_with(&self, mode: Mode, modules: &BTreeMap<String, Interface>) -> Vec<Import> {
        checker::check(&self.body, mode, modules).imports
    }
    pub fn compile(
        &self,
        mode: Mode,
        modules: &BTreeMap<String, Interface>,
    ) -> Result<Output, Vec<Diagnostic>> {
        if mode == Mode::Legacy {
            return Err(vec![Diagnostic::new(
                "M001",
                "legacy source must use the original LuaJIT loader",
                0..0,
            )]);
        }
        let analysis = checker::check(&self.body, mode, modules);
        if !analysis.errors.is_empty() {
            return Err(analysis.errors);
        }
        let mut lua = self.source.as_bytes().to_vec();
        for span in &self.erased_spans {
            for byte in &mut lua[span.clone()] {
                if *byte != b'\n' && *byte != b'\r' {
                    *byte = b' ';
                }
            }
        }
        if lua.starts_with(b"#!") {
            lua[..2].copy_from_slice(b"--");
        }
        let lua = String::from_utf8(lua).expect("complete-token erasure preserves UTF-8");
        let mut emitter = source_map::Emitter::default();
        if self.imports {
            // The runner supplies a graph-bound loader. Standalone generated Lua
            // must be embedded with the same explicit loader contract.
            emitter.helper("local __tynex_import = assert(__tynex_import, 'static import requires the Tynex project loader'); ");
        }
        if self.exceptions {
            emitter.helper("local __tynex_pcall, __tynex_unpack, __tynex_select = pcall, unpack, select; local function __tynex_pack(...) return {n=__tynex_select('#', ...), ...} end; ");
        }
        if self.classes {
            // Direct Lua execution also uses original source line numbers.
            // Keep the helper on the first line, just like import/try helpers.
            emitter.helper(&CLASS_RUNTIME.replace('\n', " "));
            emitter.helper("; ");
        }
        if self.async_runtime {
            emitter.helper("local __tynex_async_runtime = assert(__tynex_async_runtime, 'async requires the Tynex async runtime'); local function __tynex_async_values(...) return ... end; ");
        }
        let mut edits: Vec<_> = self
            .edits
            .iter()
            .map(|e| (e.span.clone(), e.text.as_str()))
            .collect();
        for span in &self.erased_spans {
            edits.push((span.clone(), &lua[span.clone()]));
        }
        // Keep this sort stable: the parser adds nested closing insertions
        // before enclosing ones at the same byte (for example an optional
        // chain followed by its class field default's generated `end`).
        edits.sort_by_key(|(span, _)| (span.start, span.end));
        let mut position = source_map::Position::default();
        let mut cursor = 0;
        for (span, replacement) in edits {
            emitter.copied(&lua[cursor..span.start], position);
            position.advance(&self.source[cursor..span.start]);
            emitter.lowered(replacement, position);
            position.advance(&self.source[span.clone()]);
            cursor = span.end;
        }
        emitter.copied(&lua[cursor..], position);
        let lua = emitter.code;
        Ok(Output {
            lua,
            source_map: emitter.map,
            erased_spans: self.erased_spans.clone(),
            interface: analysis.interface,
            imports: analysis.imports,
        })
    }
}

// Capturing the builtins before executing user code keeps lowered constructors
// independent of later local declarations or global reassignment. Helper code
// stays unmapped; field defaults and methods retain their original source spans.
const CLASS_RUNTIME: &str = r#"local __tynex_class, __tynex_impl = (function(type, next, rawget, setmetatable, error, select, getmeta)
local function create(name, traits, build)
    local fields, defaults, methods, allowed = {}, {}, {}, {}
    build(fields, defaults, methods)
    for i = 1, #fields do allowed[fields[i][1]] = true end
    local explicit = {}
    for key in next, methods do explicit[key] = true end
    for i = 1, #traits do
        for key, value in next, traits[i] do
            if not explicit[key] then
                if methods[key] ~= nil then error('conflicting trait default in ' .. name .. ': ' .. key, 2) end
                methods[key] = value
            end
        end
    end
    local instance_meta = {__index = methods}
    local implemented, extensions = {}, {}
    for key in next, methods do if not explicit[key] then extensions[key] = true end end
    for i = 1, #traits do implemented[traits[i]] = true end
    local function implement(trait, explicit, defaults)
        if implemented[trait] then error('duplicate trait implementation for ' .. name, 2) end
        for key in next, explicit do
            if allowed[key] or methods[key] ~= nil then error('impl cannot replace member ' .. key, 2) end
        end
        for key in next, defaults do
            if allowed[key] then error('trait default conflicts with field ' .. key, 2) end
            if extensions[key] then error('conflicting trait default ' .. key, 2) end
        end
        for key, value in next, defaults do
            if methods[key] == nil then methods[key] = value; extensions[key] = true end
        end
        for key, value in next, explicit do methods[key] = value; extensions[key] = true end
        implemented[trait] = true
    end
    return setmetatable({}, {__arky_impl = implement, __call = function(_, initial, ...)
        if type(initial) ~= 'table' or select('#', ...) ~= 0 then
            error(name .. ' constructor expects exactly one table', 2)
        end
        for key in next, initial do
            if not allowed[key] then error('unknown initializer field in ' .. name, 2) end
        end
        local instance = {}
        for i = 1, #fields do
            local field = fields[i]
            local key = field[1]
            local value = rawget(initial, key)
            if value == nil then
                local default = defaults[key]
                if default then value = default()
                elseif not field[2] then error('missing required field ' .. name .. '.' .. key, 2) end
            end
            instance[key] = value
        end
        return setmetatable(instance, instance_meta)
    end})
end
return create, function(trait, target, build)
    local meta = getmeta(target)
    local register = meta and rawget(meta, '__arky_impl')
    if type(register) ~= 'function' then error('impl target does not support Arky extensions', 2) end
    local methods = {}
    build(methods)
    register(trait, methods, trait)
end
end)(type, next, rawget, setmetatable, error, select, (debug and debug.getmetatable) or getmetatable)
"#;

#[derive(Debug)]
pub struct Output {
    pub lua: String,
    pub source_map: source_map::SourceMap,
    /// Ranges removed by the current annotation erasure pass.
    pub erased_spans: Vec<Range<usize>>,
    pub interface: Interface,
    pub imports: Vec<Import>,
}

/// Compile one file without a module resolver. Imports remain dynamic boundaries.
pub fn compile(source: &str, mode: Mode, typed_syntax: bool) -> Result<Output, Vec<Diagnostic>> {
    if mode == Mode::Legacy {
        return Err(vec![Diagnostic::new(
            "M001",
            "legacy source must use the original LuaJIT loader",
            0..0,
        )]);
    }
    prepare(source, typed_syntax)?.compile(mode, &BTreeMap::new())
}
