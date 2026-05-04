use tinox_common::{Span, Spanned};

pub type Ident = String;

#[derive(Debug, Clone)]
pub enum Type {
    Int8,
    Int16,
    Int32,
    Int64,
    UInt8,
    UInt16,
    UInt32,
    UInt64,
    Float32,
    Float64,
    Bool,
    Char,
    String,
    Unit,
    Never,
    Any,
    Infer,
    Named(Ident),
    /// A named type instantiated with concrete type arguments, e.g. `Box<Int64>`.
    Generic { name: Ident, args: Vec<Type> },
    Array(Box<Type>),
    Map(Box<Type>, Box<Type>),
    Mutable(Box<Type>),
    Ref(Box<Type>),
    Fn { params: Vec<Type>, ret: Box<Type> },
    Tuple(Vec<Type>),
}

impl Type {
    pub fn unit() -> Self {
        Type::Unit
    }
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: Ident,
    pub field_type: Type,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Wildcard(Span),
    Ident(Ident, Option<Box<Pattern>>, Span),
    Literal(Literal, Span),
    Tuple(Vec<Pattern>, Span),
    EnumVariant {
        enum_name: Ident,
        variant: Ident,
        args: Vec<Pattern>,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub enum Literal {
    Integer(i64),
    Float(f64),
    String(String),
    Char(char),
    Byte(u8),
    Bool(bool),
    Null,
}

impl Literal {
    pub fn ty(&self) -> Type {
        match self {
            Literal::Integer(_) => Type::Int64,
            Literal::Float(_) => Type::Float64,
            Literal::String(_) => Type::String,
            Literal::Char(_) => Type::Char,
            Literal::Byte(_) => Type::UInt8,
            Literal::Bool(_) => Type::Bool,
            Literal::Null => Type::Named("Null".to_string()),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Literal(Literal),
    ArrayLiteral(Vec<Expr>),
    MapLiteral(Vec<(Expr, Expr)>),
    Ident(Ident),
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Call {
        func: Box<Expr>,
        args: Vec<Expr>,
    },
    MethodCall {
        obj: Box<Expr>,
        method: Ident,
        args: Vec<Expr>,
    },
    Index {
        obj: Box<Expr>,
        index: Box<Expr>,
    },
    FieldAccess {
        obj: Box<Expr>,
        field: Ident,
    },
    This,
    SuperCall {
        method: Ident,
        args: Vec<Expr>,
    },
    New {
        class: Ident,
        type_args: Vec<Type>,
        args: Vec<Expr>,
    },
    StructLiteral {
        name: Ident,
        fields: Vec<(Ident, Expr)>,
    },
    Block(Vec<Stmt>),
    If {
        cond: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Option<Box<Expr>>,
    },
    While {
        cond: Box<Expr>,
        body: Box<Expr>,
    },
    For {
        var: Ident,
        iter: Box<Expr>,
        body: Box<Expr>,
    },
    Loop {
        body: Box<Expr>,
    },
    Match {
        expr: Box<Expr>,
        cases: Vec<MatchCase>,
    },
    Return(Option<Box<Expr>>),
    Break,
    Continue,
    Throw(Box<Expr>),
    Try {
        body: Box<Expr>,
        catches: Vec<CatchClause>,
        finally: Option<Box<Expr>>,
    },
    Assign {
        target: Box<Expr>,
        value: Box<Expr>,
    },
    CompoundAssign {
        op: CompoundOp,
        target: Box<Expr>,
        value: Box<Expr>,
    },
    Lambda {
        params: Vec<Param>,
        ret_type: Option<Type>,
        body: Box<Expr>,
    },
    Spawn(Box<Expr>),
    Await(Box<Expr>),
    Channel,
    Send {
        channel: Box<Expr>,
        value: Box<Expr>,
    },
    Recv(Box<Expr>),
    Cast {
        expr: Box<Expr>,
        ty: Type,
    },
    Is {
        expr: Box<Expr>,
        ty: Type,
    },
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
        inclusive: bool,
    },
    Tuple(Vec<Expr>),
    TupleIndex {
        tuple: Box<Expr>,
        index: usize,
    },
    EnumValue {
        enum_name: Ident,
        variant: Ident,
        args: Vec<Expr>,
    },
}

pub type Expr = Spanned<ExprKind>;

#[derive(Debug, Clone)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    And,
    Or,
    BitAnd,
    BitOr,
    Xor,
    Shl,
    Shr,
    ShrArith,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone)]
pub enum CompoundOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    ShrArith,
}

#[derive(Debug, Clone)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
}

