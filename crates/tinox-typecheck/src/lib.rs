use std::collections::HashMap;
use tinox_common::{Error, ErrorBag, Span, Spanned};
use tinox_parser::{
    BinaryOp, Class, DeclKind, Expr, ExprKind, Function, Literal, SourceFile, Stmt, StmtKind,
    Type, UnaryOp,
};

#[derive(Debug, Clone)]
pub enum TypeError {
    UndefinedVariable(String, Span),
    UndefinedFunction(String, Span),
    TypeMismatch {
        expected: String,
        found: String,
        span: Span,
    },
    BinaryOpTypeMismatch {
        op: String,
        lhs: String,
        rhs: String,
        span: Span,
    },
    UnaryOpTypeMismatch {
        op: String,
        operand: String,
        span: Span,
    },
    InvalidArgumentCount {
        expected: usize,
        found: usize,
        span: Span,
    },
    InvalidFieldAccess(String, Span),
    IndexNotInteger(Span),
    FieldNotFound(String, String, Span),
    CannotAssignToImmutable(String, Span),
    MutableExpected(Span),
    ReturnTypeMismatch {
        expected: String,
        found: String,
        span: Span,
    },
    MissingReturn(Span),
    BreakOutsideLoop(Span),
    ContinueOutsideLoop(Span),
    CatchParameterTypeMismatch(String, String, Span),
    ThrowTypeMismatch(String, Span),
    InvalidCast(String, String, Span),
    InvalidMatchArm(String, Span),
    NonExhaustiveMatch(Span),
    InvalidRangeType(String, Span),
    DivisionByZero(Span),
    CannotInferType(Span),
    DuplicateDefinition(String, Span),
}

