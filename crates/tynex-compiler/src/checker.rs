use crate::{Diagnostic, Import, Interface, Mode, ast::*};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ops::Range,
    sync::atomic::{AtomicU64, Ordering},
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
    nominals: BTreeMap<u64, NominalType>,
    self_type: Option<Type>,
    async_body: bool,
}

fn callable_results(is_async: bool, result: Pack) -> Pack {
    if is_async {
        Pack::one(Type::Task(Box::new(result)))
    } else {
        result
    }
}
fn current_type(declared: &Type, actual: Type) -> Type {
    if *declared == Type::Any || matches!(actual, Type::Any | Type::Unknown) {
        return declared.clone();
    }
    // A trusted dynamic Task adapter must not leak any back through an
    // explicit Task<T> boundary. Retain only the useful non-nil refinement.
    if matches!(actual, Type::Task(_)) {
        if matches!(declared, Type::Task(_)) {
            return declared.clone();
        }
        if let Type::Optional(inner) = declared
            && matches!(inner.as_ref(), Type::Task(_))
        {
            return (**inner).clone();
        }
    }
    actual
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
        nominals: modules.values().flat_map(|m| m.nominals.clone()).collect(),
        self_type: None,
        async_body: false,
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
            "scheduler",
            library(vec![
                ("run", Type::AsyncRun),
                ("step", Type::AsyncStep),
                ("pending", function(vec![], Type::Number)),
                (
                    "from_worker",
                    function(vec![Type::Any], Type::Task(Box::new(Pack::one(Type::Any)))),
                ),
            ]),
        ),
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
        interface: Interface {
            value,
            nominals: c.nominals,
        },
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
fn substitute_self(ty: &Type, id: u64, replacement: &Type) -> Type {
    match ty {
        Type::AbstractSelf(found, _) if *found == id => replacement.clone(),
        Type::Optional(t) => substitute_self(t, id, replacement).optional(),
        Type::Receiver(t) => Type::Receiver(Box::new(substitute_self(t, id, replacement))),
        Type::Union(ts) => Type::Union(
            ts.iter()
                .map(|t| substitute_self(t, id, replacement))
                .collect(),
        ),
        Type::Table(t) => Type::Table(
            TableType {
                fields: t
                    .fields
                    .iter()
                    .map(|(k, v)| (k.clone(), substitute_self(v, id, replacement)))
                    .collect(),
                index: t.index.as_ref().map(|(k, v)| {
                    (
                        Box::new(substitute_self(k, id, replacement)),
                        Box::new(substitute_self(v, id, replacement)),
                    )
                }),
            }
            .into(),
        ),
        Type::Function(args, result, tail) => Type::Function(
            args.iter()
                .map(|t| substitute_self(t, id, replacement))
                .collect(),
            Box::new(Pack {
                values: result
                    .values
                    .iter()
                    .map(|t| substitute_self(t, id, replacement))
                    .collect(),
                tail: result
                    .tail
                    .as_ref()
                    .map(|t| Box::new(substitute_self(t, id, replacement))),
                minimum: result.minimum,
            }),
            tail.as_ref()
                .map(|t| Box::new(substitute_self(t, id, replacement))),
        ),
        Type::Task(result) => Type::Task(Box::new(Pack {
            values: result
                .values
                .iter()
                .map(|t| substitute_self(t, id, replacement))
                .collect(),
            tail: result
                .tail
                .as_ref()
                .map(|t| Box::new(substitute_self(t, id, replacement))),
            minimum: result.minimum,
        })),
        other => other.clone(),
    }
}
fn mentions_self(ty: &Type, id: u64) -> bool {
    substitute_self(ty, id, &Type::Never) != *ty
}
// Direct Self results can be hidden behind a trait. Mutable containers and
// callbacks retain invariance/parameter obligations and must keep concrete Self.
fn safe_abstract_result(ty: &Type, id: u64) -> bool {
    match ty {
        Type::AbstractSelf(found, _) if *found == id => true,
        Type::Optional(inner) => safe_abstract_result(inner, id),
        Type::Union(types) => types.iter().all(|t| safe_abstract_result(t, id)),
        Type::Task(result) => result
            .values
            .iter()
            .chain(result.tail.as_deref())
            .all(|t| safe_abstract_result(t, id)),
        other => !mentions_self(other, id),
    }
}
impl Checker<'_> {
    fn named_value(&self, name: &str) -> Option<Type> {
        let mut parts = name.split('.');
        let mut value = self.lookup(parts.next()?)?.declared.clone();
        for part in parts {
            let Type::Table(table) = value else {
                return None;
            };
            value = table.fields.get(part.as_bytes())?.clone();
        }
        Some(value)
    }
    fn resolve(&mut self, ty: &Type, span: Range<usize>) -> Type {
        match ty {
            Type::Named(name) if name == "Self" => self.self_type.clone().unwrap_or_else(|| {
                self.error("T013", "Self is only valid inside a class or trait", span);
                Type::Unknown
            }),
            Type::Named(name) => match self.named_value(name) {
                Some(Type::Definition(id, label)) => Type::Instance(id, label),
                _ => {
                    self.error(
                        "T013",
                        format!("unknown class or trait type '{name}'"),
                        span,
                    );
                    Type::Unknown
                }
            },
            Type::Optional(inner) => self.resolve(inner, span).optional(),
            Type::Receiver(inner) => Type::Receiver(Box::new(self.resolve(inner, span))),
            Type::Union(types) => Type::Union(
                types
                    .iter()
                    .map(|t| self.resolve(t, span.clone()))
                    .collect(),
            ),
            Type::Table(table) => Type::Table(
                TableType {
                    fields: table
                        .fields
                        .iter()
                        .map(|(k, v)| (k.clone(), self.resolve(v, span.clone())))
                        .collect(),
                    index: table.index.as_ref().map(|(k, v)| {
                        (
                            Box::new(self.resolve(k, span.clone())),
                            Box::new(self.resolve(v, span.clone())),
                        )
                    }),
                }
                .into(),
            ),
            Type::Function(args, result, tail) => Type::Function(
                args.iter().map(|t| self.resolve(t, span.clone())).collect(),
                Box::new(self.resolve_pack(result, span.clone())),
                tail.as_ref().map(|t| Box::new(self.resolve(t, span))),
            ),
            Type::Task(result) => Type::Task(Box::new(self.resolve_pack(result, span))),
            other => other.clone(),
        }
    }
    fn resolve_pack(&mut self, pack: &Pack, span: Range<usize>) -> Pack {
        Pack {
            values: pack
                .values
                .iter()
                .map(|t| self.resolve(t, span.clone()))
                .collect(),
            tail: pack
                .tail
                .as_ref()
                .map(|t| Box::new(self.resolve(t, span.clone()))),
            minimum: pack.minimum,
        }
    }
    fn nominal(&mut self, binding: &Binding, is_trait: bool) -> u64 {
        if matches!(
            binding.name.as_str(),
            "number" | "string" | "boolean" | "any" | "Self" | "Task"
        ) {
            self.error(
                "T014",
                format!(
                    "'{}' is reserved in class and trait type declarations",
                    binding.name
                ),
                binding.span.clone(),
            );
        }
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        self.check_redeclaration(binding);
        let value = Type::Definition(id, binding.name.clone());
        self.insert(&binding.name, value.clone(), value);
        self.lookup_mut(&binding.name).unwrap().readonly = true;
        self.nominals.insert(
            id,
            NominalType {
                name: binding.name.clone(),
                is_trait,
                fields: BTreeMap::new(),
                methods: BTreeMap::new(),
                required_fields: HashSet::new(),
                defaults: HashSet::new(),
                traits: vec![],
            },
        );
        id
    }
    fn class(&mut self, declaration: &ClassDecl) {
        // Resolve implemented traits before introducing the class's local name.
        let mut traits = vec![];
        for binding in &declaration.traits {
            match self.named_value(&binding.name) {
                Some(Type::Definition(id, _))
                    if self.nominals.get(&id).is_some_and(|t| t.is_trait) =>
                {
                    if traits.contains(&id) {
                        self.error("T014", "duplicate implemented trait", binding.span.clone());
                    } else {
                        traits.push(id);
                    }
                }
                _ => self.error(
                    "T014",
                    format!("'{}' is not a declared trait", binding.name),
                    binding.span.clone(),
                ),
            }
        }
        let id = self.nominal(&declaration.name, false);
        let instance = Type::Instance(id, declaration.name.name.clone());
        let previous_self = self.self_type.replace(instance.clone());
        self.nominals.get_mut(&id).unwrap().traits = traits.clone();
        let mut names = HashSet::new();
        let mut declarations = Vec::new();
        // Publish the entire constructor contract before checking deferred
        // closures that may construct this same class recursively.
        for field in &declaration.fields {
            let binding = &field.binding;
            let key = binding.name.as_bytes().to_vec();
            if !names.insert(key.clone()) {
                self.error("T014", "duplicate class member", binding.span.clone());
            }
            let declared = binding
                .annotation
                .as_ref()
                .map(|t| self.resolve(t, binding.span.clone()));
            let ty = declared.clone().unwrap_or(Type::Unknown);
            let required = field.default.is_none() && !self.accepts(&ty, &Type::Nil);
            let nominal = self.nominals.get_mut(&id).unwrap();
            nominal.fields.insert(key.clone(), ty);
            if required {
                nominal.required_fields.insert(key);
            }
            declarations.push(declared);
        }
        // Seed every signature before checking bodies so self recursion and
        // calls to later methods see the same class identity.
        for method in &declaration.methods {
            let key = method.name.name.as_bytes().to_vec();
            if !names.insert(key.clone()) {
                self.error("T014", "duplicate class member", method.name.span.clone());
            }
            let ty = self.signature(&method.function, Some(&instance));
            self.nominals.get_mut(&id).unwrap().methods.insert(key, ty);
        }
        let own: HashSet<_> = declaration
            .methods
            .iter()
            .map(|m| m.name.name.as_bytes().to_vec())
            .collect();
        let mut inherited = HashSet::new();
        for trait_id in &traits {
            let contract = self.nominals[trait_id].clone();
            for key in &contract.defaults {
                if own.contains(key) {
                    continue;
                }
                if self.nominals[&id].fields.contains_key(key) {
                    self.error(
                        "T014",
                        format!(
                            "field '{}' conflicts with a trait method",
                            String::from_utf8_lossy(key)
                        ),
                        declaration.name.span.clone(),
                    );
                }
                if !inherited.insert(key.clone()) {
                    self.error(
                        "T014",
                        format!(
                            "conflicting trait defaults for '{}'; implement it in the class",
                            String::from_utf8_lossy(key)
                        ),
                        declaration.name.span.clone(),
                    );
                }
                let ty = substitute_self(&contract.methods[key], *trait_id, &instance);
                self.nominals
                    .get_mut(&id)
                    .unwrap()
                    .methods
                    .insert(key.clone(), ty);
            }
        }
        for (field, declared) in declaration.fields.iter().zip(declarations) {
            let binding = &field.binding;
            let saved = self.scopes.clone();
            let old_async = std::mem::replace(&mut self.async_body, false);
            // Defaults run after captured locals may have changed, so they
            // cannot depend on declaration-time flow refinements.
            self.reset();
            let actual = field
                .default
                .as_ref()
                .map(|e| self.expr_expected(e, declared.as_ref()));
            self.scopes = saved;
            self.async_body = old_async;
            if let (Some(declared), Some(actual)) = (&declared, &actual) {
                self.expect(declared, actual, binding.span.clone());
            }
            let ty = declared.or(actual).unwrap_or(Type::Unknown);
            if ty.has_unknown() && self.mode == Mode::Strict {
                self.error(
                    "T002",
                    format!(
                        "cannot infer field '{}'; add a type or default",
                        binding.name
                    ),
                    binding.span.clone(),
                );
            }
            self.nominals
                .get_mut(&id)
                .unwrap()
                .fields
                .insert(binding.name.as_bytes().to_vec(), ty);
        }
        for method in &declaration.methods {
            let ty = self.function(&method.function, None, Some(&instance));
            self.nominals
                .get_mut(&id)
                .unwrap()
                .methods
                .insert(method.name.name.as_bytes().to_vec(), ty);
        }
        for trait_id in traits {
            let contract = self.nominals[&trait_id].clone();
            for (key, expected) in contract.methods {
                let expected = substitute_self(&expected, trait_id, &instance);
                match self.nominals[&id].methods.get(&key).cloned() {
                    Some(actual) if self.method_accepts(&expected, &actual) => {}
                    Some(_) => self.error(
                        "T014",
                        format!(
                            "method '{}' does not satisfy trait '{}'",
                            String::from_utf8_lossy(&key),
                            contract.name
                        ),
                        declaration.name.span.clone(),
                    ),
                    None => self.error(
                        "T014",
                        format!(
                            "missing method '{}' required by trait '{}'",
                            String::from_utf8_lossy(&key),
                            contract.name
                        ),
                        declaration.name.span.clone(),
                    ),
                }
            }
        }
        self.self_type = previous_self;
    }
    fn trait_declaration(&mut self, declaration: &TraitDecl) {
        let id = self.nominal(&declaration.name, true);
        let receiver = Type::AbstractSelf(id, declaration.name.name.clone());
        let previous_self = self.self_type.replace(receiver.clone());
        let mut names = HashSet::new();
        for binding in &declaration.required {
            let key = binding.name.as_bytes().to_vec();
            if !names.insert(key.clone()) {
                self.error("T014", "duplicate trait member", binding.span.clone());
            }
            let ty = self.resolve(
                binding.annotation.as_ref().unwrap_or(&Type::Unknown),
                binding.span.clone(),
            );
            if !matches!(&ty,Type::Function(args,_,_) if args.first() == Some(&receiver)) {
                self.error(
                    "T014",
                    "trait requirements must be functions with Self as their first parameter",
                    binding.span.clone(),
                );
            }
            self.nominals.get_mut(&id).unwrap().methods.insert(key, ty);
        }
        for method in &declaration.methods {
            let key = method.name.name.as_bytes().to_vec();
            if !names.insert(key.clone()) {
                self.error("T014", "duplicate trait member", method.name.span.clone());
            }
            let ty = self.signature(&method.function, Some(&receiver));
            let nominal = self.nominals.get_mut(&id).unwrap();
            nominal.methods.insert(key.clone(), ty);
            nominal.defaults.insert(key);
        }
        for method in &declaration.methods {
            let ty = self.function(&method.function, None, Some(&receiver));
            self.nominals
                .get_mut(&id)
                .unwrap()
                .methods
                .insert(method.name.name.as_bytes().to_vec(), ty);
        }
        self.self_type = previous_self;
    }
    fn method_accepts(&self, expected: &Type, actual: &Type) -> bool {
        let (Type::Function(want, wr, wt), Type::Function(got, gr, gt)) = (expected, actual) else {
            return false;
        };
        let unwrap = |t: &Type| {
            if let Type::Receiver(inner) = t {
                (**inner).clone()
            } else {
                t.clone()
            }
        };
        want.len() == got.len()
            && wt == gt
            && want.iter().zip(got).all(|(a, b)| {
                self.accepts(&unwrap(a), &unwrap(b)) && self.accepts(&unwrap(b), &unwrap(a))
            })
            && wr.values.len() == gr.values.len()
            && wr.tail == gr.tail
            && wr
                .values
                .iter()
                .zip(&gr.values)
                .all(|(a, b)| self.accepts(a, b))
    }
    fn construct(
        &mut self,
        id: u64,
        arguments: &[Expr],
        receiver: Option<Type>,
        span: Range<usize>,
    ) -> Pack {
        let nominal = self.nominals[&id].clone();
        if nominal.is_trait {
            self.expressions(arguments, &[], None);
            self.error("T014", "traits cannot be constructed", span);
            return Pack::one(Type::Unknown);
        }
        let initializer = TableType {
            fields: nominal
                .fields
                .iter()
                .map(|(key, ty)| {
                    (
                        key.clone(),
                        if nominal.required_fields.contains(key) {
                            ty.clone()
                        } else {
                            ty.clone().optional()
                        },
                    )
                })
                .collect(),
            index: None,
        };
        let expected = Type::Table(initializer.clone().into());
        let actual = self.expressions(arguments, &[Some(expected)], None);
        let dynamic = actual.tail.as_ref().is_some_and(|t| {
            **t == Type::Any || **t == Type::Unknown && self.mode == Mode::Gradual
        });
        if receiver.is_some()
            || actual.values.len() > 1
            || !dynamic
                && (actual.minimum != 1 || actual.values.len() != 1 || actual.tail.is_some())
        {
            self.error("T006","class construction requires exactly one initializer table after Lua argument expansion",span.clone());
        }
        match actual.at(0) {
            Type::Table(table) => {
                for (key, ty) in &table.fields {
                    if let Some(expected) = initializer.fields.get(key) {
                        self.expect(expected, ty, span.clone());
                    } else {
                        self.error(
                            "T014",
                            format!(
                                "unknown initializer field '{}'",
                                String::from_utf8_lossy(key)
                            ),
                            span.clone(),
                        );
                    }
                }
                for key in &nominal.required_fields {
                    if !table.fields.contains_key(key) {
                        self.error(
                            "T014",
                            format!("missing required field '{}'", String::from_utf8_lossy(key)),
                            span.clone(),
                        );
                    }
                }
                if table.index.is_some() {
                    self.error(
                        "T014",
                        "class initializers require named, statically known fields",
                        span.clone(),
                    );
                }
            }
            Type::Any => {}
            Type::Unknown if self.mode == Mode::Gradual => {}
            _ => self.error(
                "T014",
                "class construction requires an initializer table",
                span,
            ),
        }
        Pack::one(Type::Instance(id, nominal.name))
    }
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
        if let (Type::Task(expected), Type::Task(actual)) = (expected, actual) {
            return self.accepts_task_pack(expected, actual);
        }
        if let Type::Instance(expected_id, _) = expected {
            let actual_id = match actual {
                Type::Instance(id, _) | Type::AbstractSelf(id, _) => Some(*id),
                _ => None,
            };
            if let Some(actual_id) = actual_id {
                return *expected_id == actual_id
                    || self.nominals.get(expected_id).is_some_and(|t| t.is_trait)
                        && self
                            .nominals
                            .get(&actual_id)
                            .is_some_and(|t| t.traits.contains(expected_id));
            }
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
    fn accepts_task_pack(&self, expected: &Pack, actual: &Pack) -> bool {
        if actual.values == [Type::Never] {
            return true;
        }
        let dynamic = actual.tail.as_ref().is_some_and(|t| {
            **t == Type::Any || **t == Type::Unknown && self.mode == Mode::Gradual
        });
        // A Task exposes its complete result pack via await/result/run. Unlike
        // assigning ordinary Lua locals, covariance cannot manufacture missing
        // return slots, even when their value types happen to include nil.
        if !dynamic && actual.minimum < expected.minimum {
            return false;
        }
        if expected.tail.is_none()
            && (actual.values.len() > expected.values.len() || actual.tail.is_some() && !dynamic)
        {
            return false;
        }
        if !expected
            .values
            .iter()
            .enumerate()
            .all(|(index, ty)| self.accepts(ty, &actual.at(index)))
        {
            return false;
        }
        if let Some(tail) = &expected.tail {
            return actual
                .values
                .iter()
                .skip(expected.values.len())
                .chain(actual.tail.as_deref())
                .all(|ty| self.accepts(tail, ty));
        }
        true
    }
    fn task_results(&mut self, ty: &Type, span: Range<usize>) -> Pack {
        match ty {
            Type::Task(result) => (**result).clone(),
            Type::Any => Pack::variadic(Type::Any),
            Type::Unknown if self.mode == Mode::Gradual => Pack::variadic(Type::Unknown),
            Type::Never => Pack::one(Type::Never),
            Type::Union(types) => types
                .iter()
                .map(|ty| self.task_results(ty, span.clone()))
                .fold(Pack::one(Type::Never), Pack::join),
            _ => {
                self.error(
                    "T016",
                    format!("expected an async Task, got {ty}; await requires a non-nil Task"),
                    span,
                );
                Pack::variadic(Type::Unknown)
            }
        }
    }
    fn expect(&mut self, expected: &Type, actual: &Type, span: Range<usize>) {
        if !self.accepts(expected, actual) {
            self.error("T001", format!("expected {expected}, got {actual}"), span);
        }
    }
    fn bind(&mut self, binding: &Binding, actual: Type) {
        let declared = binding
            .annotation
            .as_ref()
            .map(|t| self.resolve(t, binding.span.clone()))
            .unwrap_or_else(|| actual.clone());
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
        let current = current_type(&declared, actual);
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
            Stmt::Class(declaration) => self.class(declaration),
            Stmt::Trait(declaration) => self.trait_declaration(declaration),
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
                let expected: Vec<_> = bindings
                    .iter()
                    .map(|b| {
                        b.annotation
                            .as_ref()
                            .map(|t| self.resolve(t, b.span.clone()))
                    })
                    .collect();
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
                        variable.current = current_type(&variable.declared, actual);
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
    fn signature(&mut self, function: &Function, receiver: Option<&Type>) -> Type {
        let mut params: Vec<_> = function
            .params
            .iter()
            .map(|p| {
                p.annotation
                    .as_ref()
                    .map(|t| self.resolve(t, p.span.clone()))
                    .unwrap_or(Type::Unknown)
            })
            .collect();
        if let Some(receiver) = receiver {
            params.insert(0, Type::Receiver(Box::new(receiver.clone())));
        }
        let returns = function
            .returns
            .as_ref()
            .map(|p| self.resolve_pack(p, function.span.clone()))
            .unwrap_or_else(|| Pack::variadic(Type::Unknown));
        Type::Function(
            params,
            Box::new(callable_results(function.is_async, returns)),
            function.variadic.as_ref().map(|b| {
                Box::new(
                    b.annotation
                        .as_ref()
                        .map(|t| self.resolve(t, b.span.clone()))
                        .unwrap_or(Type::Unknown),
                )
            }),
        )
    }
    fn function(
        &mut self,
        function: &Function,
        recursive: Option<&str>,
        receiver: Option<&Type>,
    ) -> Type {
        let Type::Function(params, signature_returns, variadic) =
            self.signature(function, receiver)
        else {
            unreachable!()
        };
        // Async call sites see one Task; annotations still describe the body
        // result pack, which is checked before wrapping it for callers.
        let signature_returns = if function.is_async {
            let Type::Task(result) = &signature_returns.values[0] else {
                unreachable!()
            };
            result.clone()
        } else {
            signature_returns
        };
        // Resolve the complete signature in its declaration scope before any
        // parameter can shadow a class/trait name used by another annotation.
        let contract = function.returns.as_ref().map(|_| *signature_returns);
        let saved = self.scopes.clone();
        let old_varargs = self.varargs.clone();
        let old_async = std::mem::replace(&mut self.async_body, function.is_async);
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
        let parameter_types = params
            .iter()
            .skip(usize::from(receiver.is_some()))
            .cloned()
            .chain(variadic.as_deref().cloned());
        for (param, ty) in function
            .params
            .iter()
            .chain(function.variadic.iter())
            .zip(parameter_types)
        {
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
        let flow = self.body(&function.body, contract.as_ref());
        let mut result = flow
            .returns
            .into_iter()
            .fold(Pack::one(Type::Never), Pack::join);
        if flow.falls {
            if let Some(expected) = &contract {
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
        if let Some(contract) = &contract {
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
        self.async_body = old_async;
        self.conditional = old_conditional;
        Type::Function(
            params,
            Box::new(callable_results(function.is_async, result)),
            variadic,
        )
    }
    fn expect_returns(&mut self, expected: &Pack, actual: &Pack, span: Range<usize>) {
        if actual.values == [Type::Never] {
            return;
        }
        let dynamic = actual.tail.as_ref().is_some_and(|t| {
            **t == Type::Any || **t == Type::Unknown && self.mode == Mode::Gradual
        });
        if self.async_body && !dynamic && actual.minimum < expected.minimum {
            self.error("T006","async return annotations require every fixed result slot; return nil explicitly for a nullable result",span.clone());
        }
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
            ExprKind::Call(..)
            | ExprKind::MethodCall(..)
            | ExprKind::Await(..)
            | ExprKind::OptionalChain(..)
            | ExprKind::Varargs => self.expansion(expr).at(0),
        };
        self.depth -= 1;
        result
    }
    fn expands(expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Call(..)
            | ExprKind::MethodCall(..)
            | ExprKind::Await(..)
            | ExprKind::Varargs => true,
            ExprKind::OptionalChain(_, steps) => steps.last().is_some_and(|step| {
                matches!(
                    step.kind,
                    ChainStepKind::Call(_) | ChainStepKind::MethodCall(..)
                )
            }),
            _ => false,
        }
    }
    fn may_be_nil(ty: &Type) -> bool {
        match ty {
            Type::Nil | Type::Optional(_) | Type::Any | Type::Unknown => true,
            Type::Union(types) => types.iter().any(Self::may_be_nil),
            _ => false,
        }
    }
    fn declared_chain_base(&self, expr: &Expr) -> Option<Type> {
        if let ExprKind::Group(inner) = &expr.kind {
            self.declared_chain_base(inner)
        } else {
            self.defined_type(expr)
        }
    }
    fn optional_chain(&mut self, base: &Expr, steps: &[ChainStep]) -> Pack {
        let declared = self.declared_chain_base(base);
        let mut value = self.expr(base);
        let mut reachable = value != Type::Never;
        let mut skipped = false;
        let mut result = Pack::one(value.clone());
        for (index, step) in steps.iter().enumerate() {
            if step.optional {
                skipped |= reachable && Self::may_be_nil(&value);
                value = value.without_nil();
                if value == Type::Never {
                    reachable = false;
                    // Keep checking a known contract even when the receiver
                    // is currently nil. This is the same policy as dead bodies.
                    if index == 0
                        && let Some(declared) = &declared
                    {
                        value = declared.without_nil();
                    }
                }
            }
            result = if value == Type::Never {
                // A literal nil has no member/call contract, but expressions
                // inside its skipped suffix still receive normal diagnostics.
                match &step.kind {
                    ChainStepKind::Index(key) => {
                        self.expr(key);
                    }
                    ChainStepKind::Call(args) | ChainStepKind::MethodCall(_, args) => {
                        self.expressions(args, &[], None);
                    }
                    ChainStepKind::Field(_) => {}
                }
                Pack::one(Type::Never)
            } else {
                match &step.kind {
                    ChainStepKind::Field(name) => Pack::one(self.access(
                        &value,
                        &Type::String,
                        Some(name),
                        false,
                        step.span.clone(),
                    )),
                    ChainStepKind::Index(key) => {
                        let key_type = self.expr(key);
                        let name = Self::string_constant(key);
                        Pack::one(self.access(&value, &key_type, name, false, step.span.clone()))
                    }
                    ChainStepKind::Call(args) => self.call(&value, args, None, step.span.clone()),
                    ChainStepKind::MethodCall(name, args) => {
                        let method = if value == Type::String {
                            let library = self.scopes[0]["string"].declared.clone();
                            self.access(
                                &library,
                                &Type::String,
                                Some(name),
                                false,
                                step.span.clone(),
                            )
                        } else {
                            self.access_member(
                                &value,
                                &Type::String,
                                Some(name),
                                false,
                                true,
                                step.span.clone(),
                            )
                        };
                        self.call(&method, args, Some(value), step.span.clone())
                    }
                }
            };
            value = result.at(0);
        }
        if !reachable {
            result = Pack::one(Type::Never);
        }
        if skipped {
            result.join(Pack::one(Type::Nil))
        } else {
            result
        }
    }
    fn expressions(
        &mut self,
        expressions: &[Expr],
        expected: &[Option<Type>],
        tail: Option<&Type>,
    ) -> Pack {
        let mut out = Pack::default();
        for (i, expr) in expressions.iter().enumerate() {
            if i + 1 == expressions.len() && Self::expands(expr) {
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
            ExprKind::Await(value) => {
                let ty = self.expr(value);
                let result = self.task_results(&ty, expr.span.clone());
                self.reset();
                result
            }
            ExprKind::OptionalChain(base, steps) => self.optional_chain(base, steps),
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
                    self.access_member(
                        &receiver,
                        &Type::String,
                        Some(key),
                        false,
                        true,
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
            Type::Definition(id, _) => self.construct(*id, arguments, receiver, span),
            Type::AsyncRun => {
                if self.async_body {
                    self.error("T016","scheduler.run cannot pump the scheduler inside an async function; use await",span.clone());
                }
                let actual = self.expressions(arguments, &[], None);
                let dynamic = actual.tail.as_ref().is_some_and(|t| {
                    **t == Type::Any || **t == Type::Unknown && self.mode == Mode::Gradual
                });
                if receiver.is_some()
                    || actual.values.len() > 1
                    || !dynamic
                        && (actual.minimum != 1
                            || actual.values.len() != 1
                            || actual.tail.is_some())
                {
                    self.error(
                        "T006",
                        "scheduler.run requires exactly one Task",
                        span.clone(),
                    );
                }
                self.task_results(&actual.at(0), span)
            }
            Type::AsyncStep => {
                if self.async_body {
                    self.error("T016","scheduler.step cannot pump the scheduler inside an async function; use await",span.clone());
                }
                self.call(
                    &function(vec![Type::Number.optional()], Type::Boolean),
                    arguments,
                    receiver,
                    span,
                )
            }
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
                    "cannot redeclare immutable binding '{}' in the same scope",
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
                format!("cannot assign to immutable binding '{name}'"),
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
        self.access_member(ty, key, name, write, false, span)
    }
    fn access_member(
        &mut self,
        ty: &Type,
        key: &Type,
        name: Option<&[u8]>,
        write: bool,
        method_call: bool,
        span: Range<usize>,
    ) -> Type {
        if *ty == Type::Any {
            return Type::Any;
        }
        if *ty == Type::Unknown && self.mode == Mode::Gradual {
            return Type::Unknown;
        }
        if let Type::Task(result) = ty {
            if write {
                self.error("T016", "Task methods and state are immutable", span.clone());
            }
            let returns = match name {
                Some(b"ready" | b"cancel") => Some(Pack::one(Type::Boolean)),
                Some(b"status") => Some(Pack::one(Type::String)),
                Some(b"result") => Some((**result).clone()),
                _ => None,
            };
            if let Some(returns) = returns {
                return Type::Function(
                    vec![Type::Receiver(Box::new(ty.clone()))],
                    Box::new(returns),
                    None,
                );
            }
            self.error(
                "T016",
                "unknown Task member; supported methods are ready, result, cancel, and status",
                span,
            );
            return Type::Unknown;
        }
        if let Type::Instance(id, _) | Type::AbstractSelf(id, _) | Type::Definition(id, _) = ty {
            let nominal = self.nominals[id].clone();
            if matches!(ty, Type::Definition(..)) {
                self.error(
                    "T014",
                    "class and trait methods must be accessed through an instance",
                    span,
                );
                return Type::Unknown;
            }
            if let Some(name) = name {
                if !matches!(ty, Type::Definition(..))
                    && let Some(value) = nominal.fields.get(name)
                {
                    return value.clone();
                }
                if let Some(value) = nominal.methods.get(name) {
                    if write {
                        self.error(
                            "T014",
                            "class and trait methods are immutable",
                            span.clone(),
                        );
                    }
                    if nominal.is_trait && matches!(ty, Type::Instance(..)) {
                        if !method_call {
                            self.error("T015","trait methods require colon dispatch; extracting a method loses its concrete receiver",span);
                            return Type::Unknown;
                        }
                        if let Type::Function(args, result, tail) = value
                            && (args.iter().skip(1).any(|t| mentions_self(t, *id))
                                || tail.as_ref().is_some_and(|t| mentions_self(t, *id))
                                || result
                                    .values
                                    .iter()
                                    .chain(result.tail.as_deref())
                                    .any(|t| !safe_abstract_result(t, *id)))
                        {
                            self.error(
                                "T015",
                                "this Self contract requires a concrete class receiver",
                                span,
                            );
                            return Type::Unknown;
                        }
                        return substitute_self(value, *id, ty);
                    }
                    return value.clone();
                }
            }
            self.error(
                "T014",
                format!("unknown or inaccessible member of '{}'", nominal.name),
                span,
            );
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
                && Self::expands(value)
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