#[derive(Debug, Clone)]
pub struct MatchCase {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct CatchClause {
    pub param: Ident,
    pub ty: Type,
    pub body: Stmt,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum StmtKind {
    Expr(Expr),
    Let {
        name: Ident,
        ty: Option<Type>,
        value: Option<Expr>,
    },
    Var {
        name: Ident,
        ty: Option<Type>,
        value: Option<Expr>,
        mutable: bool,
    },
    Assignment {
        target: Expr,
        value: Expr,
    },
    If {
        cond: Expr,
        then_branch: Box<Stmt>,
        else_branch: Option<Box<Stmt>>,
    },
    While {
        cond: Expr,
        body: Box<Stmt>,
    },
    For {
        var: Ident,
        iter: Expr,
        body: Box<Stmt>,
    },
    ForC {
        init: Option<Box<Stmt>>,
        cond: Option<Expr>,
        update: Option<Expr>,
        body: Box<Stmt>,
    },
    Loop {
        body: Box<Stmt>,
    },
    Return(Option<Expr>),
    Break,
    Continue,
    Throw(Expr),
    Try {
        body: Box<Stmt>,
        catches: Vec<CatchClause>,
        finally: Option<Box<Stmt>>,
    },
    Defer(Box<Stmt>),
    Block(Vec<Stmt>),
    Select {
        arms: Vec<SelectArm>,
        default: Option<Box<Stmt>>,
    },
    Empty,
}

#[derive(Debug, Clone)]
pub struct SelectArm {
    pub channel: Expr,
    pub var: Ident,
    pub body: Stmt,
    pub span: Span,
}

pub type Stmt = Spanned<StmtKind>;

#[derive(Debug, Clone)]
pub struct Param {
    pub name: Ident,
    pub param_type: Type,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: Ident,
    pub type_params: Vec<String>,
    pub params: Vec<Param>,
    pub ret_type: Type,
    pub body: Stmt,
    pub span: Span,
    pub is_async: bool,
    pub doc: Option<String>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone)]
pub struct Method {
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret_type: Type,
    pub body: Stmt,
    pub static_: bool,
    pub visibility: Visibility,
    pub span: Span,
    pub is_async: bool,
    pub doc: Option<String>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone)]
pub enum Visibility {
    Public,
    Private,
    Protected,
    Package,
}

impl Default for Visibility {
    fn default() -> Self {
        Visibility::Public
    }
}

#[derive(Debug, Clone)]
pub struct FieldDef {
    pub name: Ident,
    pub field_type: Type,
    pub visibility: Visibility,
    pub mutable: bool,
    pub span: Span,
    pub doc: Option<String>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone)]
pub struct Class {
    pub name: Ident,
    pub type_params: Vec<String>,
    pub extends: Option<Ident>,
    pub implements: Vec<Ident>,
    pub fields: Vec<FieldDef>,
    pub methods: Vec<Method>,
    pub span: Span,
    pub doc: Option<String>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone)]
pub struct Interface {
    pub name: Ident,
    pub extends: Vec<Ident>,
    pub methods: Vec<Function>,
    pub span: Span,
    pub doc: Option<String>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: Ident,
    pub args: Vec<Type>,
    pub span: Span,
    pub doc: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Enum {
    pub name: Ident,
    pub variants: Vec<EnumVariant>,
    pub span: Span,
    pub doc: Option<String>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone)]
pub struct Trait {
    pub name: Ident,
    pub methods: Vec<Function>,
    pub span: Span,
    pub doc: Option<String>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone)]
pub struct Namespace {
    pub name: Vec<Ident>,
    pub decls: Vec<Decl>,
    pub span: Span,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone)]
pub struct Annotation {
    pub name: Ident,
    pub args: Vec<Literal>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ImmutableDecl {
    pub name: Ident,
    pub fields: Vec<Param>,
    pub span: Span,
    pub doc: Option<String>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone)]
pub enum DeclKind {
    Function(Function),
    Class(Class),
    Interface(Interface),
    Enum(Enum),
    Trait(Trait),
    Import(Import),
    Module(Ident),
    Namespace(Namespace),
    Immutable(ImmutableDecl),
}

pub type Decl = Spanned<DeclKind>;

#[derive(Debug, Clone)]
pub struct Import {
    pub path: Vec<Ident>,
    pub alias: Option<Ident>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub decls: Vec<Decl>,
    pub span: Span,
}

impl SourceFile {
    pub fn new(decls: Vec<Decl>) -> Self {
        Self {
            decls,
            span: Span::dummy(),
        }
    }
}
