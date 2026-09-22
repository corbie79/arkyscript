use crate::{
    Diagnostic,
    ast::*,
    lexer::{Kind, Token, decode_string},
};
use std::ops::Range;

#[derive(Debug)]
pub(crate) struct Edit {
    pub span: Range<usize>,
    pub text: String,
}
pub(crate) struct Parsed {
    pub body: Vec<Stmt>,
    pub erased: Vec<Range<usize>>,
    pub edits: Vec<Edit>,
    pub imports: bool,
    pub exceptions: bool,
    pub classes: bool,
    pub async_runtime: bool,
}
struct Parser {
    tokens: Vec<Token>,
    index: usize,
    erased: Vec<Range<usize>>,
    typed: bool,
    declarations_only: bool,
    depth: usize,
    loops: usize,
    variadic: bool,
    protected: Vec<usize>,
    edits: Vec<Edit>,
    imports: bool,
    exceptions: usize,
    classes: bool,
    async_body: bool,
    async_runtime: bool,
}
pub fn parse(tokens: Vec<Token>, typed: bool) -> Result<Parsed, Diagnostic> {
    let mut p = Parser {
        tokens,
        index: 0,
        erased: Vec::new(),
        typed,
        declarations_only: false,
        depth: 0,
        loops: 0,
        variadic: true,
        protected: vec![],
        edits: vec![],
        imports: false,
        exceptions: 0,
        classes: false,
        async_body: false,
        async_runtime: false,
    };
    let body = p.block(&[])?;
    Ok(Parsed {
        body,
        erased: p.erased,
        edits: p.edits,
        imports: p.imports,
        exceptions: p.exceptions > 0,
        classes: p.classes,
        async_runtime: p.async_runtime,
    })
}
pub fn declaration(tokens: Vec<Token>) -> Result<Type, Diagnostic> {
    let mut p = Parser {
        tokens,
        index: 0,
        erased: vec![],
        typed: true,
        declarations_only: true,
        depth: 0,
        loops: 0,
        variadic: false,
        protected: vec![],
        edits: vec![],
        imports: false,
        exceptions: 0,
        classes: false,
        async_body: false,
        async_runtime: false,
    };
    if p.take("export") {
        p.expect(":")?;
        let ty = p.ty()?;
        p.take(";");
        if p.tok().kind != Kind::Eof {
            return Err(p.fail("export: TYPE cannot be mixed with named declarations"));
        }
        return Ok(ty);
    }
    let mut table = TableType::default();
    while p.tok().kind != Kind::Eof {
        p.expect("declare")?;
        let binding = p.binding()?;
        let ty = binding
            .annotation
            .ok_or_else(|| p.fail("declare requires a type and no initializer"))?;
        if table.fields.insert(binding.name.into_bytes(), ty).is_some() {
            return Err(p.fail("duplicate declaration"));
        }
        p.take(";");
    }
    if table.fields.is_empty() {
        return Err(p.fail("expected export: TYPE or declare NAME: TYPE"));
    }
    Ok(Type::Table(table.into()))
}
impl Parser {
    fn tok(&self) -> &Token {
        &self.tokens[self.index]
    }
    fn at(&self, s: &str) -> bool {
        self.tok().text == s
    }
    fn take(&mut self, s: &str) -> bool {
        if self.at(s) {
            self.index += 1;
            true
        } else {
            false
        }
    }
    fn fail(&self, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new("P001", message, self.tok().span.clone())
    }
    fn expect(&mut self, s: &str) -> Result<(), Diagnostic> {
        if self.take(s) {
            Ok(())
        } else {
            Err(self.fail(format!("expected '{s}', found '{}'", self.tok().text)))
        }
    }
    fn enter(&mut self) -> Result<(), Diagnostic> {
        if self.depth >= 128 {
            return Err(self.fail("source nesting exceeds the current limit (128)"));
        }
        self.depth += 1;
        Ok(())
    }
    fn name(&mut self) -> Result<Token, Diagnostic> {
        const RESERVED: &[&str] = &[
            "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto",
            "if", "in", "local", "nil", "not", "or", "repeat", "return", "then", "true", "until",
            "while", "class", "trait", "enum", "match", "async", "await",
        ];
        if self.tok().kind != Kind::Word
            || RESERVED.contains(&self.tok().text.as_str())
            || (self.typed
                && (["import", "from", "declare", "try", "except"]
                    .contains(&self.tok().text.as_str())
                    || self.tok().text.starts_with("__tynex_")))
        {
            return Err(
                self.fail("expected an identifier (this keyword may not be implemented yet)")
            );
        }
        let t = self.tok().clone();
        self.index += 1;
        Ok(t)
    }
    fn annotation(&mut self) -> Result<Option<Type>, Diagnostic> {
        if !self.at(":") {
            return Ok(None);
        }
        if !self.typed {
            return Err(
                self.fail("type annotations require a .tlj file; .lua remains ordinary Lua")
            );
        }
        let start = self.tok().span.start;
        self.index += 1;
        let ty = self.ty()?;
        self.erased
            .push(start..self.tokens[self.index - 1].span.end);
        Ok(Some(ty))
    }
    fn ty(&mut self) -> Result<Type, Diagnostic> {
        self.enter()?;
        let mut ty = if self.take("(") {
            let ty = self.ty()?;
            self.expect(")")?;
            ty
        } else if self.take("function") {
            self.function_type(false)?
        } else if self.take("async") {
            self.expect("function")?;
            self.function_type(true)?
        } else if self.at("Task")
            && self
                .tokens
                .get(self.index + 1)
                .is_some_and(|token| token.text == "<")
        {
            self.index += 2;
            let mut results = Pack::default();
            if !self.at(">") && !self.at(">=") {
                loop {
                    if self.take("...") {
                        results.tail = Some(Box::new(self.ty()?));
                        break;
                    }
                    results.values.push(self.ty()?);
                    if !self.take(",") {
                        break;
                    }
                }
            }
            // A type annotation can be followed immediately by assignment.
            // Split >= only here, retaining the original byte spans for erasure.
            if self.at(">=") {
                let span = self.tok().span.clone();
                self.tokens[self.index] = Token {
                    kind: Kind::Symbol,
                    text: ">".into(),
                    span: span.start..span.start + 1,
                };
                self.tokens.insert(
                    self.index + 1,
                    Token {
                        kind: Kind::Symbol,
                        text: "=".into(),
                        span: span.start + 1..span.end,
                    },
                );
            }
            self.expect(">")?;
            results.minimum = results.values.len();
            Type::Task(Box::new(results))
        } else if self.take("{") {
            let mut table = TableType::default();
            if !self.at("}") {
                if self.take("[") {
                    let key = self.ty()?;
                    if matches!(key, Type::Nil | Type::Optional(_)) {
                        return Err(self.fail("table index types cannot include nil"));
                    }
                    self.expect("]")?;
                    self.expect(":")?;
                    let value = self.ty()?;
                    table.index = Some((Box::new(key), Box::new(value)));
                } else if self
                    .tokens
                    .get(self.index + 1)
                    .is_some_and(|t| t.text == ":")
                {
                    loop {
                        let field = self.name()?.text.into_bytes();
                        self.expect(":")?;
                        let value = self.ty()?;
                        if table.fields.insert(field, value).is_some() {
                            return Err(self.fail("duplicate field in table type"));
                        }
                        if !self.take(",") || self.at("}") {
                            break;
                        }
                    }
                } else {
                    table.index = Some((Box::new(Type::Number), Box::new(self.ty()?)));
                }
            }
            self.expect("}")?;
            Type::Table(table.into())
        } else {
            let ty = match self.tok().text.as_str() {
                "number" => Type::Number,
                "string" => Type::String,
                "boolean" => Type::Boolean,
                "Socket" => Type::Socket,
                "nil" => Type::Nil,
                "any" => Type::Any,
                _ => {
                    if self.declarations_only {
                        return Err(self.fail(
                            "named types in declaration files require a class or trait definition; use a structural contract",
                        ));
                    }
                    let mut name = self.name()?.text;
                    while self.take(".") {
                        name.push('.');
                        name.push_str(&self.name()?.text);
                    }
                    if self.take("?") {
                        self.depth -= 1;
                        return Ok(Type::Named(name).optional());
                    }
                    self.depth -= 1;
                    return Ok(Type::Named(name));
                }
            };
            self.index += 1;
            ty
        };
        if self.take("?") {
            ty = ty.optional();
        }
        self.depth -= 1;
        Ok(ty)
    }
    fn function_type(&mut self, is_async: bool) -> Result<Type, Diagnostic> {
        self.expect("(")?;
        let mut params = vec![];
        let mut tail = None;
        if !self.at(")") {
            loop {
                if self.take("...") {
                    tail = Some(Box::new(self.ty()?));
                    break;
                }
                params.push(self.ty()?);
                if !self.take(",") {
                    break;
                }
            }
        }
        self.expect(")")?;
        let returns = self
            .return_annotation()?
            .ok_or_else(|| self.fail("function types require a return annotation"))?;
        let returns = if is_async {
            Pack::one(Type::Task(Box::new(returns)))
        } else {
            returns
        };
        Ok(Type::Function(params, Box::new(returns), tail))
    }
    fn binding(&mut self) -> Result<Binding, Diagnostic> {
        let t = self.name()?;
        Ok(Binding {
            name: t.text,
            annotation: self.annotation()?,
            span: t.span,
        })
    }
    fn return_annotation(&mut self) -> Result<Option<Pack>, Diagnostic> {
        if !self.at(":") {
            return Ok(None);
        }
        if !self.typed {
            return Err(self.fail("type annotations require a .tlj file"));
        }
        let start = self.tok().span.start;
        self.index += 1;
        let mut pack = Pack::default();
        if self.take("(") {
            if !self.at(")") {
                loop {
                    if self.take("...") {
                        pack.tail = Some(Box::new(self.ty()?));
                        break;
                    }
                    pack.values.push(self.ty()?);
                    if !self.take(",") {
                        break;
                    }
                }
            }
            self.expect(")")?;
            if self.at("?") {
                if pack.values.len() != 1 || pack.tail.is_some() {
                    return Err(self.fail(
                        "only a single grouped return type can be optional; return packs cannot use '?'",
                    ));
                }
                self.index += 1;
                let value = pack.values.pop().unwrap();
                pack.values.push(value.optional());
            }
        } else {
            pack.values.push(self.ty()?);
        }
        pack.minimum = pack.values.len();
        self.erased
            .push(start..self.tokens[self.index - 1].span.end);
        Ok(Some(pack))
    }
    fn function(&mut self, start: usize, is_async: bool) -> Result<Function, Diagnostic> {
        self.expect("(")?;
        let mut params = Vec::new();
        let mut variadic = None;
        if !self.at(")") {
            loop {
                if self.take("...") {
                    let span = self.tokens[self.index - 1].span.clone();
                    variadic = Some(Binding {
                        name: "...".into(),
                        annotation: self.annotation()?,
                        span,
                    });
                    break;
                }
                params.push(self.binding()?);
                if !self.take(",") {
                    break;
                }
            }
        }
        self.expect(")")?;
        let returns = self.return_annotation()?;
        let previous_loops = self.loops;
        let previous_variadic = self.variadic;
        let previous_async = self.async_body;
        let previous_protected = std::mem::take(&mut self.protected);
        self.loops = 0;
        self.variadic = variadic.is_some();
        self.async_body = is_async;
        self.async_runtime |= is_async;
        let body = self.block(&["end"])?;
        self.expect("end")?;
        self.protected = previous_protected;
        self.loops = previous_loops;
        self.variadic = previous_variadic;
        self.async_body = previous_async;
        if is_async {
            let end = self.tokens[self.index - 1].span.end;
            self.edit(end..end, ")".into());
        }
        Ok(Function {
            is_async,
            params,
            returns,
            variadic,
            body,
            span: start..self.tokens[self.index - 1].span.end,
        })
    }
    fn named_function(&mut self, start: usize, is_async: bool) -> Result<Stmt, Diagnostic> {
        let function_token = self.tokens[self.index - 1].span.clone();
        let name = self.name()?;
        let mut target = Expr {
            kind: ExprKind::Name(name.text),
            span: name.span,
        };
        while self.take(".") {
            let name = self.name()?;
            let span = target.span.start..name.span.end;
            target = Expr {
                kind: ExprKind::Field(Box::new(target), name.text.into_bytes()),
                span,
            };
        }
        let method = self.take(":");
        if method {
            if is_async {
                self.edit(self.tokens[self.index - 1].span.clone(), ".".into());
            }
            let name = self.name()?;
            let span = target.span.start..name.span.end;
            target = Expr {
                kind: ExprKind::Field(Box::new(target), name.text.into_bytes()),
                span,
            };
        }
        let opening = self.tok().span.end;
        let function = self.function(start, is_async)?;
        if is_async {
            self.edit(function.span.end..function.span.end, ";".into());
            self.edit(function_token, String::new());
            self.edit(
                target.span.end..target.span.end,
                " = __tynex_async_runtime.create(function".into(),
            );
            if method {
                let receiver = if function.params.is_empty() && function.variadic.is_none() {
                    "self"
                } else {
                    "self, "
                };
                self.edit(opening..opening, receiver.into());
            }
        }
        Ok(Stmt::NamedFunction(target, function, method))
    }
    fn expressions(&mut self) -> Result<Vec<Expr>, Diagnostic> {
        let mut values = vec![self.expr(0)?];
        while self.take(",") {
            values.push(self.expr(0)?);
        }
        Ok(values)
    }
    fn arguments(&mut self) -> Result<Vec<Expr>, Diagnostic> {
        if self.take("(") {
            let values = if self.at(")") {
                vec![]
            } else {
                self.expressions()?
            };
            self.expect(")")?;
            Ok(values)
        } else if self.take("{") {
            let start = self.tokens[self.index - 1].span.start;
            let fields = self.table()?;
            Ok(vec![Expr {
                kind: ExprKind::Table(fields),
                span: start..self.tokens[self.index - 1].span.end,
            }])
        } else if self.tok().kind == Kind::String {
            let t = self.tok().clone();
            self.index += 1;
            Ok(vec![Expr {
                kind: ExprKind::String(decode_string(&t.text)),
                span: t.span,
            }])
        } else {
            Err(self.fail("expected method arguments"))
        }
    }
    fn loop_block(&mut self, end: &str) -> Result<Vec<Stmt>, Diagnostic> {
        self.loops += 1;
        let body = self.block(&[end])?;
        self.expect(end)?;
        self.loops -= 1;
        Ok(body)
    }
    fn block(&mut self, ends: &[&str]) -> Result<Vec<Stmt>, Diagnostic> {
        self.enter()?;
        let mut out = Vec::new();
        while self.tok().kind != Kind::Eof && !ends.contains(&self.tok().text.as_str()) {
            if self.take(";") {
                continue;
            }
            let start = self.tok().span.start;
            let stmt = if self.typed && self.take("import") {
                self.imports = true;
                self.edit(start..self.tokens[self.index - 1].span.end, "local".into());
                let binding = self.binding()?;
                let from = self.tok().span.clone();
                self.expect("from")?;
                self.edit(from, "= __tynex_import(".into());
                let token = self.tok().clone();
                if token.kind != Kind::String {
                    return Err(self.fail("import requires a literal module name"));
                }
                let name = String::from_utf8(decode_string(&token.text))
                    .map_err(|_| self.fail("import module names must be UTF-8"))?;
                self.index += 1;
                self.edit(token.span.end..token.span.end, ")".into());
                Stmt::Import(binding, name)
            } else if self.typed && self.take("try") {
                self.try_statement(start)?
            } else if self.typed && self.take("class") {
                self.class_statement(start)?
            } else if self.typed && self.take("trait") {
                self.trait_statement(start)?
            } else if self.take("local") {
                let is_async = self.typed && self.take("async");
                if is_async {
                    self.edit(self.tokens[self.index - 1].span.clone(), String::new());
                }
                if if is_async {
                    self.expect("function")?;
                    true
                } else {
                    self.take("function")
                } {
                    let function_token = self.tokens[self.index - 1].span.clone();
                    let t = self.name()?;
                    let name = Binding {
                        name: t.text,
                        annotation: None,
                        span: t.span,
                    };
                    if is_async {
                        self.edit(function_token, String::new());
                        self.edit(
                            name.span.clone(),
                            format!(
                                "{0}; {0} = __tynex_async_runtime.create(function",
                                name.name
                            ),
                        );
                    }
                    let function = self.function(start, is_async)?;
                    if is_async {
                        self.edit(function.span.end..function.span.end, ";".into());
                    }
                    Stmt::Function(name, function)
                } else {
                    let mut names = vec![self.binding()?];
                    while self.take(",") {
                        names.push(self.binding()?);
                    }
                    let values = if self.take("=") {
                        self.expressions()?
                    } else {
                        vec![]
                    };
                    Stmt::Local(names, values)
                }
            } else if self.take("function") {
                self.named_function(start, false)?
            } else if self.typed && self.take("async") {
                self.edit(self.tokens[self.index - 1].span.clone(), String::new());
                self.expect("function")?;
                self.named_function(start, true)?
            } else if self.take("do") {
                let body = self.block(&["end"])?;
                self.expect("end")?;
                Stmt::Block(body)
            } else if self.take("if") {
                let mut branches = Vec::new();
                loop {
                    let condition = self.expr(0)?;
                    self.expect("then")?;
                    branches.push((condition, self.block(&["elseif", "else", "end"])?));
                    if !self.take("elseif") {
                        break;
                    }
                }
                let otherwise = if self.take("else") {
                    Some(self.block(&["end"])?)
                } else {
                    None
                };
                self.expect("end")?;
                Stmt::If(branches, otherwise)
            } else if self.take("while") {
                let condition = self.expr(0)?;
                self.expect("do")?;
                Stmt::While(condition, self.loop_block("end")?)
            } else if self.take("repeat") {
                let body = self.loop_block("until")?;
                let condition = self.expr(0)?;
                Stmt::Repeat(body, condition)
            } else if self.take("for") {
                let first = self.binding()?;
                if self.take("=") {
                    let from = self.expr(0)?;
                    self.expect(",")?;
                    let to = self.expr(0)?;
                    let step = if self.take(",") {
                        Some(self.expr(0)?)
                    } else {
                        None
                    };
                    self.expect("do")?;
                    Stmt::NumericFor(Box::new(NumericFor {
                        binding: first,
                        from,
                        to,
                        step,
                        body: self.loop_block("end")?,
                    }))
                } else {
                    let mut names = vec![first];
                    while self.take(",") {
                        names.push(self.binding()?);
                    }
                    self.expect("in")?;
                    let iterator = self.expr(0)?;
                    self.expect("do")?;
                    Stmt::GenericFor(names, iterator, self.loop_block("end")?)
                }
            } else if self.take("break") {
                if self.loops == 0 {
                    return Err(
                        self.fail("break is only allowed inside the current function's loop")
                    );
                }
                if self
                    .protected
                    .last()
                    .is_some_and(|depth| self.loops <= *depth)
                {
                    self.edit(
                        start..self.tokens[self.index - 1].span.end,
                        "return 2".into(),
                    );
                }
                Stmt::Break
            } else if self.take("return") {
                let span = start..self.tokens[self.index - 1].span.end;
                let value = if self.tok().kind == Kind::Eof
                    || self.at(";")
                    || ends.contains(&self.tok().text.as_str())
                {
                    vec![]
                } else {
                    self.expressions()?
                };
                if !self.protected.is_empty() {
                    self.edit(
                        span.clone(),
                        if value.is_empty() {
                            "return 1"
                        } else {
                            "return 1,"
                        }
                        .into(),
                    );
                }
                Stmt::Return(value, span)
            } else {
                let target = self.expr(0)?;
                if self.at("=") || self.at(",") {
                    let mut targets = vec![target];
                    while self.take(",") {
                        targets.push(self.expr(0)?);
                    }
                    self.expect("=")?;
                    if targets.iter().any(|t| {
                        !matches!(
                            t.kind,
                            ExprKind::Name(_) | ExprKind::Field(..) | ExprKind::Index(..)
                        )
                    }) {
                        return Err(self.fail("expected an assignable name or table field"));
                    }
                    Stmt::Assign(targets, self.expressions()?)
                } else if matches!(
                    target.kind,
                    ExprKind::Call(..) | ExprKind::MethodCall(..) | ExprKind::Await(_)
                ) || matches!(&target.kind, ExprKind::OptionalChain(_, steps) if steps.last().is_some_and(|step| matches!(step.kind, ChainStepKind::Call(_) | ChainStepKind::MethodCall(..))))
                {
                    Stmt::Call(target)
                } else {
                    return Err(self.fail("expected an assignment or function call"));
                }
            };
            let terminal = matches!(stmt, Stmt::Return(..) | Stmt::Break);
            out.push(stmt);
            if terminal {
                self.take(";");
                if self.tok().kind != Kind::Eof && !ends.contains(&self.tok().text.as_str()) {
                    return Err(
                        self.fail("return or break must be the last statement in this block")
                    );
                }
            }
        }
        self.depth -= 1;
        Ok(out)
    }
    fn edit(&mut self, span: Range<usize>, text: String) {
        self.edits.push(Edit { span, text });
    }
    fn declaration_name(&mut self) -> Result<Binding, Diagnostic> {
        let token = self.name()?;
        Ok(Binding {
            name: token.text,
            annotation: None,
            span: token.span,
        })
    }
    fn method(&mut self, start: usize, is_async: bool) -> Result<Method, Diagnostic> {
        let function_token = self.tokens[self.index - 1].span.clone();
        let name = self.declaration_name()?;
        let opening = self.tok().span.end;
        let function = self.function(start, is_async)?;
        if is_async {
            self.edit(function.span.end..function.span.end, ";".into());
        }
        if let Some(parameter) = function.params.iter().find(|p| p.name == "self") {
            return Err(Diagnostic::new(
                "P001",
                "class and trait methods receive self implicitly",
                parameter.span.clone(),
            ));
        }
        let method_name = if is_async {
            self.edit(function_token, String::new());
            format!(
                "__tynex_methods.{} = __tynex_async_runtime.create(function",
                name.name
            )
        } else {
            format!("__tynex_methods.{}", name.name)
        };
        self.edit(name.span.clone(), method_name);
        self.edit(
            opening..opening,
            if function.params.is_empty() && function.variadic.is_none() {
                "self"
            } else {
                "self, "
            }
            .into(),
        );
        Ok(Method { name, function })
    }
    fn class_statement(&mut self, start: usize) -> Result<Stmt, Diagnostic> {
        self.classes = true;
        self.edit(start..self.tokens[self.index - 1].span.end, "local".into());
        let name = self.declaration_name()?;
        let mut traits = vec![];
        if self.take("is") {
            self.edit(self.tokens[self.index - 1].span.clone(), String::new());
            loop {
                let mut binding = self.declaration_name()?;
                while self.take(".") {
                    let part = self.name()?;
                    binding.name.push('.');
                    binding.name.push_str(&part.text);
                    binding.span.end = part.span.end;
                }
                traits.push(binding);
                if !self.take(",") {
                    break;
                }
            }
        }
        let header_end = self.tokens[self.index - 1].span.end;
        let builder = "function(__tynex_fields, __tynex_defaults, __tynex_methods) ";
        if traits.is_empty() {
            self.edit(
                name.span.clone(),
                format!("{0}; {0} = __tynex_class({0:?}, {{", name.name),
            );
            self.edit(header_end..header_end, format!("}}, {builder}"));
        } else {
            // A local's initializer still sees the outer binding of its name.
            // Capture the trait list there before the class shadows that name,
            // then reuse the local for the constructor. Method/default closures
            // capture this new binding; neither body executes while building it.
            self.edit(name.span.clone(), format!("{} = {{", name.name));
            self.edit(
                header_end..header_end,
                format!("}}; {0} = __tynex_class({0:?}, {0}, {builder}", name.name),
            );
        }
        let mut fields = vec![];
        let mut methods = vec![];
        while !self.at("end") && self.tok().kind != Kind::Eof {
            if self.take(";") {
                continue;
            }
            let member_start = self.tok().span.start;
            let is_async = self.take("async");
            if is_async {
                self.edit(self.tokens[self.index - 1].span.clone(), String::new());
                self.expect("function")?;
            }
            if is_async || self.take("function") {
                methods.push(self.method(member_start, is_async)?);
            } else {
                let binding = self.binding()?;
                let nullable = matches!(
                    binding.annotation,
                    None | Some(Type::Any | Type::Unknown | Type::Nil | Type::Optional(_))
                );
                self.edit(
                    binding.span.clone(),
                    format!(
                        "__tynex_fields[#__tynex_fields + 1] = {{{:?}, {nullable}}};",
                        binding.name
                    ),
                );
                let default = if self.take("=") {
                    let equals = self.tokens[self.index - 1].span.clone();
                    self.edit(
                        equals,
                        format!("__tynex_defaults[{:?}] = function() return ", binding.name),
                    );
                    // A default is evaluated once per construction, inside a new
                    // zero-argument closure. It captures lexical locals but not
                    // the containing function's varargs or protected return tag.
                    let previous_variadic = self.variadic;
                    let previous_async = self.async_body;
                    let previous_protected = std::mem::take(&mut self.protected);
                    self.variadic = false;
                    self.async_body = false;
                    let value = self.expr(0)?;
                    self.variadic = previous_variadic;
                    self.async_body = previous_async;
                    self.protected = previous_protected;
                    self.edit(value.span.end..value.span.end, " end;".into());
                    Some(value)
                } else {
                    None
                };
                fields.push(ClassField { binding, default });
            }
        }
        let end = self.tok().span.clone();
        self.expect("end")?;
        self.edit(end, "end);".into());
        Ok(Stmt::Class(ClassDecl {
            name,
            traits,
            fields,
            methods,
        }))
    }
    fn trait_statement(&mut self, start: usize) -> Result<Stmt, Diagnostic> {
        self.edit(start..self.tokens[self.index - 1].span.end, "local".into());
        let name = self.declaration_name()?;
        self.edit(
            name.span.clone(),
            format!("{0} = {{}}; do local __tynex_methods = {0};", name.name),
        );
        let mut required = vec![];
        let mut methods = vec![];
        while !self.at("end") && self.tok().kind != Kind::Eof {
            if self.take(";") {
                continue;
            }
            let member_start = self.tok().span.start;
            let is_async = self.take("async");
            if is_async {
                self.edit(self.tokens[self.index - 1].span.clone(), String::new());
                self.expect("function")?;
            }
            if is_async || self.take("function") {
                methods.push(self.method(member_start, is_async)?);
            } else {
                let binding = self.binding()?;
                if !matches!(binding.annotation, Some(Type::Function(..))) {
                    return Err(Diagnostic::new(
                        "P001",
                        "trait requirements must have a function type",
                        binding.span,
                    ));
                }
                self.edit(binding.span.clone(), String::new());
                required.push(binding);
            }
        }
        self.expect("end")?;
        Ok(Stmt::Trait(TraitDecl {
            name,
            required,
            methods,
        }))
    }
    fn try_statement(&mut self, start: usize) -> Result<Stmt, Diagnostic> {
        self.exceptions += 1;
        let result = format!("__tynex_result_{}", self.exceptions);
        let parameters = if self.variadic { "..." } else { "" };
        self.edit(
            start..self.tokens[self.index - 1].span.end,
            format!("do local {result} = __tynex_pack(__tynex_pcall(function({parameters}) "),
        );
        self.protected.push(self.loops);
        let body = self.block(&["except"])?;
        self.protected.pop();
        let except = self.tok().span.clone();
        self.expect("except")?;
        let name = self.name()?;
        let error = Binding {
            name: name.text,
            annotation: Some(Type::Any),
            span: name.span,
        };
        let then = self.tok().span.clone();
        self.expect("then")?;
        let arguments = if self.variadic { ", ..." } else { "" };
        self.edit(
            except,
            format!("end{arguments})); if not {result}[1] then local "),
        );
        self.edit(then, format!("= {result}[2];"));
        let handler = self.block(&["else", "end"])?;
        let otherwise = if self.take("else") {
            self.edit(
                self.tokens[self.index - 1].span.clone(),
                format!("elseif {result}[2] == nil then"),
            );
            Some(self.block(&["end"])?)
        } else {
            None
        };
        let end = self.tok().span.clone();
        self.expect("end")?;
        let tag = if self.protected.is_empty() { "" } else { "1, " };
        let mut finish = format!(
            "end; if {result}[1] and {result}[2] == 1 then return {tag}__tynex_unpack({result}, 3, {result}.n) end;"
        );
        if self.loops > 0 {
            let escape = if self
                .protected
                .last()
                .is_some_and(|depth| self.loops <= *depth)
            {
                "return 2"
            } else {
                "break"
            };
            finish.push_str(&format!(
                " if {result}[1] and {result}[2] == 2 then {escape} end;"
            ));
        }
        finish.push_str(" end");
        self.edit(end, finish);
        Ok(Stmt::Try(Box::new(TryStatement {
            body,
            error,
            handler,
            otherwise,
        })))
    }
    fn table(&mut self) -> Result<Vec<TableField>, Diagnostic> {
        let mut fields = Vec::new();
        if !self.at("}") {
            loop {
                let field = if self.take("[") {
                    let key = self.expr(0)?;
                    self.expect("]")?;
                    self.expect("=")?;
                    TableField::Indexed(key, self.expr(0)?)
                } else if self.tok().kind == Kind::Word
                    && self
                        .tokens
                        .get(self.index + 1)
                        .is_some_and(|t| t.text == "=")
                {
                    let name = self.name()?.text.into_bytes();
                    self.expect("=")?;
                    TableField::Named(name, self.expr(0)?)
                } else {
                    TableField::List(self.expr(0)?)
                };
                fields.push(field);
                if !(self.take(",") || self.take(";")) || self.at("}") {
                    break;
                }
            }
        }
        self.expect("}")?;
        Ok(fields)
    }
    fn optional_chain(&mut self, base: Expr) -> Result<Expr, Diagnostic> {
        let mut steps = vec![];
        let mut markers = vec![];
        loop {
            let start = self.tok().span.start;
            let marker = if self.take("?") {
                if !self.typed {
                    return Err(self.fail("optional chaining requires a .tlj file"));
                }
                let marker = self.tokens[self.index - 1].span.clone();
                if marker.end != self.tok().span.start
                    || ![".", "[", "(", ":"].contains(&self.tok().text.as_str())
                {
                    return Err(self.fail("expected adjacent ?., ?[, ?(, or ?:"));
                }
                Some(marker)
            } else {
                None
            };
            let kind = if self.take(".") {
                ChainStepKind::Field(self.name()?.text.into_bytes())
            } else if self.take("[") {
                let index = self.expr(0)?;
                self.expect("]")?;
                ChainStepKind::Index(index)
            } else if self.take(":") {
                let name = self.name()?.text.into_bytes();
                ChainStepKind::MethodCall(name, self.arguments()?)
            } else if self.at("(") || self.at("{") || self.tok().kind == Kind::String {
                ChainStepKind::Call(self.arguments()?)
            } else {
                break;
            };
            steps.push(ChainStep {
                kind,
                optional: marker.is_some(),
                span: start..self.tokens[self.index - 1].span.end,
            });
            markers.push(marker);
            if !(self.at("?")
                || self.at(".")
                || self.at("[")
                || self.at(":")
                || self.at("(")
                || self.at("{")
                || self.tok().kind == Kind::String)
            {
                break;
            }
        }
        let end = steps
            .last()
            .expect("optional suffix starts a chain")
            .span
            .end;
        let parameters = if self.variadic { "..." } else { "" };
        self.edit(
            base.span.start..base.span.start,
            format!("(function({parameters}) local __tynex_value = "),
        );
        for (index, (step, marker)) in steps.iter().zip(markers).enumerate() {
            let mut prefix = String::from("; ");
            if step.optional {
                prefix.push_str("if __tynex_value == nil then return nil end; ");
            }
            prefix.push_str(if index + 1 == steps.len() {
                "return __tynex_value"
            } else {
                "__tynex_value = __tynex_value"
            });
            self.edit(marker.unwrap_or(step.span.start..step.span.start), prefix);
        }
        self.edit(end..end, format!(" end)({parameters})"));
        let span = base.span.start..end;
        Ok(Expr {
            kind: ExprKind::OptionalChain(Box::new(base), steps),
            span,
        })
    }
    fn expr(&mut self, min: u8) -> Result<Expr, Diagnostic> {
        self.enter()?;
        let t = self.tok().clone();
        let mut lhs = if self.typed && self.take("await") {
            if !self.async_body {
                return Err(Diagnostic::new(
                    "P001",
                    "await is only allowed inside an async function body",
                    t.span,
                ));
            }
            let rhs = self.expr(7)?;
            // A direct `return await task` becomes a Lua tail call and loses
            // the suspended source frame. Evaluate await as a call argument;
            // the identity helper retains all return slots without new lines.
            self.edit(
                t.span.clone(),
                "__tynex_async_values(__tynex_async_runtime.await(".into(),
            );
            self.edit(rhs.span.end..rhs.span.end, "))".into());
            Expr {
                span: t.span.start..rhs.span.end,
                kind: ExprKind::Await(Box::new(rhs)),
            }
        } else if self.take("-") || self.take("not") || self.take("#") {
            let rhs = self.expr(7)?;
            Expr {
                span: t.span.start..rhs.span.end,
                kind: ExprKind::Unary(t.text, Box::new(rhs)),
            }
        } else if self.take("(") {
            let expr = self.expr(0)?;
            self.expect(")")?;
            Expr {
                span: t.span.start..self.tokens[self.index - 1].span.end,
                kind: ExprKind::Group(Box::new(expr)),
            }
        } else {
            let kind = match t.kind {
                Kind::Number => {
                    self.index += 1;
                    ExprKind::Literal(Type::Number)
                }
                Kind::String => {
                    self.index += 1;
                    ExprKind::String(decode_string(&t.text))
                }
                _ if t.text == "true" || t.text == "false" => {
                    self.index += 1;
                    ExprKind::Bool(t.text == "true")
                }
                _ if t.text == "..." => {
                    if !self.variadic {
                        return Err(
                            self.fail("... is only available in the current variadic function")
                        );
                    }
                    self.index += 1;
                    ExprKind::Varargs
                }
                _ if t.text == "nil" => {
                    self.index += 1;
                    ExprKind::Literal(Type::Nil)
                }
                _ if self.take("{") => ExprKind::Table(self.table()?),
                _ if self.typed && self.take("async") => {
                    self.expect("function")?;
                    self.edit(t.span.clone(), "__tynex_async_runtime.create(".into());
                    ExprKind::Function(Box::new(self.function(t.span.start, true)?))
                }
                _ if self.take("function") => {
                    ExprKind::Function(Box::new(self.function(t.span.start, false)?))
                }
                Kind::Word => ExprKind::Name(self.name()?.text),
                _ => return Err(self.fail(
                    "expected an expression; varargs and advanced syntax are not implemented yet",
                )),
            };
            Expr {
                kind,
                span: t.span.start..self.tokens[self.index.saturating_sub(1)].span.end,
            }
        };
        loop {
            let prefix = matches!(
                lhs.kind,
                ExprKind::Name(_)
                    | ExprKind::Group(_)
                    | ExprKind::Call(..)
                    | ExprKind::MethodCall(..)
                    | ExprKind::Field(..)
                    | ExprKind::Index(..)
                    | ExprKind::OptionalChain(..)
            );
            if self.at("?") {
                if !prefix {
                    return Err(self.fail("parenthesize a literal before optional chaining"));
                }
                lhs = self.optional_chain(lhs)?;
                continue;
            }
            if self.at("(")
                || self.at(".")
                || self.at(":")
                || self.at("[")
                || self.at("{")
                || self.tok().kind == Kind::String
            {
                if !prefix {
                    return Err(self.fail("parenthesize a literal before indexing or calling it"));
                }
                if self.take(":") {
                    let name = self.name()?.text.into_bytes();
                    let args = self.arguments()?;
                    let span = lhs.span.start..self.tokens[self.index - 1].span.end;
                    lhs = Expr {
                        kind: ExprKind::MethodCall(Box::new(lhs), name, args),
                        span,
                    };
                    continue;
                }
                if self.take(".") {
                    let field = self.name()?;
                    let span = lhs.span.start..field.span.end;
                    lhs = Expr {
                        kind: ExprKind::Field(Box::new(lhs), field.text.into_bytes()),
                        span,
                    };
                    continue;
                }
                if self.take("[") {
                    let index = self.expr(0)?;
                    self.expect("]")?;
                    let span = lhs.span.start..self.tokens[self.index - 1].span.end;
                    lhs = Expr {
                        kind: ExprKind::Index(Box::new(lhs), Box::new(index)),
                        span,
                    };
                    continue;
                }
                let args = self.arguments()?;
                let span = lhs.span.start..self.tokens[self.index - 1].span.end;
                lhs = Expr {
                    kind: ExprKind::Call(Box::new(lhs), args),
                    span,
                };
                continue;
            }
            let (left, right) = match self.tok().text.as_str() {
                "or" => (1, 2),
                "and" => (2, 3),
                "==" | "~=" | "<" | ">" | "<=" | ">=" => (3, 4),
                ".." => (4, 4),
                "+" | "-" => (5, 6),
                "*" | "/" | "%" => (6, 7),
                "^" => (8, 8),
                _ => break,
            };
            if left < min {
                break;
            }
            let op = self.tok().text.clone();
            self.index += 1;
            let rhs = self.expr(right)?;
            let span = lhs.span.start..rhs.span.end;
            lhs = Expr {
                kind: ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)),
                span,
            };
        }
        self.depth -= 1;
        Ok(lhs)
    }
}
