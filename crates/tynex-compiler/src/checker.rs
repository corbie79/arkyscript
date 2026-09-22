use crate::{Diagnostic, Import, Interface, Mode, ast::*};
use std::{
    collections::{BTreeMap, HashMap},
    ops::Range,
};

#[derive(Clone)]
struct Variable {
    readonly: bool,
    declared: Type,
    current: Type,
}
type Scopes = Vec<HashMap<String, Variable>>;
struct Flow {
    returns: Vec<Pack>,
    falls: bool,
    breaks: bool,
}
impl Default for Flow {
    fn default() -> Self {
        Self {
            returns: Vec::new(),
            falls: true,
            breaks: false,
        }
    }
}
pub(crate) struct Analysis {
    pub errors: Vec<Diagnostic>,
    pub imports: Vec<Import>,
    pub interface: Interface,
}
struct Checker<'a> {
    modules: &'a BTreeMap<String, Interface>,
    imports: Vec<Import>,
    scopes: Scopes,
    mode: Mode,
    errors: Vec<Diagnostic>,
    depth: usize,
    varargs: Option<Type>,
    conditional: usize,
}

fn function(args: Vec<Type>, result: Type) -> Type {
    Type::Function(args, Box::new(Pack::one(result)), None)
}
fn library(fields: Vec<(&str, Type)>) -> Type {
    Type::Table(
        TableType {
            fields: fields
                .into_iter()
                .map(|(n, t)| (n.as_bytes().to_vec(), t))
                .collect(),
            index: None,
        }
        .into(),
    )
}
pub fn check(body: &[Stmt], mode: Mode, modules: &BTreeMap<String, Interface>) -> Analysis {
    let mut c = Checker {
        modules,
        imports: vec![],
        scopes: vec![HashMap::new(), HashMap::new()],
        mode,
        errors: Vec::new(),
        depth: 0,
        varargs: Some(Type::Any),
        conditional: 0,
    };
    let values = vec![
        (
            "print",
            Type::Function(vec![], Box::default(), Some(Box::new(Type::Any))),
        ),
        ("tostring", function(vec![Type::Any], Type::String)),
        (
            "tonumber",
            function(
                vec![Type::Any, Type::Number.optional()],
                Type::Number.optional(),
            ),
        ),
        ("require", Type::Require),
        (
            "error",
            function(vec![Type::Any, Type::Number.optional()], Type::Never),
        ),
        ("type", function(vec![Type::Any], Type::String)),
        ("pairs", Type::IteratorFactory(false)),
        ("ipairs", Type::IteratorFactory(true)),
        ("select", Type::Select),
        ("unpack", Type::Unpack),
        (
            "math",
            library(vec![
                ("abs", function(vec![Type::Number], Type::Number)),
                ("floor", function(vec![Type::Number], Type::Number)),
                ("ceil", function(vec![Type::Number], Type::Number)),
                ("sqrt", function(vec![Type::Number], Type::Number)),
                (
                    "min",
                    Type::Function(
                        vec![Type::Number],
                        Box::new(Pack::one(Type::Number)),
                        Some(Box::new(Type::Number)),
                    ),
                ),
                (
                    "max",
                    Type::Function(
                        vec![Type::Number],
                        Box::new(Pack::one(Type::Number)),
                        Some(Box::new(Type::Number)),
                    ),
                ),
                ("pi", Type::Number),
                ("huge", Type::Number),
            ]),
        ),
        (
            "string",
            library(vec![
                ("len", function(vec![Type::String], Type::Number)),
                ("upper", function(vec![Type::String], Type::String)),
                ("lower", function(vec![Type::String], Type::String)),
                (
                    "sub",
                    function(
                        vec![Type::String, Type::Number, Type::Number.optional()],
                        Type::String,
                    ),
                ),
            ]),
        ),
    ];
    for (name, ty) in values {
        c.scopes[0].insert(
            name.into(),
            Variable {
                readonly: false,
                declared: ty.clone(),
                current: ty,
            },
        );
    }
    let flow = c.body(body, None);
    let mut result = flow
        .returns
        .into_iter()
        .fold(Pack::one(Type::Never), Pack::join);
    if flow.falls {
        result = result.join(Pack::default());
    }
    let value = module_value(result.at(0));
    c.imports.sort_by_key(|i| i.span.start);
    c.imports.dedup_by(|a, b| a.span == b.span);
    Analysis {
        errors: c.errors,
        imports: c.imports,
        interface: Interface { value },
    }
}
fn module_value(ty: Type) -> Type {
    match ty {
        Type::Nil => Type::Boolean,
        Type::Optional(inner) => (*inner).join(Type::Boolean),
        Type::Union(types) => types
            .into_iter()
            .map(module_value)
            .fold(Type::Never, Type::join),
        other => other,
    }
}
impl Checker<'_> {
    fn error(&mut self, code: &'static str, message: impl Into<String>, span: Range<usize>) {
        self.errors.push(Diagnostic::new(code, message, span));
    }
    fn lookup(&self, name: &str) -> Option<&Variable> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }
    fn lookup_mut(&mut self, name: &str) -> Option<&mut Variable> {
        self.scopes.iter_mut().rev().find_map(|s| s.get_mut(name))
    }
    fn insert(&mut self, name: &str, declared: Type, current: Type) {
        self.scopes.last_mut().unwrap().insert(
            name.into(),
            Variable {
                declared,
                current,
                readonly: false,
            },
        );
    }
    fn reset(&mut self) {
        for scope in &mut self.scopes {
            for v in scope.values_mut() {
                v.current = v.declared.clone();
            }
        }
    }
    fn accepts(&self, expected: &Type, actual: &Type) -> bool {
        if *expected == Type::Any
            || *actual == Type::Any
            || *actual == Type::Never
            || expected == actual
        {
            return true;
        }
        if *expected == Type::Unknown || *actual == Type::Unknown {
            return self.mode == Mode::Gradual;
        }
        if let Type::Union(types) = actual {
            return types.iter().all(|t| self.accepts(expected, t));
        }
        if let Type::Union(types) = expected {
            return types.iter().any(|t| self.accepts(t, actual));
        }
        if let Type::Optional(inner) = expected {
            if let Type::Optional(actual) = actual {
                return self.accepts(inner, actual);
            }
            return *actual == Type::Nil || self.accepts(inner, actual);
        }
        if let Type::Receiver(required) = expected {
            if let (Type::Table(required), Type::Table(actual)) = (required.as_ref(), actual) {
                return required
                    .fields
                    .iter()
                    .all(|(k, v)| actual.fields.get(k) == Some(v))
                    && (required.index.is_none() || required.index == actual.index);
            }
            return self.accepts(required, actual);
        }
        // Mutable tables and function contracts are invariant. Fresh literals
        // are contextualized separately; existing aliases are never widened.
        false
    }
    fn expect(&mut self, expected: &Type, actual: &Type, span: Range<usize>) {
        if !self.accepts(expected, actual) {
            self.error("T001", format!("expected {expected}, got {actual}"), span);
        }
    }
    fn bind(&mut self, binding: &Binding, actual: Type) {
        let declared = binding.annotation.clone().unwrap_or_else(|| actual.clone());
        if binding.annotation.is_some() {
            self.expect(&declared, &actual, binding.span.clone());
        } else if actual.has_unknown() && self.mode == Mode::Strict {
            self.error(
                "T002",
                format!(
                    "cannot infer '{}'; add a type or an explicit any boundary",
                    binding.name
                ),
                binding.span.clone(),
            );
        }
        // Annotation contracts take precedence over escaped dynamic values.
        let current = if declared == Type::Any || actual == Type::Any || actual == Type::Unknown {
            declared.clone()
        } else {
            actual
        };
        self.insert(&binding.name, declared, current);
    }
    fn scoped(&mut self, body: &[Stmt], returns: Option<&Pack>) -> Flow {
        self.scopes.push(HashMap::new());
        let flow = self.body(body, returns);
        self.scopes.pop();
        flow
    }
    fn body(&mut self, body: &[Stmt], returns: Option<&Pack>) -> Flow {
        let mut flow = Flow::default();
        for stmt in body {
            let next = self.statement(stmt, returns);
            if flow.falls {
                flow.returns.extend(next.returns);
                flow.breaks |= next.breaks;
                flow.falls = next.falls;
            }
        }
        flow
    }
    fn statement(&mut self, stmt: &Stmt, expected_return: Option<&Pack>) -> Flow {
        let mut flow = Flow::default();
        let conditional = matches!(
            stmt,
            Stmt::If(..)
                | Stmt::Try(..)
                | Stmt::While(..)
                | Stmt::Repeat(..)
                | Stmt::NumericFor(..)
                | Stmt::GenericFor(..)
        );
        if conditional {
            self.conditional += 1;
        }
        match stmt {
            Stmt::Import(binding, name) => {
                self.imports.push(Import {
                    name: Some(name.clone()),
                    span: binding.span.clone(),
                    static_import: true,
                });
                let value = self
                    .modules
                    .get(name)
                    .map(|i| i.value.clone())
                    .unwrap_or(Type::Unknown);
                // Module initialization can call host functions that mutate captured locals.
                self.reset();
                self.check_redeclaration(binding);
                self.bind(binding, value);
                self.lookup_mut(&binding.name).unwrap().readonly = true;
            }
            Stmt::Try(statement) => {
                let initial = self.scopes.clone();
                let success = self.scoped(&statement.body, expected_return);
                let mut continuing = vec![];
                let otherwise = statement
                    .otherwise
                    .as_ref()
                    .map(|body| self.scoped(body, expected_return));
                if success.falls && otherwise.as_ref().is_none_or(|f| f.falls) {
                    continuing.push(self.scopes.clone());
                }
                flow.returns = success.returns;
                flow.breaks = success.breaks;
                if success.falls
                    && let Some(otherwise) = otherwise
                {
                    flow.returns.extend(otherwise.returns);
                    flow.breaks |= otherwise.breaks;
                }
                self.scopes = initial.clone();
                // An exception can follow any partial assignment in the body.
                self.reset();
                self.scopes.push(HashMap::new());
                self.bind(&statement.error, Type::Any);
                let handler = self.body(&statement.handler, expected_return);
                self.scopes.pop();
                if handler.falls {
                    continuing.push(self.scopes.clone());
                }
                flow.returns.extend(handler.returns);
                flow.breaks |= handler.breaks;
                flow.falls = !continuing.is_empty();
                self.scopes = initial;
                if let Some(first) = continuing.first() {
                    for (i, scope) in self.scopes.iter_mut().enumerate() {
                        for (name, var) in scope {
                            var.current = continuing
                                .iter()
                                .skip(1)
                                .fold(first[i][name].current.clone(), |ty, s| {
                                    ty.join(s[i][name].current.clone())
                                });
                        }
                    }
                }
            }
            Stmt::Local(bindings, expressions) => {
                let expected: Vec<_> = bindings.iter().map(|b| b.annotation.clone()).collect();
                let values = self.expressions(expressions, &expected, None);
                // All RHS expressions see the old scope, including shadowed names.
                for (i, binding) in bindings.iter().enumerate() {
                    let ty = if expressions.is_empty() && binding.annotation.is_none() {
                        Type::Unknown
                    } else {
                        values.at(i)
                    };
                    self.check_redeclaration(binding);
                    self.bind(binding, ty);
                }
            }
            Stmt::Assign(targets, expressions) => {
                let contracts: Vec<_> = targets
                    .iter()
                    .map(|e| Some(self.assignment_type(e)))
                    .collect();
                let values = self.expressions(expressions, &contracts, None);
                for (i, target) in targets.iter().enumerate() {
                    self.expect(
                        contracts[i].as_ref().unwrap(),
                        &values.at(i),
                        target.span.clone(),
                    );
                }
                // The pinned LuaJIT writes duplicate destinations right-to-left (a,a=1,2 => 1).
                for (i, target) in targets.iter().enumerate().rev() {
                    if let ExprKind::Name(name) = &target.kind
                        && let Some(variable) = self.lookup_mut(name)
                    {
                        let actual = values.at(i);
                        variable.current = if variable.declared == Type::Any
                            || matches!(actual, Type::Any | Type::Unknown)
                        {
                            variable.declared.clone()
                        } else {
                            actual
                        };
                    }
                }
            }
            Stmt::Function(binding, function) => {
                self.check_redeclaration(binding);
                let ty = self.function(function, Some(&binding.name), None);
                self.insert(&binding.name, ty.clone(), ty);
            }
            Stmt::NamedFunction(target, function, method) => {
                self.named_function(target, function, *method)
            }
            Stmt::Call(expr) => {
                if self.expr(expr) == Type::Never {
                    flow.falls = false;
                }
            }
            Stmt::Return(expressions, span) => {
                let expected: Vec<_> = expected_return
                    .map(|p| p.values.iter().cloned().map(Some).collect())
                    .unwrap_or_default();
                let pack = self.expressions(
                    expressions,
                    &expected,
                    expected_return.and_then(|p| p.tail.as_deref()),
                );
                if let Some(expected) = expected_return {
                    self.expect_returns(expected, &pack, span.clone());
                }
                flow.returns.push(pack);
                flow.falls = false;
            }
            Stmt::Break => {
                flow.breaks = true;
                flow.falls = false;
            }
            Stmt::Block(body) => flow = self.scoped(body, expected_return),
            Stmt::If(branches, otherwise) => {
                let initial = self.scopes.clone();
                let mut continuing = Vec::new();
                flow.falls = false;
                for (condition, body) in branches {
                    self.expr(condition);
                    let false_path = self.scopes.clone();
                    self.narrow(condition, true);
                    let branch = self.scoped(body, expected_return);
                    if branch.falls {
                        continuing.push(self.scopes.clone());
                    }
                    flow.returns.extend(branch.returns);
                    flow.breaks |= branch.breaks;
                    self.scopes = false_path;
                    self.narrow(condition, false);
                }
                if let Some(body) = otherwise {
                    let branch = self.scoped(body, expected_return);
                    if branch.falls {
                        continuing.push(self.scopes.clone());
                    }
                    flow.returns.extend(branch.returns);
                    flow.breaks |= branch.breaks;
                } else {
                    continuing.push(self.scopes.clone());
                }
                flow.falls = !continuing.is_empty();
                self.scopes = initial;
                if let Some(first) = continuing.first() {
                    for (i, scope) in self.scopes.iter_mut().enumerate() {
                        for (name, var) in scope {
                            var.current = continuing
                                .iter()
                                .skip(1)
                                .fold(first[i][name].current.clone(), |ty, s| {
                                    ty.join(s[i][name].current.clone())
                                });
                        }
                    }
                }
            }
            Stmt::While(condition, body) => {
                self.reset();
                self.expr(condition);
                self.narrow(condition, true);
                let inner = self.scoped(body, expected_return);
                flow.returns = inner.returns;
                flow.falls = !matches!(condition.kind, ExprKind::Bool(true)) || inner.breaks;
                self.reset();
            }
            Stmt::Repeat(body, condition) => {
                self.reset();
                self.scopes.push(HashMap::new());
                let inner = self.body(body, expected_return);
                self.expr(condition);
                self.scopes.pop();
                flow.returns = inner.returns;
                flow.falls = inner.breaks
                    || (inner.falls && !matches!(condition.kind, ExprKind::Bool(false)));
                self.reset();
            }
            Stmt::NumericFor(statement) => {
                let NumericFor {
                    binding,
                    from,
                    to,
                    step,
                    body,
                } = statement.as_ref();
                self.reset();
                for bound in [Some(from), Some(to), step.as_ref()].into_iter().flatten() {
                    let ty = self.expr(bound);
                    self.expect(&Type::Number, &ty, bound.span.clone());
                }
                self.scopes.push(HashMap::new());
                self.bind(binding, Type::Number);
                let inner = self.body(body, expected_return);
                self.scopes.pop();
                flow.returns = inner.returns;
                self.reset();
            }
            Stmt::GenericFor(bindings, iterator, body) => {
                self.reset();
                let (key, value) = self.iterator(iterator);
                self.scopes.push(HashMap::new());
                for (i, binding) in bindings.iter().enumerate() {
                    self.bind(
                        binding,
                        match i {
                            0 => key.clone(),
                            1 => value.clone(),
                            _ => Type::Nil,
                        },
                    );
                }
                let inner = self.body(body, expected_return);
                self.scopes.pop();
                flow.returns = inner.returns;
                self.reset();
            }
        }
        if conditional {
            self.conditional -= 1;
        }
        flow
    }
    fn signature(&self, function: &Function, receiver: Option<&Type>) -> Type {
        let mut params: Vec<_> = function
            .params
            .iter()
            .map(|p| p.annotation.clone().unwrap_or(Type::Unknown))
            .collect();
        if let Some(receiver) = receiver {
            params.insert(0, Type::Receiver(Box::new(receiver.clone())));
        }
        Type::Function(
            params,
            Box::new(
                function
                    .returns
                    .clone()
                    .unwrap_or_else(|| Pack::variadic(Type::Unknown)),
            ),
            function
                .variadic
                .as_ref()
                .map(|b| Box::new(b.annotation.clone().unwrap_or(Type::Unknown))),
        )
    }
    fn function(
        &mut self,
        function: &Function,
        recursive: Option<&str>,
        receiver: Option<&Type>,
    ) -> Type {
        let Type::Function(params, _, variadic) = self.signature(function, receiver) else {
            unreachable!()
        };
        let saved = self.scopes.clone();
        let old_varargs = self.varargs.clone();
        let old_conditional = self.conditional;
        self.conditional = 0;
        self.varargs = variadic.as_deref().cloned();
        self.reset();
        if let Some(name) = recursive {
            let ty = self.signature(function, receiver);
            self.insert(name, ty.clone(), ty);
        }
        self.scopes.push(HashMap::new());
        if let Some(receiver) = receiver {
            self.insert("self", receiver.clone(), receiver.clone());
        }
        for param in function.params.iter().chain(function.variadic.iter()) {
            let ty = param.annotation.clone().unwrap_or(Type::Unknown);
            if ty == Type::Unknown && self.mode == Mode::Strict {
                self.error(
                    "T002",
                    format!("cannot infer parameter '{}'; add a type", param.name),
                    param.span.clone(),
                );
            }
            if self.scopes.last().unwrap().contains_key(&param.name) {
                self.error("T004", "duplicate parameter", param.span.clone());
            }
            self.insert(&param.name, ty.clone(), ty);
        }
        let flow = self.body(&function.body, function.returns.as_ref());
        let mut result = flow
            .returns
            .into_iter()
            .fold(Pack::one(Type::Never), Pack::join);
        if flow.falls {
            if let Some(expected) = &function.returns {
                self.expect_returns(expected, &Pack::default(), function.span.clone());
            }
            result = result.join(Pack::default());
        }
        if function.returns.is_none() && result.has_unknown() && self.mode == Mode::Strict {
            self.error(
                "T002",
                "cannot infer return type; add a return annotation",
                function.span.clone(),
            );
        }
        if let Some(contract) = &function.returns {
            if result.tail.is_some() && contract.tail.is_none() {
                result = contract.clone();
            }
            // Preserve actual result counts, including zero returns, while exposing
            // annotated types (rather than an implementation's narrower types).
            for (i, value) in result.values.iter_mut().enumerate() {
                if let Some(ty) = contract.values.get(i).or(contract.tail.as_deref()) {
                    *value = ty.clone();
                }
            }
            if result.tail.is_some() {
                result.tail = contract.tail.clone().or(result.tail);
            }
        }
        self.scopes = saved;
        self.varargs = old_varargs;
        self.conditional = old_conditional;
        Type::Function(params, Box::new(result), variadic)
    }
    fn expect_returns(&mut self, expected: &Pack, actual: &Pack, span: Range<usize>) {
        if actual.values == [Type::Never] {
            return;
        }
        let dynamic = actual.tail.as_ref().is_some_and(|t| {
            **t == Type::Any || **t == Type::Unknown && self.mode == Mode::Gradual
        });
        if expected.tail.is_none()
            && (actual.values.len() > expected.values.len() || actual.tail.is_some() && !dynamic)
        {
            self.error(
                "T006",
                "too many return values for the declared result list",
                span.clone(),
            );
        }
        for (i, ty) in expected.values.iter().enumerate() {
            self.expect(ty, &actual.at(i), span.clone());
        }
        if let Some(tail) = &expected.tail {
            for ty in actual.values.iter().skip(expected.values.len()) {
                self.expect(tail, ty, span.clone());
            }
            if let Some(ty) = &actual.tail {
                self.expect(tail, ty, span);
            }
        }
    }
    fn named_function(&mut self, target: &Expr, function: &Function, method: bool) {
        self.check_readonly(target);
        let receiver = if method {
            if let ExprKind::Field(object, _) = &target.kind {
                Some(self.expr(object))
            } else {
                None
            }
        } else {
            None
        };
        let old = self.defined_type(target);
        if old.is_none() && self.conditional > 0 {
            self.error(
                "T011",
                "declare new functions/table members outside conditional blocks and loops",
                target.span.clone(),
            );
        }
        let signature = self.signature(function, receiver.as_ref());
        // Seed the qualified name for recursive calls. The function body's
        // temporary state is restored by function(), just as for local functions.
        self.define(target, signature);
        let recursive = if let ExprKind::Name(name) = &target.kind {
            Some(name.as_str())
        } else {
            None
        };
        let result = self.function(function, recursive, receiver.as_ref());
        if let Some(old) = old {
            self.expect(&old, &result, target.span.clone());
        }
        self.define(target, result);
    }
    fn defined_type(&self, target: &Expr) -> Option<Type> {
        match &target.kind {
            ExprKind::Name(name) => self.lookup(name).map(|v| v.declared.clone()),
            ExprKind::Field(object, key) => match self.defined_type(object)? {
                Type::Table(t) => t
                    .fields
                    .get(key)
                    .cloned()
                    .or_else(|| t.index.as_ref().map(|(_, v)| (**v).clone())),
                Type::Any => Some(Type::Any),
                _ => None,
            },
            _ => None,
        }
    }
    fn define(&mut self, target: &Expr, ty: Type) {
        match &target.kind {
            ExprKind::Name(name) => {
                let value = Variable {
                    readonly: self.lookup(name).is_some_and(|v| v.readonly),
                    declared: ty.clone(),
                    current: ty,
                };
                if let Some(var) = self.lookup_mut(name) {
                    *var = value;
                } else {
                    self.scopes[0].insert(name.clone(), value);
                }
            }
            ExprKind::Field(object, key) => {
                let object_type = self.defined_type(object).unwrap_or(Type::Unknown);
                match object_type {
                    Type::Table(table) => {
                        let mut table = (*table).clone();
                        if let Some((index, value)) = &table.index {
                            self.expect(index, &Type::String, target.span.clone());
                            self.expect(value, &ty, target.span.clone());
                        }
                        table.fields.insert(key.clone(), ty);
                        self.define(object, Type::Table(table.into()));
                    }
                    Type::Any => {}
                    Type::Unknown if self.mode == Mode::Gradual => {}
                    _ => self.error(
                        "T005",
                        "function member requires a known table receiver",
                        object.span.clone(),
                    ),
                }
            }
            _ => unreachable!(),
        }
    }
    fn narrow(&mut self, condition: &Expr, truth: bool) {
        match &condition.kind {
            ExprKind::Group(expr) => self.narrow(expr, truth),
            ExprKind::Unary(op, expr) if op == "not" => self.narrow(expr, !truth),
            ExprKind::Binary(op, lhs, rhs) if op == "==" || op == "~=" => {
                let name = if matches!(rhs.kind, ExprKind::Literal(Type::Nil)) {
                    Self::name(lhs)
                } else if matches!(lhs.kind, ExprKind::Literal(Type::Nil)) {
                    Self::name(rhs)
                } else {
                    None
                };
                if let Some(name) = name
                    && let Some(var) = self.lookup_mut(name)
                {
                    var.current = if truth == (op == "~=") {
                        var.current.without_nil()
                    } else {
                        Type::Nil
                    };
                }
            }
            ExprKind::Name(name) => {
                if let Some(var) = self.lookup_mut(name) {
                    var.current = if truth {
                        var.current.without_nil()
                    } else {
                        Self::falsy(&var.current)
                    };
                }
            }
            // Compound conditions are deliberately not reconstructed after their
            // RHS: a call in the RHS may invalidate an earlier refinement.
            _ => {}
        }
    }
    fn string_constant(expr: &Expr) -> Option<&[u8]> {
        match &expr.kind {
            ExprKind::String(s) => Some(s),
            ExprKind::Group(e) => Self::string_constant(e),
            _ => None,
        }
    }
    fn name(expr: &Expr) -> Option<&str> {
        match &expr.kind {
            ExprKind::Name(n) => Some(n),
            ExprKind::Group(e) => Self::name(e),
            _ => None,
        }
    }
    fn falsy(ty: &Type) -> Type {
        match ty {
            Type::Optional(t) => Self::falsy(t).join(Type::Nil),
            Type::Union(ts) => ts.iter().map(Self::falsy).fold(Type::Never, Type::join),
            Type::Any | Type::Unknown | Type::Boolean | Type::Nil => ty.clone(),
            _ => Type::Never,
        }
    }
    fn expr_expected(&mut self, expr: &Expr, expected: Option<&Type>) -> Type {
        let expected = match expected {
            Some(Type::Optional(inner)) => Some(inner.as_ref()),
            other => other,
        };
        if let ExprKind::Group(inner) = &expr.kind {
            return self.expr_expected(inner, expected);
        }
        if let (ExprKind::Table(fields), Some(Type::Table(table))) = (&expr.kind, expected) {
            return self.table(fields, Some(table), expr.span.clone());
        }
        self.expr(expr)
    }
    fn expr(&mut self, expr: &Expr) -> Type {
        if self.depth >= 128 {
            self.error(
                "T008",
                "expression nesting exceeds the current limit (128)",
                expr.span.clone(),
            );
            return Type::Unknown;
        }
        self.depth += 1;
        let result = match &expr.kind {
            ExprKind::Group(inner) => self.expr(inner),
            ExprKind::Literal(ty) => ty.clone(),
            ExprKind::String(_) => Type::String,
            ExprKind::Bool(_) => Type::Boolean,
            ExprKind::Name(name) => {
                self.lookup(name)
                    .map(|v| v.current.clone())
                    .unwrap_or_else(|| {
                        if self.mode == Mode::Strict {
                            self.error(
                                "T003",
                                format!("unknown variable '{name}'"),
                                expr.span.clone(),
                            );
                        }
                        Type::Unknown
                    })
            }
            ExprKind::Function(function) => self.function(function, None, None),
            ExprKind::Table(fields) => self.table(fields, None, expr.span.clone()),
            ExprKind::Unary(op, rhs) => {
                let ty = self.expr(rhs);
                match op.as_str() {
                    "not" => Type::Boolean,
                    "#" => {
                        if !(matches!(ty, Type::String | Type::Table(_) | Type::Any | Type::Never)
                            || ty == Type::Unknown && self.mode == Mode::Gradual)
                        {
                            self.error(
                                "T001",
                                "length requires a string or table",
                                rhs.span.clone(),
                            );
                        }
                        Type::Number
                    }
                    _ => {
                        self.expect(&Type::Number, &ty, rhs.span.clone());
                        Type::Number
                    }
                }
            }
            ExprKind::Binary(op, lhs, rhs) => {
                let left = self.expr(lhs);
                if op == "and" || op == "or" {
                    let before = self.scopes.clone();
                    self.narrow(lhs, op == "and");
                    let right = self.expr(rhs);
                    // Conservatively discard refinements after a conditional
                    // expression: only one path may execute its assignments/calls.
                    self.scopes = before;
                    self.reset();
                    let retained = if op == "and" {
                        Self::falsy(&left)
                    } else {
                        left.without_nil()
                    };
                    retained.join(right)
                } else {
                    let right = self.expr(rhs);
                    match op.as_str() {
                        "==" | "~=" => Type::Boolean,
                        "<" | ">" | "<=" | ">=" => {
                            if !(matches!(
                                left,
                                Type::Number | Type::String | Type::Any | Type::Never
                            ) || left == Type::Unknown && self.mode == Mode::Gradual)
                            {
                                self.error(
                                    "T001",
                                    "ordered comparison requires numbers or strings",
                                    lhs.span.clone(),
                                );
                            }
                            self.expect(&left, &right, rhs.span.clone());
                            Type::Boolean
                        }
                        ".." => {
                            for (ty, e) in [(&left, lhs), (&right, rhs)] {
                                if !(matches!(
                                    ty,
                                    Type::Number | Type::String | Type::Any | Type::Never
                                ) || *ty == Type::Unknown && self.mode == Mode::Gradual)
                                {
                                    self.error(
                                        "T001",
                                        "concatenation requires a string or number",
                                        e.span.clone(),
                                    );
                                }
                            }
                            Type::String
                        }
                        _ => {
                            self.expect(&Type::Number, &left, lhs.span.clone());
                            self.expect(&Type::Number, &right, rhs.span.clone());
                            Type::Number
                        }
                    }
                }
            }
            ExprKind::Field(object, key) => {
                let ty = self.expr(object);
                self.access(&ty, &Type::String, Some(key), false, expr.span.clone())
            }
            ExprKind::Index(object, key) => {
                let ty = self.expr(object);
                let key_type = self.expr(key);
                let name = if let ExprKind::String(s) = &key.kind {
                    Some(s.as_slice())
                } else {
                    None
                };
                self.access(&ty, &key_type, name, false, expr.span.clone())
            }
            ExprKind::Call(..) | ExprKind::MethodCall(..) | ExprKind::Varargs => {
                self.expansion(expr).at(0)
            }
        };
        self.depth -= 1;
        result
    }
    fn expressions(
        &mut self,
        expressions: &[Expr],
        expected: &[Option<Type>],
        tail: Option<&Type>,
    ) -> Pack {
        let mut out = Pack::default();
        for (i, expr) in expressions.iter().enumerate() {
            if i + 1 == expressions.len()
                && matches!(
                    expr.kind,
                    ExprKind::Call(..) | ExprKind::MethodCall(..) | ExprKind::Varargs
                )
            {
                let pack = self.expansion(expr);
                out.minimum += pack.minimum;
                out.values.extend(pack.values);
                out.tail = pack.tail;
            } else {
                let ty =
                    self.expr_expected(expr, expected.get(i).and_then(Option::as_ref).or(tail));
                out.values.push(ty);
                out.minimum += 1;
            }
        }
        out
    }
    fn expansion(&mut self, expr: &Expr) -> Pack {
        // Calls in a list bypass expr(), so maintain the same recursion guard.
        if self.depth >= 128 {
            self.error(
                "T008",
                "expression nesting exceeds the current limit (128)",
                expr.span.clone(),
            );
            return Pack::variadic(Type::Unknown);
        }
        self.depth += 1;
        let result = match &expr.kind {
            ExprKind::Varargs => Pack::variadic(self.varargs.clone().unwrap_or(Type::Unknown)),
            ExprKind::Call(callee, args) => {
                let ty = self.expr(callee);
                self.call(&ty, args, None, expr.span.clone())
            }
            ExprKind::MethodCall(object, key, args) => {
                // Evaluate the receiver exactly once; colon supplies it as arg 1.
                let receiver = self.expr(object);
                let ty = if receiver == Type::String {
                    let library = self.scopes[0]["string"].declared.clone();
                    self.access(&library, &Type::String, Some(key), false, expr.span.clone())
                } else {
                    self.access(
                        &receiver,
                        &Type::String,
                        Some(key),
                        false,
                        expr.span.clone(),
                    )
                };
                self.call(&ty, args, Some(receiver), expr.span.clone())
            }
            _ => unreachable!(),
        };
        self.depth -= 1;
        result
    }
    fn call(
        &mut self,
        ty: &Type,
        arguments: &[Expr],
        receiver: Option<Type>,
        span: Range<usize>,
    ) -> Pack {
        let offset = usize::from(receiver.is_some());
        let result = match ty {
            Type::Function(params, returns, variadic) => {
                let expected: Vec<_> = params.iter().skip(offset).cloned().map(Some).collect();
                let mut actual = self.expressions(arguments, &expected, variadic.as_deref());
                if let Some(receiver) = receiver {
                    actual.values.insert(0, receiver);
                    actual.minimum += 1;
                }
                let minimum = params
                    .iter()
                    .rposition(|p| !matches!(p, Type::Optional(_) | Type::Nil))
                    .map_or(0, |i| i + 1);
                let dynamic = actual.tail.as_ref().is_some_and(|t| {
                    **t == Type::Any || **t == Type::Unknown && self.mode == Mode::Gradual
                });
                if (!dynamic && actual.minimum < minimum)
                    || (variadic.is_none()
                        && (actual.values.len() > params.len()
                            || actual.tail.is_some() && !dynamic))
                {
                    self.error(
                        "T006",
                        "argument count does not fit the function signature",
                        span.clone(),
                    );
                }
                for (i, param) in params.iter().enumerate() {
                    self.expect(param, &actual.at(i), span.clone());
                }
                if let Some(tail) = variadic {
                    for value in actual.values.iter().skip(params.len()) {
                        self.expect(tail, value, span.clone());
                    }
                    if let Some(value) = &actual.tail {
                        self.expect(tail, value, span.clone());
                    }
                }
                (**returns).clone()
            }
            Type::Require => {
                let actual = self.expressions(arguments, &[Some(Type::String)], None);
                if receiver.is_some()
                    || actual.minimum != 1
                    || actual.values.len() != 1
                    || actual.tail.is_some()
                {
                    self.error("T006", "require expects one module name", span.clone());
                }
                self.expect(&Type::String, &actual.at(0), span.clone());
                let name = arguments
                    .first()
                    .and_then(Self::string_constant)
                    .and_then(|s| std::str::from_utf8(s).ok())
                    .map(str::to_owned);
                let value = name
                    .as_ref()
                    .and_then(|n| self.modules.get(n))
                    .map(|i| i.value.clone())
                    .unwrap_or(Type::Unknown);
                self.imports.push(Import {
                    name,
                    span,
                    static_import: false,
                });
                Pack::one(value)
            }
            Type::Select => {
                let actual = self.expressions(arguments, &[], None);
                let hash = arguments
                    .first()
                    .is_some_and(|e| matches!(&e.kind, ExprKind::String(s) if s == b"#"));
                if receiver.is_some() || actual.minimum == 0 {
                    self.error("T006", "select requires an index or '#'", span.clone());
                }
                if hash {
                    Pack::one(Type::Number)
                } else {
                    self.expect(&Type::Number, &actual.at(0), span.clone());
                    let value = actual
                        .values
                        .iter()
                        .skip(1)
                        .chain(actual.tail.as_deref())
                        .cloned()
                        .fold(Type::Never, Type::join);
                    if value == Type::Never {
                        Pack::default()
                    } else {
                        Pack::variadic(value)
                    }
                }
            }
            Type::Unpack => {
                let actual = self.expressions(arguments, &[], None);
                if receiver.is_some()
                    || actual.minimum < 1
                    || actual.values.len() > 3
                    || actual.tail.is_some()
                {
                    self.error(
                        "T006",
                        "unpack expects a table and up to two numeric bounds",
                        span.clone(),
                    );
                }
                for i in 1..actual.values.len().min(3) {
                    self.expect(&Type::Number.optional(), &actual.at(i), span.clone());
                }
                match actual.at(0) {
                    Type::Table(table) => {
                        if let Some((key, value)) = &table.index {
                            self.expect(&Type::Number, key, span.clone());
                            Pack::variadic((**value).clone().optional())
                        } else {
                            Pack::variadic(Type::Nil)
                        }
                    }
                    Type::Any => Pack::variadic(Type::Any),
                    Type::Unknown if self.mode == Mode::Gradual => Pack::variadic(Type::Unknown),
                    _ => {
                        self.error("T005", "unpack requires a table", span);
                        Pack::variadic(Type::Unknown)
                    }
                }
            }
            Type::IteratorFactory(_) => {
                let actual = self.expressions(arguments, &[], None);
                if actual.minimum != 1
                    || actual.values.len() != 1
                    || actual.tail.is_some()
                    || receiver.is_some()
                {
                    self.error("T006", "pairs/ipairs expects one table", span.clone());
                }
                let ty = actual.at(0);
                if !(matches!(ty, Type::Table(_) | Type::Any)
                    || ty == Type::Unknown && self.mode == Mode::Gradual)
                {
                    self.error("T005", "pairs/ipairs requires a table", span);
                }
                Pack::variadic(Type::Unknown)
            }
            Type::Any | Type::Unknown => {
                self.expressions(arguments, &[], None);
                if *ty == Type::Unknown && self.mode == Mode::Strict {
                    self.error("T007", "cannot call unknown", span);
                }
                Pack::variadic(ty.clone())
            }
            other => {
                self.expressions(arguments, &[], None);
                self.error("T007", format!("cannot call {other}"), span);
                Pack::variadic(Type::Unknown)
            }
        };
        self.reset();
        result
    }
    fn check_redeclaration(&mut self, binding: &Binding) {
        if self
            .scopes
            .last()
            .unwrap()
            .get(&binding.name)
            .is_some_and(|v| v.readonly)
        {
            self.error(
                "T012",
                format!(
                    "cannot redeclare imported binding '{}' in the same scope",
                    binding.name
                ),
                binding.span.clone(),
            );
        }
    }
    fn check_readonly(&mut self, target: &Expr) {
        if let ExprKind::Name(name) = &target.kind
            && self.lookup(name).is_some_and(|v| v.readonly)
        {
            self.error(
                "T012",
                format!("cannot assign to imported binding '{name}'"),
                target.span.clone(),
            );
        }
    }
    fn assignment_type(&mut self, target: &Expr) -> Type {
        self.check_readonly(target);
        match &target.kind {
            ExprKind::Name(name) => self
                .lookup(name)
                .map(|v| v.declared.clone())
                .unwrap_or_else(|| {
                    if self.mode == Mode::Strict {
                        self.error(
                            "T003",
                            format!("unknown variable '{name}'"),
                            target.span.clone(),
                        );
                    }
                    Type::Unknown
                }),
            ExprKind::Field(object, name) => {
                let ty = self.expr(object);
                self.access(&ty, &Type::String, Some(name), true, target.span.clone())
            }
            ExprKind::Index(object, key) => {
                let ty = self.expr(object);
                let key_type = self.expr(key);
                let name = if let ExprKind::String(s) = &key.kind {
                    Some(s.as_slice())
                } else {
                    None
                };
                self.access(&ty, &key_type, name, true, target.span.clone())
            }
            _ => Type::Unknown,
        }
    }
    fn access(
        &mut self,
        ty: &Type,
        key: &Type,
        name: Option<&[u8]>,
        write: bool,
        span: Range<usize>,
    ) -> Type {
        if *ty == Type::Any {
            return Type::Any;
        }
        if *ty == Type::Unknown && self.mode == Mode::Gradual {
            return Type::Unknown;
        }
        let Type::Table(table) = ty else {
            self.error(
                "T005",
                format!("cannot index {ty}; check for nil before accessing a table"),
                span,
            );
            return Type::Unknown;
        };
        if let Some(name) = name
            && let Some(value) = table.fields.get(name)
        {
            return value.clone();
        }
        if let Some((index, value)) = &table.index {
            self.expect(index, key, span.clone());
            if write
                && *index.as_ref() == Type::String
                && name.is_none()
                && table.fields.values().any(|t| t != value.as_ref())
            {
                self.error(
                    "T005",
                    "dynamic writes require a uniform table value type",
                    span,
                );
            }
            return if write {
                (**value).clone()
            } else {
                (**value).clone().optional()
            };
        }
        if *key == Type::String && name.is_none() && !write {
            return table
                .fields
                .values()
                .cloned()
                .fold(Type::Never, Type::join)
                .optional();
        }
        if self.mode == Mode::Strict {
            self.error(
                "T005",
                "unknown field or index; declare an index type for a growable table",
                span.clone(),
            );
        }
        // A dynamic write into a closed record could overwrite a known field.
        if write && name.is_none() && !table.fields.is_empty() {
            let mut types = table.fields.values();
            let first = types.next().unwrap().clone();
            if types.any(|t| *t != first) {
                self.error(
                    "T005",
                    "dynamic writes into heterogeneous records are not supported",
                    span,
                );
            }
            return first;
        }
        Type::Unknown
    }
    fn table(
        &mut self,
        fields: &[TableField],
        expected: Option<&TableType>,
        span: Range<usize>,
    ) -> Type {
        let mut table = TableType::default();
        for (position, field) in fields.iter().enumerate() {
            let (key, name, value) = match field {
                TableField::Named(name, value) => (Type::String, Some(name.clone()), value),
                TableField::List(value) => (Type::Number, None, value),
                TableField::Indexed(key, value) => {
                    let ty = self.expr(key);
                    if matches!(ty, Type::Nil | Type::Optional(_)) {
                        self.error("T005", "table keys cannot be nil", key.span.clone());
                    }
                    let name = if let ExprKind::String(name) = &key.kind {
                        Some(name.clone())
                    } else {
                        None
                    };
                    (ty, name, value)
                }
            };
            let target = expected.and_then(|t| {
                name.as_ref()
                    .and_then(|n| t.fields.get(n))
                    .or_else(|| t.index.as_ref().map(|(_, v)| v.as_ref()))
            });
            if matches!(field, TableField::List(_))
                && position + 1 == fields.len()
                && matches!(
                    value.kind,
                    ExprKind::Call(..) | ExprKind::MethodCall(..) | ExprKind::Varargs
                )
            {
                let pack = self.expansion(value);
                let mut element = Type::Never;
                for ty in pack.values.iter().chain(pack.tail.as_deref()) {
                    if let Some(target) = target {
                        self.expect(target, ty, value.span.clone());
                    }
                    element = element.join(ty.clone());
                }
                if element != Type::Never {
                    if let Some(expected) = expected {
                        if let Some((index, _)) = &expected.index {
                            self.expect(index, &Type::Number, value.span.clone());
                        } else {
                            self.error(
                                "T005",
                                "list values are absent from the declared record",
                                value.span.clone(),
                            );
                        }
                    }
                    if let Some((old_key, old_value)) = table.index.take() {
                        table.index = Some((
                            Box::new(old_key.join(Type::Number)),
                            Box::new(old_value.join(element)),
                        ));
                    } else {
                        table.index = Some((Box::new(Type::Number), Box::new(element)));
                    }
                }
                continue;
            }
            let actual = self.expr_expected(value, target);
            if let Some(expected) = expected {
                if let Some((index, _)) = &expected.index {
                    self.expect(index, &key, value.span.clone());
                }
                if let Some(target) = target {
                    self.expect(target, &actual, value.span.clone());
                } else {
                    self.error(
                        "T005",
                        "field is absent from the declared table type",
                        value.span.clone(),
                    );
                }
            }
            if let Some(name) = name {
                if table.fields.insert(name, actual).is_some() {
                    self.error("T005", "duplicate constant table key", value.span.clone());
                }
            } else if let Some((old_key, old_value)) = table.index.take() {
                table.index = Some((
                    Box::new((*old_key).join(key)),
                    Box::new((*old_value).join(actual)),
                ));
            } else {
                table.index = Some((Box::new(key), Box::new(actual)));
            }
        }
        if let Some(expected) = expected {
            for (name, ty) in &expected.fields {
                if !table.fields.contains_key(name) {
                    self.expect(ty, &Type::Nil, span.clone());
                }
            }
            return Type::Table(expected.clone().into());
        }
        if let Some((key, value)) = &mut table.index
            && self.accepts(key, &Type::String)
        {
            let joined = table
                .fields
                .values()
                .cloned()
                .fold((**value).clone(), Type::join);
            **value = joined.clone();
            for field in table.fields.values_mut() {
                *field = field.clone().join(joined.clone());
            }
        }
        Type::Table(table.into())
    }
    fn iterator(&mut self, expr: &Expr) -> (Type, Type) {
        if let ExprKind::Call(function, args) = &expr.kind {
            let function_type = self.expr(function);
            if let Type::IteratorFactory(array) = function_type {
                let arguments = self.expressions(args, &[], None);
                if arguments.minimum != 1 || arguments.values.len() != 1 || arguments.tail.is_some()
                {
                    self.error("T006", "pairs/ipairs expects one table", expr.span.clone());
                }
                let table = arguments.at(0);
                self.reset();
                if let Type::Table(table) = table {
                    if array {
                        if let Some((key, value)) = &table.index {
                            self.expect(&Type::Number, key, expr.span.clone());
                            return (Type::Number, value.without_nil());
                        }
                        return (Type::Number, Type::Never);
                    }
                    let key = if table.fields.is_empty() {
                        Type::Never
                    } else {
                        Type::String
                    };
                    let value = table.fields.values().cloned().fold(Type::Never, Type::join);
                    return if let Some((k, v)) = &table.index {
                        (key.join((**k).clone()), value.join(v.without_nil()))
                    } else {
                        (key, value.without_nil())
                    };
                }
                if table == Type::Any {
                    return (Type::Any, Type::Any);
                }
                if table != Type::Unknown || self.mode == Mode::Strict {
                    self.error("T005", "pairs/ipairs requires a table", expr.span.clone());
                }
                return (Type::Unknown, Type::Unknown);
            }
        }
        let ty = self.expr(expr);
        if ty == Type::Any {
            return (Type::Any, Type::Any);
        }
        if self.mode == Mode::Strict {
            self.error("T010","typed generic for currently supports pairs/ipairs; use an explicit any boundary for custom iterators",expr.span.clone());
        }
        (Type::Unknown, Type::Unknown)
    }
}
