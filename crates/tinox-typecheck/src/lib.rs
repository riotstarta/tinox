use std::collections::{HashMap, HashSet};
use tinox_common::{Error, ErrorBag, Span, Spanned};
use tinox_parser::{
    BinaryOp, Class, DeclKind, Expr, ExprKind, Function, Literal, SourceFile, Stmt, StmtKind,
    Type, UnaryOp, Visibility,
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
    UndefinedInterface(String, Span),
    InterfaceMethodConflict {
        interface: String,
        parent: String,
        method: String,
        span: Span,
    },
    PrivateAccess {
        class: String,
        member: String,
        span: Span,
    },
    ProtectedAccess {
        class: String,
        member: String,
        span: Span,
    },
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
            TypeError::UndefinedInterface(name, span) => {
                Error::new(*span, format!("undefined interface: {}", name))
            }
            TypeError::InterfaceMethodConflict {
                interface,
                parent,
                method,
                span,
            } => Error::new(
                *span,
                format!(
                    "interface '{}' extends '{}' but both define method '{}' with conflicting signatures",
                    interface, parent, method
                ),
            ),
            TypeError::PrivateAccess { class, member, span } => Error::new(
                *span,
                format!("'{}' is private to class '{}'", member, class),
            ),
            TypeError::ProtectedAccess { class, member, span } => Error::new(
                *span,
                format!("'{}' is protected in class '{}' and not accessible here", member, class),
            ),
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
            Type::Generic { name, .. } => ValueType::Named(name.clone()),
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
    interface_extends: HashMap<String, (Vec<String>, tinox_common::Span)>, // interface_name -> (parent_names, span)
    interface_implementations: HashMap<String, Vec<String>>, // class_name -> [interface_names]
    class_parents: HashMap<String, String>, // child_class_name -> parent_class_name
    current_class: Option<String>, // class currently being type-checked
    method_visibility: HashMap<String, Visibility>, // ClassName_methodName -> visibility
    field_visibility: HashMap<String, Visibility>,  // ClassName.fieldName -> visibility
    /// Type parameter names in scope for the function currently being checked.
    type_param_scope: HashSet<String>,
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
        symbols.functions.insert(
            "len".to_string(),
            FunctionSignature {
                params: vec![("value".to_string(), ValueType::Any)],
                return_type: ValueType::Int,
            },
        );
        symbols.functions.insert(
            "assert".to_string(),
            FunctionSignature {
                params: vec![("cond".to_string(), ValueType::Bool)],
                return_type: ValueType::Unit,
            },
        );
        symbols.functions.insert(
            "push".to_string(),
            FunctionSignature {
                params: vec![
                    ("arr".to_string(), ValueType::Array),
                    ("val".to_string(), ValueType::Any),
                ],
                return_type: ValueType::Array,
            },
        );
        symbols.functions.insert(
            "pop".to_string(),
            FunctionSignature {
                params: vec![("arr".to_string(), ValueType::Array)],
                return_type: ValueType::Array,
            },
        );
        symbols.functions.insert(
            "charAt".to_string(),
            FunctionSignature {
                params: vec![
                    ("s".to_string(), ValueType::String),
                    ("i".to_string(), ValueType::Int),
                ],
                return_type: ValueType::String,
            },
        );
        symbols.functions.insert(
            "toInt".to_string(),
            FunctionSignature {
                params: vec![("value".to_string(), ValueType::String)],
                return_type: ValueType::Int,
            },
        );
        symbols.functions.insert(
            "toFloat".to_string(),
            FunctionSignature {
                params: vec![("value".to_string(), ValueType::String)],
                return_type: ValueType::Float,
            },
        );
        symbols.functions.insert(
            "toString".to_string(),
            FunctionSignature {
                params: vec![("value".to_string(), ValueType::Any)],
                return_type: ValueType::String,
            },
        );
        Self {
            errors: Vec::new(),
            symbols,
            enums: HashMap::new(),
            interfaces: HashMap::new(),
            interface_extends: HashMap::new(),
            interface_implementations: HashMap::new(),
            class_parents: HashMap::new(),
            current_class: None,
            method_visibility: HashMap::new(),
            field_visibility: HashMap::new(),
            type_param_scope: HashSet::new(),
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

    /// Returns (interface_name -> ordered method names, class_name -> [interface names])
    pub fn interface_info(
        &self,
    ) -> (HashMap<String, Vec<String>>, HashMap<String, Vec<String>>) {
        let iface_methods: HashMap<String, Vec<String>> = self
            .interfaces
            .iter()
            .map(|(name, methods)| {
                (
                    name.clone(),
                    methods.iter().map(|(m, _)| m.clone()).collect(),
                )
            })
            .collect();
        (iface_methods, self.interface_implementations.clone())
    }

    fn check_source_file(&mut self, source: &SourceFile) {
        for decl in &source.decls {
            match &decl.node {
                DeclKind::Function(f) => {
                    let sig = FunctionSignature {
                        params: f
                            .params
                            .iter()
                            .map(|p| (p.name.clone(), Self::type_to_value_erasing(&p.param_type, &f.type_params)))
                            .collect(),
                        return_type: Self::type_to_value_erasing(&f.ret_type, &f.type_params),
                    };
                    self.symbols.functions.insert(f.name.clone(), sig);
                }
                DeclKind::Class(c) => {
                    if let Some(parent) = &c.extends {
                        self.class_parents.insert(c.name.clone(), parent.clone());
                    }
                    for field in &c.fields {
                        let ty = Self::type_to_value(&field.field_type);
                        let key = format!("{}.{}", c.name, field.name);
                        self.symbols.variables.insert(key.clone(), (ty, true));
                        self.field_visibility.insert(key, field.visibility.clone());
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
                        let key = format!("{}_{}", c.name, method.name);
                        self.symbols.functions.insert(key.clone(), sig);
                        self.method_visibility.insert(key, method.visibility.clone());
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
                    if !iface.extends.is_empty() {
                        self.interface_extends.insert(
                            iface.name.clone(),
                            (iface.extends.clone(), decl.span),
                        );
                    }
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

        // Second pass: expand interfaces with methods from their parent interfaces.
        // Collect the extends relationships first to avoid borrow conflicts.
        let extends_map: Vec<(String, Vec<String>, tinox_common::Span)> = self
            .interface_extends
            .iter()
            .map(|(name, (parents, span))| (name.clone(), parents.clone(), *span))
            .collect();

        for (iface_name, parents, span) in &extends_map {
            // Validate that every parent interface exists.
            for parent in parents {
                if !self.interfaces.contains_key(parent) {
                    self.errors.push(
                        TypeError::UndefinedInterface(parent.clone(), *span).to_error(),
                    );
                }
            }

            // Collect all methods from parents (only those that are defined).
            let mut inherited: Vec<(String, FunctionSignature)> = Vec::new();
            for parent in parents {
                if let Some(parent_methods) = self.interfaces.get(parent).cloned() {
                    for (method_name, parent_sig) in parent_methods {
                        inherited.push((method_name, parent_sig));
                    }
                }
            }

            // Merge: for each inherited method, check for conflicts with own methods.
            if let Some(own_methods) = self.interfaces.get(iface_name).cloned() {
                for (method_name, parent_sig) in &inherited {
                    if let Some((_, own_sig)) =
                        own_methods.iter().find(|(n, _)| n == method_name)
                    {
                        // Both define the method — check for signature conflict.
                        let params_match = own_sig.params.len() == parent_sig.params.len()
                            && own_sig
                                .params
                                .iter()
                                .zip(parent_sig.params.iter())
                                .all(|((_, a), (_, b))| Self::types_compatible(a, b));
                        let ret_match = Self::types_compatible(
                            &own_sig.return_type,
                            &parent_sig.return_type,
                        );
                        if !params_match || !ret_match {
                            // Find the parent name that defined this method.
                            let parent_name = parents
                                .iter()
                                .find(|p| {
                                    self.interfaces
                                        .get(*p)
                                        .map(|ms| ms.iter().any(|(n, _)| n == method_name))
                                        .unwrap_or(false)
                                })
                                .cloned()
                                .unwrap_or_default();
                            self.errors.push(
                                TypeError::InterfaceMethodConflict {
                                    interface: iface_name.clone(),
                                    parent: parent_name,
                                    method: method_name.clone(),
                                    span: *span,
                                }
                                .to_error(),
                            );
                        }
                        // Own method takes precedence — do not add inherited version.
                    } else {
                        // Not defined in own methods — add the inherited one.
                        self.interfaces
                            .get_mut(iface_name)
                            .unwrap()
                            .push((method_name.clone(), parent_sig.clone()));
                    }
                }
            }
        }

        // Register interface methods as InterfaceName_methodName in symbol table
        // so method calls through interface-typed variables type-check correctly.
        let iface_entries: Vec<(String, String, FunctionSignature)> = self
            .interfaces
            .iter()
            .flat_map(|(iface_name, methods)| {
                methods.iter().map(move |(method_name, sig)| {
                    (iface_name.clone(), method_name.clone(), sig.clone())
                })
            })
            .collect();
        for (iface_name, method_name, sig) in iface_entries {
            let full_name = format!("{}_{}", iface_name, method_name);
            // first param is self (the interface-typed object)
            let mut params = vec![("self".to_string(), ValueType::Named(iface_name.clone()))];
            params.extend(sig.params.clone());
            self.symbols.functions.insert(
                full_name,
                FunctionSignature {
                    params,
                    return_type: sig.return_type.clone(),
                },
            );
        }

        // Third pass: expand class inheritance (fields and methods from parent classes).
        {
            use std::collections::HashSet;
            let class_map: HashMap<String, tinox_parser::Class> = source
                .decls
                .iter()
                .filter_map(|d| {
                    if let DeclKind::Class(c) = &d.node {
                        Some((c.name.clone(), c.clone()))
                    } else {
                        None
                    }
                })
                .collect();

            let class_names: Vec<String> = class_map.keys().cloned().collect();
            let mut processed: HashSet<String> = HashSet::new();

            loop {
                let before = processed.len();
                for name in &class_names {
                    if processed.contains(name) {
                        continue;
                    }
                    let c = &class_map[name];
                    let parent_ready = c
                        .extends
                        .as_ref()
                        .map(|p| processed.contains(p) || !class_map.contains_key(p))
                        .unwrap_or(true);
                    if !parent_ready {
                        continue;
                    }

                    if let Some(parent_name) = &c.extends {
                        if !class_map.contains_key(parent_name) {
                            self.errors.push(Error::new(
                                c.span,
                                format!("undefined parent class: {}", parent_name),
                            ));
                            processed.insert(name.clone());
                            continue;
                        }

                        let child_own_fields: HashSet<String> =
                            c.fields.iter().map(|f| f.name.clone()).collect();
                        let child_own_methods: HashSet<String> =
                            c.methods.iter().map(|m| m.name.clone()).collect();

                        // Walk the ancestor chain and collect inherited fields/methods.
                        let mut ancestor = parent_name.clone();
                        loop {
                            let Some(pc) = class_map.get(&ancestor) else {
                                break;
                            };

                            for field in &pc.fields {
                                if child_own_fields.contains(&field.name) {
                                    continue;
                                }
                                let child_key = format!("{}.{}", name, field.name);
                                if self.symbols.variables.contains_key(&child_key) {
                                    continue;
                                }
                                let ty = Self::type_to_value(&field.field_type);
                                self.symbols.variables.insert(child_key.clone(), (ty, true));
                                self.field_visibility
                                    .entry(child_key)
                                    .or_insert_with(|| field.visibility.clone());
                            }

                            for method in &pc.methods {
                                if child_own_methods.contains(&method.name) {
                                    continue;
                                }
                                let child_key = format!("{}_{}", name, method.name);
                                if self.symbols.functions.contains_key(&child_key) {
                                    continue;
                                }
                                let mut params = vec![(
                                    "self".to_string(),
                                    ValueType::Named(name.clone()),
                                )];
                                params.extend(method.params.iter().map(|p| {
                                    (p.name.clone(), Self::type_to_value(&p.param_type))
                                }));
                                let sig = FunctionSignature {
                                    params,
                                    return_type: Self::type_to_value(&method.ret_type),
                                };
                                self.symbols.functions.insert(child_key.clone(), sig);
                                self.method_visibility
                                    .entry(child_key)
                                    .or_insert_with(|| method.visibility.clone());
                            }

                            ancestor = match &pc.extends {
                                Some(next) => next.clone(),
                                None => break,
                            };
                        }
                    }

                    processed.insert(name.clone());
                }
                if processed.len() == before {
                    break;
                }
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
        let saved_type_params = std::mem::take(&mut self.type_param_scope);
        for tp in &f.type_params {
            self.type_param_scope.insert(tp.clone());
        }
        for param in &f.params {
            self.symbols.variables.insert(
                param.name.clone(),
                (self.resolve_type(&param.param_type), false),
            );
        }
        let has_return = self.check_stmt(&f.body);
        let expected = self.resolve_type(&f.ret_type);
        if expected != ValueType::Unit && expected != ValueType::Never && !has_return {
            self.errors
                .push(Error::new(f.span, "missing return statement"));
        }
        self.type_param_scope = saved_type_params;
        self.symbols.exit_scope(saved_vars);
    }

    fn check_class(&mut self, c: &Class) {
        let saved_class = self.current_class.clone();
        self.current_class = Some(c.name.clone());
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
        self.current_class = saved_class;

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
            StmtKind::Select { arms, default } => {
                for arm in arms {
                    self.infer_type(&arm.channel);
                    let saved = self.symbols.enter_scope();
                    self.symbols.variables.insert(arm.var.clone(), (ValueType::Int, false));
                    self.check_stmt(&arm.body);
                    self.symbols.exit_scope(saved);
                }
                if let Some(d) = default {
                    self.check_stmt(d);
                }
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
                let class_name = obj_ty.to_string();
                let method_name = format!("{}_{}", class_name, method);

                if let Some(vis) = self.method_visibility.get(&method_name).cloned() {
                    self.check_member_visibility(&class_name, method, &vis, expr.span);
                }

                let func_expr = Spanned::new(ExprKind::Ident(method_name), expr.span);
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
                    if let Some(vis) = self.field_visibility.get(&full_name).cloned() {
                        self.check_member_visibility(&name, field, &vis, expr.span);
                    }
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
            ExprKind::SuperCall { method, args } => {
                // super calls are only valid inside a class method
                let current_class = match &self.current_class {
                    Some(c) => c.clone(),
                    None => {
                        self.errors.push(Error::new(
                            expr.span,
                            "super call is only valid inside a class method",
                        ));
                        for arg in args {
                            self.infer_type(arg);
                        }
                        return ValueType::Any;
                    }
                };
                // get the parent class
                let parent_class = match self.class_parents.get(&current_class).cloned() {
                    Some(p) => p,
                    None => {
                        self.errors.push(Error::new(
                            expr.span,
                            format!("class {} has no parent class for super call", current_class),
                        ));
                        for arg in args {
                            self.infer_type(arg);
                        }
                        return ValueType::Any;
                    }
                };
                // look up parent method in symbol table as ParentClass_methodName
                let parent_method_key = format!("{}_{}", parent_class, method);
                let sig = match self.symbols.functions.get(&parent_method_key).cloned() {
                    Some(s) => s,
                    None => {
                        self.errors.push(Error::new(
                            expr.span,
                            format!(
                                "parent class {} has no method '{}'",
                                parent_class, method
                            ),
                        ));
                        for arg in args {
                            self.infer_type(arg);
                        }
                        return ValueType::Any;
                    }
                };
                // sig.params[0] is self; check user-supplied args against params[1..]
                let expected_arg_count = sig.params.len().saturating_sub(1);
                if args.len() != expected_arg_count {
                    self.errors.push(
                        TypeError::InvalidArgumentCount {
                            expected: expected_arg_count,
                            found: args.len(),
                            span: expr.span,
                        }
                        .to_error(),
                    );
                }
                for (arg, (_, expected_ty)) in args.iter().zip(sig.params.iter().skip(1)) {
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
                sig.return_type.clone()
            }
            ExprKind::New { class, args, .. } => {
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
            ExprKind::Spawn(inner) => {
                self.infer_type(inner);
                ValueType::Int // task handle (opaque i64)
            }
            ExprKind::Await(inner) => {
                self.infer_type(inner);
                ValueType::Int // task result
            }
            ExprKind::Channel => ValueType::Int, // channel handle (opaque i64)
            ExprKind::Send { channel, value } => {
                self.infer_type(channel);
                self.infer_type(value);
                ValueType::Unit
            }
            ExprKind::Recv(inner) => {
                self.infer_type(inner);
                ValueType::Int // received value
            }
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
                let is_variadic_print = matches!(name.as_str(), "print" | "println");
                if !is_variadic_print && sig.params.len() != args.len() {
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
            BinaryOp::Add => {
                (matches!(lhs, ValueType::Int | ValueType::Float)
                    && matches!(rhs, ValueType::Int | ValueType::Float))
                    || (matches!(lhs, ValueType::String) && matches!(rhs, ValueType::String))
            }
            BinaryOp::Sub
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
            BinaryOp::Add if *lhs == ValueType::String => ValueType::String,
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

    /// Like `type_to_value` but resolves type parameters in scope to `Any`.
    fn resolve_type(&self, ty: &Type) -> ValueType {
        match ty {
            Type::Named(name) if self.type_param_scope.contains(name) => ValueType::Any,
            Type::Generic { name, .. } if self.type_param_scope.contains(name) => ValueType::Any,
            _ => Self::type_to_value(ty),
        }
    }

    /// Used during registration to erase type parameters to `Any`.
    fn type_to_value_erasing(ty: &Type, type_params: &[String]) -> ValueType {
        match ty {
            Type::Named(name) if type_params.contains(name) => ValueType::Any,
            Type::Generic { name, .. } if type_params.contains(name) => ValueType::Any,
            _ => Self::type_to_value(ty),
        }
    }

    fn is_subclass_or_equal(&self, candidate: &str, base: &str) -> bool {
        if candidate == base {
            return true;
        }
        let mut current = candidate.to_string();
        while let Some(parent) = self.class_parents.get(&current) {
            if parent == base {
                return true;
            }
            current = parent.clone();
        }
        false
    }

    fn check_member_visibility(
        &mut self,
        class: &str,
        member: &str,
        visibility: &Visibility,
        span: Span,
    ) {
        match visibility {
            Visibility::Public | Visibility::Package => {}
            Visibility::Private => {
                let allowed = self
                    .current_class
                    .as_deref()
                    .map(|c| c == class)
                    .unwrap_or(false);
                if !allowed {
                    self.errors.push(
                        TypeError::PrivateAccess {
                            class: class.to_string(),
                            member: member.to_string(),
                            span,
                        }
                        .to_error(),
                    );
                }
            }
            Visibility::Protected => {
                let allowed = self
                    .current_class
                    .as_deref()
                    .map(|c| self.is_subclass_or_equal(c, class))
                    .unwrap_or(false);
                if !allowed {
                    self.errors.push(
                        TypeError::ProtectedAccess {
                            class: class.to_string(),
                            member: member.to_string(),
                            span,
                        }
                        .to_error(),
                    );
                }
            }
        }
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

    fn parse_code(code: &str) -> Result<tinox_parser::SourceFile, tinox_common::ErrorBag> {
        let mut lexer = Lexer::new(code);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        parser.parse()
    }

    #[test]
    fn test_interface_extends_full_implementation() {
        // A class implementing IDrawable (which extends IShape) must implement both methods.
        // Interface methods require a body (parser constraint); use empty bodies here.
        let code = r#"
interface IShape {
    fn area() -> Int64 { return 0; }
}
interface IDrawable extends IShape {
    fn draw() { }
}
class Circle implements IDrawable {
    fn draw() { }
    fn area() -> Int64 { return 42; }
}
"#;
        let ast = parse_code(code).expect("parse should succeed");
        let mut checker = TypeChecker::new();
        let result = checker.check(&ast);
        assert!(result.is_ok(), "expected ok but got: {:?}", result);
    }

    #[test]
    fn test_interface_extends_missing_inherited_method() {
        // A class implementing IDrawable but missing area() (inherited from IShape) should fail.
        let code = r#"
interface IShape {
    fn area() -> Int64 { return 0; }
}
interface IDrawable extends IShape {
    fn draw() { }
}
class Circle implements IDrawable {
    fn draw() { }
}
"#;
        let ast = parse_code(code).expect("parse should succeed");
        let mut checker = TypeChecker::new();
        let result = checker.check(&ast);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(
            errors.errors.iter().any(|e| e.message.contains("area")),
            "expected error about missing 'area', got: {:?}",
            errors.errors
        );
    }

    #[test]
    fn test_interface_extends_undefined_parent() {
        // Extending an interface that doesn't exist should produce an error.
        let code = r#"
interface IDrawable extends IDoesNotExist {
    fn draw() { }
}
"#;
        let ast = parse_code(code).expect("parse should succeed");
        let mut checker = TypeChecker::new();
        let result = checker.check(&ast);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(
            errors.errors.iter().any(|e| e.message.contains("IDoesNotExist")),
            "expected error about undefined interface, got: {:?}",
            errors.errors
        );
    }
}
