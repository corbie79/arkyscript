use std::{
    collections::{BTreeMap, HashSet},
    ops::Range,
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Type {
    Named(String),
    Instance(u64, String),
    Definition(u64, String),
    AbstractSelf(u64, String),
    Number,
    String,
    Boolean,
    Socket,
    FileSystem,
    Network,
    Nil,
    Any,
    Unknown,
    Never,
    Optional(Box<Type>),
    Union(Vec<Type>),
    // Shared immutable snapshots avoid copying every earlier method receiver.
    Table(Arc<TableType>),
    Function(Vec<Type>, Box<Pack>, Option<Box<Type>>),
    Task(Box<Pack>),
    AsyncRun,
    AsyncStep,
    Receiver(Box<Type>),
    IteratorFactory(bool),
    Require,
    Select,
    Unpack,
}
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TableType {
    pub fields: BTreeMap<Vec<u8>, Type>,
    pub index: Option<(Box<Type>, Box<Type>)>,
}

/// Nominal references use IDs so recursive class contracts remain finite.
#[derive(Clone, Debug)]
pub struct NominalType {
    pub name: String,
    pub is_trait: bool,
    pub fields: BTreeMap<Vec<u8>, Type>,
    pub methods: BTreeMap<Vec<u8>, Type>,
    pub required_fields: HashSet<Vec<u8>>,
    pub defaults: HashSet<Vec<u8>>,
    pub traits: Vec<u64>,
}

/// A Lua value list, not a tuple value. Only its last expression can expand.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Pack {
    pub values: Vec<Type>,
    pub tail: Option<Box<Type>>,
    pub minimum: usize,
}
impl Pack {
    pub fn fixed(values: Vec<Type>) -> Self {
        let minimum = values.len();
        Self {
            values,
            tail: None,
            minimum,
        }
    }
    pub fn one(value: Type) -> Self {
        Self::fixed(vec![value])
    }
    pub fn variadic(value: Type) -> Self {
        Self {
            values: vec![],
            tail: Some(Box::new(value)),
            minimum: 0,
        }
    }
    pub fn at(&self, index: usize) -> Type {
        let value = self
            .values
            .get(index)
            .or(self.tail.as_deref())
            .cloned()
            .unwrap_or(Type::Nil);
        if index >= self.minimum {
            value.optional()
        } else {
            value
        }
    }
    pub fn has_unknown(&self) -> bool {
        self.values.iter().any(Type::has_unknown)
            || self.tail.as_ref().is_some_and(|t| t.has_unknown())
    }
    pub fn join(self, other: Self) -> Self {
        if self.values == [Type::Never] {
            return other;
        }
        if other.values == [Type::Never] {
            return self;
        }
        let count = self.values.len().max(other.values.len());
        let values = (0..count).map(|i| self.at(i).join(other.at(i))).collect();
        let tail = match (self.tail, other.tail) {
            (Some(a), Some(b)) => Some(Box::new(a.join(*b))),
            (a, b) => a.or(b),
        };
        Self {
            values,
            tail,
            minimum: self.minimum.min(other.minimum),
        }
    }
}
impl Type {
    pub fn has_unknown(&self) -> bool {
        self.unknown_in(&mut HashSet::new())
    }
    fn unknown_in(&self, visited: &mut HashSet<*const TableType>) -> bool {
        match self {
            Self::Unknown => true,
            Self::Optional(t) | Self::Receiver(t) => t.unknown_in(visited),
            Self::Union(types) => types.iter().any(|t| t.unknown_in(visited)),
            Self::Table(t) => {
                if !visited.insert(Arc::as_ptr(t)) {
                    return false;
                }
                t.fields.values().any(|t| t.unknown_in(visited))
                    || t.index
                        .as_ref()
                        .is_some_and(|(k, v)| k.unknown_in(visited) || v.unknown_in(visited))
            }
            Self::Function(args, result, tail) => {
                args.iter().any(|t| t.unknown_in(visited))
                    || result.values.iter().any(|t| t.unknown_in(visited))
                    || result.tail.as_ref().is_some_and(|t| t.unknown_in(visited))
                    || tail.as_ref().is_some_and(|t| t.unknown_in(visited))
            }
            Self::Task(result) => {
                result.values.iter().any(|t| t.unknown_in(visited))
                    || result.tail.as_ref().is_some_and(|t| t.unknown_in(visited))
            }
            _ => false,
        }
    }
    pub fn optional(self) -> Self {
        match self {
            Self::Any | Self::Unknown | Self::Nil | Self::Optional(_) => self,
            Self::Never => Self::Nil,
            other => Self::Optional(Box::new(other)),
        }
    }
    pub fn without_nil(&self) -> Self {
        match self {
            Self::Nil => Self::Never,
            Self::Optional(t) => (**t).clone(),
            Self::Union(types) => types
                .iter()
                .map(Self::without_nil)
                .fold(Self::Never, Self::join),
            _ => self.clone(),
        }
    }
    pub fn join(self, other: Self) -> Self {
        if self == other || other == Self::Never {
            return self;
        }
        if self == Self::Never {
            return other;
        }
        if self == Self::Any || other == Self::Any {
            return Self::Any;
        }
        if self == Self::Nil {
            return other.optional();
        }
        if other == Self::Nil {
            return self.optional();
        }
        if let Self::Optional(inner) = &self {
            return (**inner).clone().join(other.without_nil()).optional();
        }
        if let Self::Optional(inner) = &other {
            return self.join((**inner).clone()).optional();
        }
        let mut values = if let Self::Union(v) = self {
            v
        } else {
            vec![self]
        };
        for value in if let Self::Union(v) = other {
            v
        } else {
            vec![other]
        } {
            if !values.contains(&value) {
                values.push(value);
            }
        }
        Self::Union(values)
    }
}
impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Named(name) | Self::Instance(_, name) => f.write_str(name),
            Self::AbstractSelf(_, name) => write!(f, "Self ({name})"),
            Self::Definition(_, name) => write!(f, "type {name}"),
            Self::Task(result) => {
                f.write_str("Task<")?;
                for (index, ty) in result.values.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{ty}")?;
                }
                if let Some(tail) = &result.tail {
                    if !result.values.is_empty() {
                        f.write_str(", ")?;
                    }
                    write!(f, "...{tail}")?;
                }
                f.write_str(">")
            }
            Self::Optional(t) => write!(f, "{t}?"),
            Self::Receiver(t) => write!(f, "self {t}"),
            Self::Union(types) => {
                for (i, t) in types.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" | ")?;
                    }
                    write!(f, "{t}")?;
                }
                Ok(())
            }
            Self::Table(table) => {
                f.write_str("{")?;
                for (i, (key, value)) in table.fields.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{}: {value}", String::from_utf8_lossy(key))?;
                }
                if let Some((key, value)) = &table.index {
                    if !table.fields.is_empty() {
                        f.write_str(", ")?;
                    }
                    write!(f, "[{key}]: {value}")?;
                }
                f.write_str("}")
            }
            Self::Function(..)
            | Self::AsyncRun
            | Self::AsyncStep
            | Self::IteratorFactory(_)
            | Self::Require
            | Self::Select
            | Self::Unpack => f.write_str("function"),
            _ => f.write_str(match self {
                Self::Number => "number",
                Self::String => "string",
                Self::Boolean => "boolean",
                Self::Socket => "Socket",
                Self::FileSystem => "fs library",
                Self::Network => "net library",
                Self::Nil => "nil",
                Self::Any => "any",
                Self::Never => "never",
                _ => "unknown",
            }),
        }
    }
}
#[derive(Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Range<usize>,
}
#[derive(Debug)]
pub enum ExprKind {
    Literal(Type),
    String(Vec<u8>),
    Bool(bool),
    Name(String),
    Unary(String, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    MethodCall(Box<Expr>, Vec<u8>, Vec<Expr>),
    OptionalChain(Box<Expr>, Vec<ChainStep>),
    Await(Box<Expr>),
    Varargs,
    Field(Box<Expr>, Vec<u8>),
    Index(Box<Expr>, Box<Expr>),
    Group(Box<Expr>),
    Table(Vec<TableField>),
    Function(Box<Function>),
}
#[derive(Debug)]
pub struct ChainStep {
    pub kind: ChainStepKind,
    pub optional: bool,
    pub span: Range<usize>,
}
#[derive(Debug)]
pub enum ChainStepKind {
    Field(Vec<u8>),
    Index(Expr),
    Call(Vec<Expr>),
    MethodCall(Vec<u8>, Vec<Expr>),
}
#[derive(Debug)]
pub enum TableField {
    Named(Vec<u8>, Expr),
    Indexed(Expr, Expr),
    List(Expr),
}
#[derive(Debug)]
pub struct Binding {
    pub name: String,
    pub annotation: Option<Type>,
    pub span: Range<usize>,
}
#[derive(Debug)]
pub struct Function {
    pub is_async: bool,
    pub params: Vec<Binding>,
    pub returns: Option<Pack>,
    pub variadic: Option<Binding>,
    pub body: Vec<Stmt>,
    pub span: Range<usize>,
}
#[derive(Debug)]
pub enum Stmt {
    Class(ClassDecl),
    Trait(TraitDecl),
    Import(Binding, String),
    Try(Box<TryStatement>),
    Local(Vec<Binding>, Vec<Expr>),
    Assign(Vec<Expr>, Vec<Expr>),
    Function(Binding, Function),
    NamedFunction(Expr, Function, bool),
    Call(Expr),
    Return(Vec<Expr>, Range<usize>),
    Block(Vec<Stmt>),
    If(Vec<(Expr, Vec<Stmt>)>, Option<Vec<Stmt>>),
    While(Expr, Vec<Stmt>),
    Repeat(Vec<Stmt>, Expr),
    NumericFor(Box<NumericFor>),
    GenericFor(Vec<Binding>, Expr, Vec<Stmt>),
    Break,
}

#[derive(Debug)]
pub struct ClassDecl {
    pub name: Binding,
    pub traits: Vec<Binding>,
    pub fields: Vec<ClassField>,
    pub methods: Vec<Method>,
}
#[derive(Debug)]
pub struct ClassField {
    pub binding: Binding,
    pub default: Option<Expr>,
}
#[derive(Debug)]
pub struct Method {
    pub name: Binding,
    pub function: Function,
}
#[derive(Debug)]
pub struct TraitDecl {
    pub name: Binding,
    pub required: Vec<Binding>,
    pub methods: Vec<Method>,
}

#[derive(Debug)]
pub struct NumericFor {
    pub binding: Binding,
    pub from: Expr,
    pub to: Expr,
    pub step: Option<Expr>,
    pub body: Vec<Stmt>,
}

#[derive(Debug)]
pub struct TryStatement {
    pub body: Vec<Stmt>,
    pub error: Binding,
    pub handler: Vec<Stmt>,
    pub otherwise: Option<Vec<Stmt>>,
}