impl TypeError {
    fn to_error(&self) -> Error {
        match self {
            TypeError::UndefinedVariable(name, span) => {
                Error::new(*span, format!("undefined variable: {}", name))
            }
            TypeError::UndefinedFunction(name, span) => {
                Error::new(*span, format!("undefined function: {}", name))
            }
            TypeError::TypeMismatch {
                expected,
                found,
                span,
            } => Error::new(*span, format!("expected {}, found {}", expected, found)),
            TypeError::BinaryOpTypeMismatch { op, lhs, rhs, span } => Error::new(
                *span,
                format!(
                    "binary op '{}' cannot be applied to {} and {}",
                    op, lhs, rhs
                ),
            ),
            TypeError::UnaryOpTypeMismatch { op, operand, span } => Error::new(
                *span,
                format!("unary op '{}' cannot be applied to {}", op, operand),
            ),
            TypeError::InvalidArgumentCount {
                expected,
                found,
                span,
            } => Error::new(
                *span,
                format!("expected {} arguments, found {}", expected, found),
            ),
            TypeError::InvalidFieldAccess(typename, span) => {
                Error::new(*span, format!("cannot access field on type: {}", typename))
            }
            TypeError::IndexNotInteger(span) => {
                Error::new(*span, "array index must be an integer type".to_string())
            }
            TypeError::FieldNotFound(typename, field, span) => {
                Error::new(*span, format!("type {} has no field '{}'", typename, field))
            }
            TypeError::CannotAssignToImmutable(name, span) => Error::new(
                *span,
                format!("cannot assign to immutable variable: {}", name),
            ),
            TypeError::MutableExpected(span) => Error::new(
                *span,
                "mutable variable expected for compound assignment".to_string(),
            ),
            TypeError::ReturnTypeMismatch {
                expected,
                found,
                span,
            } => Error::new(
                *span,
                format!(
                    "function return type mismatch: expected {}, found {}",
                    expected, found
                ),
            ),
            TypeError::MissingReturn(span) => {
                Error::new(*span, "missing return statement in function".to_string())
            }
            TypeError::BreakOutsideLoop(span) => {
                Error::new(*span, "break statement outside of loop".to_string())
            }
            TypeError::ContinueOutsideLoop(span) => {
                Error::new(*span, "continue statement outside of loop".to_string())
            }
            TypeError::CatchParameterTypeMismatch(expected, found, span) => Error::new(
                *span,
                format!(
                    "catch parameter type mismatch: expected {}, found {}",
                    expected, found
                ),
            ),
            TypeError::ThrowTypeMismatch(ty, span) => Error::new(
                *span,
                format!("throw requires a string or error type, found: {}", ty),
            ),
            TypeError::InvalidCast(from, to, span) => {
                Error::new(*span, format!("cannot cast from {} to {}", from, to))
            }
            TypeError::InvalidMatchArm(ty, span) => {
                Error::new(*span, format!("invalid match arm for type: {}", ty))
            }
            TypeError::NonExhaustiveMatch(span) => {
                Error::new(*span, "non-exhaustive match patterns".to_string())
            }
            TypeError::InvalidRangeType(ty, span) => Error::new(
                *span,
                format!("range requires integer operands, found: {}", ty),
            ),
            TypeError::DivisionByZero(span) => Error::new(*span, "division by zero".to_string()),
            TypeError::CannotInferType(span) => Error::new(*span, "cannot infer type".to_string()),
            TypeError::DuplicateDefinition(name, span) => {
                Error::new(*span, format!("duplicate definition of: {}", name))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueType {
    Int,
    Float,
    Bool,
    Char,
    String,
    Unit,
    Never,
    Any,
    Array,
    Ref,
    Fn,
    Named(String),
    Tuple,
    Range,
}

impl ValueType {
    fn from_parser_type(ty: &Type) -> Self {
        match ty {
            Type::Int8
            | Type::Int16
            | Type::Int32
            | Type::Int64
            | Type::UInt8
            | Type::UInt16
            | Type::UInt32
            | Type::UInt64 => ValueType::Int,
            Type::Float32 | Type::Float64 => ValueType::Float,
            Type::Bool => ValueType::Bool,
            Type::Char => ValueType::Char,
            Type::String => ValueType::String,
            Type::Unit => ValueType::Unit,
            Type::Never => ValueType::Never,
            Type::Any => ValueType::Any,
            Type::Infer => ValueType::Any,
            Type::Named(name) => ValueType::Named(name.clone()),
            Type::Array(_) => ValueType::Array,
            Type::Mutable(_) => ValueType::Ref,
            Type::Ref(_) => ValueType::Ref,
            Type::Fn { .. } => ValueType::Fn,
        }
    }

    fn to_string(&self) -> String {
        match self {
            ValueType::Int => "Int64".to_string(),
            ValueType::Float => "Float64".to_string(),
            ValueType::Bool => "Bool".to_string(),
            ValueType::Char => "Char".to_string(),
            ValueType::String => "String".to_string(),
            ValueType::Unit => "Unit".to_string(),
            ValueType::Never => "Never".to_string(),
            ValueType::Any => "Any".to_string(),
            ValueType::Array => "Array".to_string(),
            ValueType::Ref => "Ref".to_string(),
            ValueType::Fn => "Fn".to_string(),
            ValueType::Named(name) => name.clone(),
            ValueType::Tuple => "Tuple".to_string(),
            ValueType::Range => "Range".to_string(),
        }
    }
}

struct SymbolTable {
    variables: HashMap<String, (ValueType, bool)>,
    functions: HashMap<String, FunctionSignature>,
    in_loop: bool,
}

#[derive(Debug, Clone)]
struct FunctionSignature {
    params: Vec<(String, ValueType)>,
    return_type: ValueType,
}

impl SymbolTable {
    fn new() -> Self {
        Self {
            variables: HashMap::new(),
            functions: HashMap::new(),
            in_loop: false,
        }
    }

    fn enter_scope(&self) -> HashMap<String, (ValueType, bool)> {
        self.variables.clone()
    }

    fn exit_scope(&mut self, vars: HashMap<String, (ValueType, bool)>) {
        self.variables = vars;
    }
}

pub struct TypeChecker {
    errors: Vec<Error>,
    symbols: SymbolTable,
    enums: HashMap<String, Vec<String>>, // enum_name -> list of variant names
    interfaces: HashMap<String, Vec<(String, FunctionSignature)>>, // interface_name -> [(method_name, signature)]
    interface_implementations: HashMap<String, Vec<String>>, // class_name -> [interface_names]
}

impl TypeChecker {
    pub fn new() -> Self {
        let mut symbols = SymbolTable::new();
        symbols.functions.insert(
            "print".to_string(),
            FunctionSignature {
                params: vec![("value".to_string(), ValueType::Any)],
                return_type: ValueType::Unit,
            },
        );
        symbols.functions.insert(
            "println".to_string(),
            FunctionSignature {
                params: vec![("value".to_string(), ValueType::Any)],
                return_type: ValueType::Unit,
            },
        );
        Self {
            errors: Vec::new(),
            symbols,
            enums: HashMap::new(),
            interfaces: HashMap::new(),
            interface_implementations: HashMap::new(),
        }
    }

    pub fn check(&mut self, source: &SourceFile) -> Result<SourceFile, ErrorBag> {
        self.check_source_file(source);
        if self.errors.is_empty() {
            Ok(source.clone())
        } else {
            Err(ErrorBag {
                errors: std::mem::take(&mut self.errors),
            })
        }
    }

    fn check_source_file(&mut self, source: &SourceFile) {
        for decl in &source.decls {
            match &decl.node {
                DeclKind::Function(f) => {
                    let sig = FunctionSignature {
                        params: f
                            .params
                            .iter()
                            .map(|p| (p.name.clone(), Self::type_to_value(&p.param_type)))
                            .collect(),
                        return_type: Self::type_to_value(&f.ret_type),
                    };
                    self.symbols.functions.insert(f.name.clone(), sig);
                }
                DeclKind::Class(c) => {
                    for field in &c.fields {
                        let ty = Self::type_to_value(&field.field_type);
                        self.symbols
                            .variables
                            .insert(format!("{}.{}", c.name, field.name), (ty, true));
                    }
                    for method in &c.methods {
                        let mut params =
                            vec![("self".to_string(), ValueType::Named(c.name.clone()))];
                        params.extend(
                            method
                                .params
                                .iter()
                                .map(|p| (p.name.clone(), Self::type_to_value(&p.param_type))),
                        );
                        let sig = FunctionSignature {
                            params,
                            return_type: Self::type_to_value(&method.ret_type),
                        };
                        self.symbols
                            .functions
                            .insert(format!("{}_{}", c.name, method.name), sig);
                    }
                }
                DeclKind::Enum(e) => {
                    let variant_names: Vec<String> =
                        e.variants.iter().map(|v| v.name.clone()).collect();
                    self.enums.insert(e.name.clone(), variant_names.clone());
                    for variant in &e.variants {
                        let ty = ValueType::Named(format!("{}.{}", e.name, variant.name));
                        self.symbols
                            .variables
                            .insert(format!("{}.{}", e.name, variant.name), (ty, true));
                    }
                }
                DeclKind::Interface(iface) => {
                    let methods = iface
                        .methods
                        .iter()
                        .map(|m| {
                            let sig = FunctionSignature {
                                params: m
                                    .params
                                    .iter()
                                    .map(|p| (p.name.clone(), Self::type_to_value(&p.param_type)))
                                    .collect(),
                                return_type: Self::type_to_value(&m.ret_type),
                            };
                            (m.name.clone(), sig)
                        })
                        .collect();
                    self.interfaces.insert(iface.name.clone(), methods);
                }
                DeclKind::Trait(t) => {
                    let methods = t
                        .methods
                        .iter()
                        .map(|m| {
                            let sig = FunctionSignature {
                                params: m
                                    .params
                                    .iter()
                                    .map(|p| (p.name.clone(), Self::type_to_value(&p.param_type)))
                                    .collect(),
                                return_type: Self::type_to_value(&m.ret_type),
                            };
                            (m.name.clone(), sig)
                        })
                        .collect();
                    self.interfaces.insert(t.name.clone(), methods);
                }
                _ => {}
            }
        }

        for decl in &source.decls {
            match &decl.node {
                DeclKind::Function(f) => {
                    self.check_function(f);
                }
                DeclKind::Class(c) => {
                    self.check_class(c);
                }
                _ => {}
            }
        }
    }

    fn check_function(&mut self, f: &Function) {
        let saved_vars = self.symbols.enter_scope();
        for param in &f.params {
            self.symbols.variables.insert(
                param.name.clone(),
                (Self::type_to_value(&param.param_type), false),
            );
        }
        let has_return = self.check_stmt(&f.body);
        let expected = Self::type_to_value(&f.ret_type);
        if expected != ValueType::Unit && expected != ValueType::Never && !has_return {
            self.errors
                .push(Error::new(f.span, "missing return statement"));
        }
        self.symbols.exit_scope(saved_vars);
    }

    fn check_class(&mut self, c: &Class) {
        for method in &c.methods {
            let saved_vars = self.symbols.enter_scope();
            self.symbols.variables.insert(
                "self".to_string(),
                (ValueType::Named(c.name.clone()), false),
            );
            for param in &method.params {
                self.symbols.variables.insert(
                    param.name.clone(),
                    (Self::type_to_value(&param.param_type), false),
                );
            }
            let has_return = self.check_stmt(&method.body);
            let expected = Self::type_to_value(&method.ret_type);
            if expected != ValueType::Unit && expected != ValueType::Never && !has_return {
                self.errors
                    .push(Error::new(method.span, "missing return statement"));
            }
            self.symbols.exit_scope(saved_vars);
        }

        let mut implemented_ifaces = Vec::new();
        for iface_name in &c.implements {
            implemented_ifaces.push(iface_name.clone());
            if let Some(iface_methods) = self.interfaces.get(iface_name).cloned() {
                for (method_name, required_sig) in &iface_methods {
                    let full_name = format!("{}_{}", c.name, method_name);
                    if let Some(class_sig) = self.symbols.functions.get(&full_name).cloned() {
                        // Check param count (excluding implicit self)
                        let class_param_count = class_sig.params.len().saturating_sub(1);
                        if class_param_count != required_sig.params.len() {
                            self.errors.push(Error::new(
                                c.span,
                                format!(
                                    "method '{}' in class {} has wrong number of parameters for interface {}: expected {}, found {}",
                                    method_name, c.name, iface_name, required_sig.params.len(), class_param_count
                                ),
                            ));
                        } else {
                            for (i, ((_, class_ty), (_, req_ty))) in class_sig
                                .params
                                .iter()
                                .skip(1) // skip self
                                .zip(required_sig.params.iter())
                                .enumerate()
                            {
                                if !Self::types_compatible(class_ty, req_ty) {
                                    self.errors.push(Error::new(
                                        c.span,
                                        format!(
                                            "method '{}' param {} type mismatch in class {}: expected {}, found {}",
                                            method_name, i, c.name, req_ty.to_string(), class_ty.to_string()
                                        ),
                                    ));
                                }
                            }
                            if !Self::types_compatible(&class_sig.return_type, &required_sig.return_type) {
                                self.errors.push(Error::new(
                                    c.span,
                                    format!(
                                        "method '{}' return type mismatch in class {}: expected {}, found {}",
                                        method_name, c.name, required_sig.return_type.to_string(), class_sig.return_type.to_string()
                                    ),
                                ));
                            }
                        }
                    } else {
                        self.errors.push(Error::new(
                            c.span,
                            format!(
                                "class {} does not implement method '{}' from interface {}",
                                c.name, method_name, iface_name
                            ),
                        ));
                    }
                }
            }
        }
        self.interface_implementations
            .insert(c.name.clone(), implemented_ifaces);
    }

    fn check_stmt(&mut self, stmt: &Stmt) -> bool {
        match &stmt.node {
            StmtKind::Let {
                name, ty, value, ..
            } => {
                if self.symbols.variables.contains_key(name) {
                    self.errors
                        .push(TypeError::DuplicateDefinition(name.clone(), stmt.span).to_error());
                }
                let final_type = match (value, ty) {
                    (Some(v), Some(t)) => {
                        let val_ty = self.infer_type(v);
                        let ann_ty = Self::type_to_value(t);
                        if !Self::types_compatible(&ann_ty, &val_ty) {
                            self.errors.push(
                                TypeError::TypeMismatch {
                                    expected: ann_ty.to_string(),
                                    found: val_ty.to_string(),
                                    span: v.span,
                                }
                                .to_error(),
                            );
                        }
                        Some(ann_ty)
                    }
                    (Some(v), None) => Some(self.infer_type(v)),
                    (None, Some(t)) => Some(Self::type_to_value(t)),
                    (None, None) => None,
                };
                if let Some(t) = final_type {
                    self.symbols.variables.insert(name.clone(), (t, false));
                } else {
                    self.errors
                        .push(TypeError::CannotInferType(stmt.span).to_error());
                }
                false
            }
            StmtKind::Var {
                name,
                ty,
                value,
                mutable,
                ..
            } => {
                if self.symbols.variables.contains_key(name) {
                    self.errors
                        .push(TypeError::DuplicateDefinition(name.clone(), stmt.span).to_error());
                }
                let inferred_type = if let Some(v) = value {
                    Some(self.infer_type(v))
                } else if let Some(t) = ty {
                    Some(Self::type_to_value(t))
                } else {
                    None
                };
                if let Some(t) = inferred_type {
                    self.symbols.variables.insert(name.clone(), (t, *mutable));
                } else {
                    self.errors
                        .push(TypeError::CannotInferType(stmt.span).to_error());
                }
                false
            }
            StmtKind::Assignment { target, value } => {
                let target_ty = self.infer_type(target);
                let value_ty = self.infer_type(value);
                if !Self::types_compatible(&target_ty, &value_ty) {
                    self.errors.push(
                        TypeError::TypeMismatch {
                            expected: target_ty.to_string(),
                            found: value_ty.to_string(),
                            span: stmt.span,
                        }
                        .to_error(),
                    );
                }
                if let ExprKind::Ident(name) = &target.node {
                    if let Some((_, mutable)) = self.symbols.variables.get(name) {
                        if !mutable {
                            self.errors.push(
                                TypeError::CannotAssignToImmutable(name.clone(), stmt.span)
                                    .to_error(),
                            );
                        }
                    }
                }
                false
            }
            StmtKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond_ty = self.infer_type(cond);
                if cond_ty != ValueType::Bool {
                    self.errors.push(
                        TypeError::TypeMismatch {
                            expected: "Bool".to_string(),
                            found: cond_ty.to_string(),
                            span: cond.span,
                        }
                        .to_error(),
                    );
                }
                let then_returns = self.check_stmt(then_branch);
                let else_returns = if let Some(else_br) = else_branch {
                    self.check_stmt(else_br)
                } else {
                    false
                };
                then_returns && else_returns
            }
            StmtKind::While { cond, body } => {
                let cond_ty = self.infer_type(cond);
                if cond_ty != ValueType::Bool {
                    self.errors.push(
                        TypeError::TypeMismatch {
                            expected: "Bool".to_string(),
                            found: cond_ty.to_string(),
                            span: cond.span,
                        }
                        .to_error(),
                    );
                }
                self.symbols.in_loop = true;
                self.check_stmt(body);
                self.symbols.in_loop = false;
                false
            }
            StmtKind::For { var, iter, body } => {
                let iter_ty = self.infer_type(iter);
                self.symbols.in_loop = true;
                self.symbols.variables.insert(var.clone(), (iter_ty, false));
                self.check_stmt(body);
                self.symbols.in_loop = false;
                false
            }
            StmtKind::Loop { body } => {
                self.symbols.in_loop = true;
                self.check_stmt(body);
                self.symbols.in_loop = false;
                false
            }
            StmtKind::Return(opt_expr) => {
                if let Some(expr) = opt_expr {
                    self.infer_type(expr);
                }
                true
            }
            StmtKind::Break => {
                if !self.symbols.in_loop {
                    self.errors
                        .push(TypeError::BreakOutsideLoop(stmt.span).to_error());
                }
                false
            }
            StmtKind::Continue => {
                if !self.symbols.in_loop {
                    self.errors
                        .push(TypeError::ContinueOutsideLoop(stmt.span).to_error());
                }
                false
            }
            StmtKind::Throw(expr) => {
                let ty = self.infer_type(expr);
                let ty_str = ty.to_string();
                if ty_str != "String" && ty != ValueType::Any {
                    self.errors
                        .push(TypeError::ThrowTypeMismatch(ty_str, expr.span).to_error());
                }
                false
            }
            StmtKind::Try {
                body,
                catches,
                finally,
            } => {
                self.check_stmt(body);
                for catch in catches {
                    self.symbols
                        .variables
                        .insert(catch.param.clone(), (Self::type_to_value(&catch.ty), false));
                    self.check_stmt(&catch.body);
                }
                if let Some(finally_body) = finally {
                    self.check_stmt(&finally_body);
                }
                false
            }
            StmtKind::Expr(expr) => {
                self.infer_type(expr);
                false
            }
            StmtKind::Block(stmts) => {
                let saved_vars = self.symbols.enter_scope();
                let mut last_returns = false;
                for s in stmts {
                    last_returns = self.check_stmt(s);
                }
                self.symbols.exit_scope(saved_vars);
                // Block returns only if the LAST statement causes a return
                last_returns
            }
            StmtKind::Empty => false,
            StmtKind::Defer(stmt) => {
                self.check_stmt(stmt);
                false
            }
            StmtKind::ForC {
                init,
                cond,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    self.check_stmt(init_stmt);
                }
                if let Some(cond_expr) = cond {
                    let ty = self.infer_type(cond_expr);
                    if ty != ValueType::Bool {
                        self.errors.push(
                            TypeError::TypeMismatch {
                                expected: "Bool".to_string(),
                                found: ty.to_string(),
                                span: cond_expr.span,
                            }
                            .to_error(),
                        );
                    }
                }
                if let Some(update_expr) = update {
                    self.infer_type(update_expr);
                }
                self.symbols.in_loop = true;
                self.check_stmt(body);
                self.symbols.in_loop = false;
                false
            }
        }
    }

    fn infer_type(&mut self, expr: &Expr) -> ValueType {
        match &expr.node {
            ExprKind::Literal(lit) => self.literal_type(lit),
            ExprKind::Ident(name) => {
                if let Some((ty, _)) = self.symbols.variables.get(name) {
                    ty.clone()
                } else if self.symbols.functions.contains_key(name) {
                    ValueType::Fn
                } else {
                    self.errors
                        .push(TypeError::UndefinedVariable(name.clone(), expr.span).to_error());
                    ValueType::Any
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let lhs_ty = self.infer_type(lhs);
                let rhs_ty = self.infer_type(rhs);
                self.check_binary_op(op, &lhs_ty, &rhs_ty, expr.span);
                Self::binary_result_type(op, &lhs_ty, &rhs_ty)
            }
            ExprKind::Unary { op, operand } => {
                let op_ty = self.infer_type(operand);
                self.check_unary_op(op, &op_ty, expr.span);
                Self::unary_result_type(op, &op_ty)
            }
            ExprKind::Call { func, args } => self.check_call(func, args, expr.span),
            ExprKind::MethodCall { obj, method, args } => {
                let obj_ty = self.infer_type(obj);
                // Method names are looked up as ClassName_methodName
                let method_name = format!("{}_{}", obj_ty.to_string(), method);
                let func_expr = Spanned::new(ExprKind::Ident(method_name), expr.span);

                // Prepend the object as the first argument for type checking
                let mut call_args = vec![(**obj).clone()];
                call_args.extend(args.iter().map(|e| e.clone()));

                self.check_call(&func_expr, &call_args, expr.span)
            }
            ExprKind::Index { obj, index } => {
                self.infer_type(obj);
                let index_ty = self.infer_type(index);
                if !matches!(index_ty, ValueType::Int) {
                    self.errors
                        .push(TypeError::IndexNotInteger(expr.span).to_error());
                }
                // Array indexing returns the element type
                // For now, arrays are assumed to contain Int64 values
                ValueType::Int
            }
            ExprKind::FieldAccess { obj, field } => {
                let obj_ty = self.infer_type(obj);
                if let ValueType::Named(name) = obj_ty {
                    let full_name = format!("{}.{}", name, field);
                    if let Some((ty, _)) = self.symbols.variables.get(&full_name) {
                        return ty.clone();
                    }
                    self.errors
                        .push(TypeError::FieldNotFound(name, field.clone(), expr.span).to_error());
                } else {
                    self.errors.push(
                        TypeError::InvalidFieldAccess(obj_ty.to_string(), expr.span).to_error(),
                    );
                }
                ValueType::Any
            }
            ExprKind::This => {
                if let Some((ty, _)) = self.symbols.variables.get("self") {
                    ty.clone()
                } else {
                    ValueType::Named("Self".to_string())
                }
            }
            ExprKind::Super => ValueType::Named("Super".to_string()),
            ExprKind::New { class, args } => {
                for arg in args {
                    self.infer_type(arg);
                }
                ValueType::Named(class.clone())
            }
            ExprKind::StructLiteral { name, fields } => {
                for (_, field_expr) in fields {
                    self.infer_type(field_expr);
                }
                ValueType::Named(name.clone())
            }
            ExprKind::Block(stmts) => {
                let saved_vars = self.symbols.enter_scope();
                let mut last_ty = ValueType::Unit;
                for stmt in stmts {
                    self.check_stmt(stmt);
                }
                if let Some(last) = stmts.last() {
                    if let StmtKind::Expr(e) = &last.node {
                        last_ty = self.infer_type(e);
                    }
                }
                self.symbols.exit_scope(saved_vars);
                last_ty
            }
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond_ty = self.infer_type(cond);
                if cond_ty != ValueType::Bool {
                    self.errors.push(
                        TypeError::TypeMismatch {
                            expected: "Bool".to_string(),
                            found: cond_ty.to_string(),
                            span: cond.span,
                        }
                        .to_error(),
                    );
                }
                let then_ty = self.infer_type(then_branch);
                if let Some(else_br) = else_branch {
                    let else_ty = self.infer_type(else_br);
                    Self::lub(&then_ty, &else_ty)
                } else {
                    ValueType::Unit
                }
            }
            ExprKind::While { cond, body } => {
                let cond_ty = self.infer_type(cond);
                if cond_ty != ValueType::Bool {
                    self.errors.push(
                        TypeError::TypeMismatch {
                            expected: "Bool".to_string(),
                            found: cond_ty.to_string(),
                            span: cond.span,
                        }
                        .to_error(),
                    );
                }
                self.infer_type(body);
                ValueType::Unit
            }
            ExprKind::For { var, iter, body } => {
                let iter_ty = self.infer_type(iter);
                self.symbols.variables.insert(var.clone(), (iter_ty, false));
                self.infer_type(body);
                ValueType::Unit
            }
            ExprKind::Loop { body } => {
                self.infer_type(body);
                ValueType::Never
            }
            ExprKind::Match { expr, cases } => {
                self.infer_type(expr);
                let mut result_types = Vec::new();
                for case in cases {
                    let case_ty = self.infer_type(&case.body);
                    result_types.push(case_ty);
                }
                if result_types.is_empty() {
                    return ValueType::Any;
                }
                let mut unified = result_types[0].clone();
                for ty in &result_types[1..] {
                    unified = Self::lub(&unified, ty);
                }
                unified
            }
            ExprKind::Return(Some(val)) => self.infer_type(val),
            ExprKind::Return(None) => ValueType::Unit,
            ExprKind::Break => ValueType::Never,
            ExprKind::Continue => ValueType::Never,
            ExprKind::Throw(expr) => {
                self.infer_type(expr);
                ValueType::Never
            }
            ExprKind::Try {
                body,
                catches,
                finally,
            } => {
                self.infer_type(body);
                for catch in catches {
                    self.check_stmt(&catch.body);
                }
                if let Some(finally_body) = finally {
                    self.infer_type(finally_body);
                }
                ValueType::Any
            }
            ExprKind::Assign { target, value } => {
                self.infer_type(target);
                self.infer_type(value)
            }
            ExprKind::CompoundAssign {
                op: _,
                target,
                value,
            } => {
                self.infer_type(target);
                self.infer_type(value)
            }
            ExprKind::Lambda {
                params,
                ret_type,
                body,
            } => {
                let saved_vars = self.symbols.enter_scope();
                for param in params {
                    self.symbols.variables.insert(
                        param.name.clone(),
                        (Self::type_to_value(&param.param_type), false),
                    );
                }
                let body_ty = self.infer_type(body);
                self.symbols.exit_scope(saved_vars);
                let _ret = ret_type
                    .as_ref()
                    .map(|t| Self::type_to_value(t))
                    .unwrap_or(body_ty);
                ValueType::Fn
            }
            ExprKind::Spawn(_) => ValueType::Any,
            ExprKind::Await(_) => ValueType::Any,
            ExprKind::Channel => ValueType::Any,
            ExprKind::Send { channel: _, value } => {
                self.infer_type(value);
                ValueType::Unit
            }
            ExprKind::Recv(_) => ValueType::Any,
            ExprKind::Cast { expr, ty } => {
                self.infer_type(expr);
                Self::type_to_value(ty)
            }
            ExprKind::Is { expr, ty } => {
                self.infer_type(expr);
                Self::type_to_value(ty);
                ValueType::Bool
            }
            ExprKind::Range {
                start,
                end,
                inclusive: _,
            } => {
                let start_ty = self.infer_type(start);
                let end_ty = self.infer_type(end);
                if !matches!(start_ty, ValueType::Int) || !matches!(end_ty, ValueType::Int) {
                    self.errors.push(
                        TypeError::InvalidRangeType(
                            format!("{}..{}", start_ty.to_string(), end_ty.to_string()),
                            expr.span,
                        )
                        .to_error(),
                    );
                }
                ValueType::Range
            }
            ExprKind::Tuple(exprs) => {
                for e in exprs {
                    self.infer_type(e);
                }
                ValueType::Tuple
            }
            ExprKind::EnumValue {
                enum_name,
                variant: _,
                args,
            } => {
                // Type check all arguments
                for arg in args {
                    self.infer_type(arg);
                }
                // Return the enum type
                ValueType::Named(enum_name.clone())
            }
            ExprKind::ArrayLiteral(elements) => {
                for e in elements {
                    self.infer_type(e);
                }
                ValueType::Array
            }
        }
    }

    fn check_call(&mut self, func: &Expr, args: &[Expr], span: Span) -> ValueType {
        // Check if it's a simple identifier - could be function or lambda variable
        if let ExprKind::Ident(name) = &func.node {
            // First check if it's a defined function
            if let Some(sig) = self.symbols.functions.get(name).cloned() {
                if sig.params.len() != args.len() {
                    self.errors.push(
                        TypeError::InvalidArgumentCount {
                            expected: sig.params.len(),
                            found: args.len(),
                            span,
                        }
                        .to_error(),
                    );
                }
                for (arg, (_, expected_ty)) in args.iter().zip(sig.params.iter()) {
                    let arg_ty = self.infer_type(arg);
                    if !Self::types_compatible(expected_ty, &arg_ty) {
                        self.errors.push(
                            TypeError::TypeMismatch {
                                expected: expected_ty.to_string(),
                                found: arg_ty.to_string(),
                                span: arg.span,
                            }
                            .to_error(),
                        );
                    }
                }
                return sig.return_type.clone();
            }

            // Check if it's a variable with Fn type (lambda)
            if let Some((ty, _)) = self.symbols.variables.get(name) {
                if *ty == ValueType::Fn {
                    // Lambda call - just check arguments are present
                    for arg in args {
                        self.infer_type(arg);
                    }
                    return ValueType::Any; // We don't have detailed lambda type info
                }
            }

            // Not found
            self.errors
                .push(TypeError::UndefinedFunction(name.clone(), span).to_error());
        } else {
            // Not an identifier - could be complex expression returning Fn
            let func_ty = self.infer_type(func);
            if func_ty == ValueType::Fn {
                for arg in args {
                    self.infer_type(arg);
                }
                return ValueType::Any;
            }
        }

        for arg in args {
            self.infer_type(arg);
        }
        ValueType::Any
    }

    fn check_binary_op(&mut self, op: &BinaryOp, lhs: &ValueType, rhs: &ValueType, span: Span) {
        let valid = match op {
            BinaryOp::Add
            | BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Mod
            | BinaryOp::Shl
            | BinaryOp::Shr
            | BinaryOp::ShrArith => {
                matches!(lhs, ValueType::Int | ValueType::Float)
                    && matches!(rhs, ValueType::Int | ValueType::Float)
            }
            BinaryOp::And | BinaryOp::Or => {
                matches!(lhs, ValueType::Bool) && matches!(rhs, ValueType::Bool)
            }
            BinaryOp::BitOr | BinaryOp::Xor => {
                matches!(lhs, ValueType::Int) && matches!(rhs, ValueType::Int)
            }
            BinaryOp::Eq | BinaryOp::Ne => lhs == rhs,
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                matches!(lhs, ValueType::Int | ValueType::Float)
                    && matches!(rhs, ValueType::Int | ValueType::Float)
            }
        };
        if !valid {
            self.errors.push(
                TypeError::BinaryOpTypeMismatch {
                    op: format!("{:?}", op).to_lowercase(),
                    lhs: lhs.to_string(),
                    rhs: rhs.to_string(),
                    span,
                }
                .to_error(),
            );
        }
    }

    fn check_unary_op(&mut self, op: &UnaryOp, operand: &ValueType, span: Span) {
        let valid = match op {
            UnaryOp::Neg => matches!(operand, ValueType::Int | ValueType::Float),
            UnaryOp::Not => matches!(operand, ValueType::Bool),
            UnaryOp::BitNot => matches!(operand, ValueType::Int),
        };
        if !valid {
            self.errors.push(
                TypeError::UnaryOpTypeMismatch {
                    op: format!("{:?}", op).to_lowercase(),
                    operand: operand.to_string(),
                    span,
                }
                .to_error(),
            );
        }
    }

    fn binary_result_type(op: &BinaryOp, lhs: &ValueType, rhs: &ValueType) -> ValueType {
        match op {
            BinaryOp::Eq
            | BinaryOp::Ne
            | BinaryOp::Lt
            | BinaryOp::Le
            | BinaryOp::Gt
            | BinaryOp::Ge => ValueType::Bool,
            BinaryOp::And | BinaryOp::Or => ValueType::Bool,
            _ => {
                if *lhs == ValueType::Float || *rhs == ValueType::Float {
                    ValueType::Float
                } else {
                    ValueType::Int
                }
            }
        }
    }

    fn unary_result_type(op: &UnaryOp, operand: &ValueType) -> ValueType {
        match op {
            UnaryOp::Neg => operand.clone(),
            UnaryOp::Not => ValueType::Bool,
            UnaryOp::BitNot => ValueType::Int,
        }
    }

    fn literal_type(&self, lit: &Literal) -> ValueType {
        match lit {
            Literal::Integer(_) => ValueType::Int,
            Literal::Float(_) => ValueType::Float,
            Literal::String(_) => ValueType::String,
            Literal::Char(_) => ValueType::Char,
            Literal::Byte(_) => ValueType::Int,
            Literal::Bool(_) => ValueType::Bool,
            Literal::Null => ValueType::Any,
        }
    }

    fn type_to_value(ty: &Type) -> ValueType {
        ValueType::from_parser_type(ty)
    }

    fn types_compatible(a: &ValueType, b: &ValueType) -> bool {
        if a == b {
            return true;
        }
        match (a, b) {
            (ValueType::Int, ValueType::Float) => true,
            (ValueType::Float, ValueType::Int) => true,
            (ValueType::Any, _) | (_, ValueType::Any) => true,
            _ => false,
        }
    }

    fn lub(a: &ValueType, b: &ValueType) -> ValueType {
        if a == b {
            return a.clone();
        }
        match (a, b) {
            (ValueType::Int, ValueType::Float) | (ValueType::Float, ValueType::Int) => {
                ValueType::Float
            }
            _ => ValueType::Any,
        }
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

pub fn typecheck(source: &SourceFile) -> Result<SourceFile, ErrorBag> {
    let mut checker = TypeChecker::new();
    checker.check(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tinox_lexer::Lexer;
    use tinox_parser::Parser;

    fn typecheck_code(code: &str) -> Result<SourceFile, ErrorBag> {
        let mut lexer = Lexer::new(code);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        let ast = parser.parse().unwrap();
        let mut checker = TypeChecker::new();
        checker.check(&ast)
    }

    #[test]
    fn test_simple_function() {
        let result = typecheck_code("fn main() -> Int32 { return 42; }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_undefined_variable() {
        let result = typecheck_code("fn main() -> Int32 { return x; }");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .errors
            .iter()
            .any(|e| e.message.contains("undefined variable")));
    }

    #[test]
    fn test_binary_op_mismatch() {
        let result = typecheck_code("fn main() -> Int32 { return true + 1; }");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .errors
            .iter()
            .any(|e| e.message.contains("cannot be applied")));
    }

    #[test]
    fn test_type_mismatch() {
        let result = typecheck_code("fn main() -> Int32 { let x: Int32 = \"hello\"; return x; }");
        assert!(result.is_err());
    }

    #[test]
    fn test_undefined_function() {
        let result = typecheck_code("fn main() -> Int32 { return foo(); }");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .errors
            .iter()
            .any(|e| e.message.contains("undefined function")));
    }
}
