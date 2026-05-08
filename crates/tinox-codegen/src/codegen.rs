use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::path::Path;
use tinox_common::{Error, ErrorBag, Span, Spanned};
use tinox_parser::{
    CatchClause, DeclKind, Expr, ExprKind, Literal, Method, Pattern,
    SourceFile, Stmt, StmtKind, Type,
};

#[derive(Debug, Clone, PartialEq)]
pub enum DiScope {
    Application,
    Startup,
    HttpRequest,
}

#[derive(Debug, Clone)]
pub struct DiInjectField {
    pub field_name: String,
    pub field_type: String,
}

#[derive(Debug, Clone)]
pub struct DiComponentInfo {
    pub class_name: String,
    pub scope: DiScope,
    pub inject_fields: Vec<DiInjectField>,
}

#[derive(Debug, Clone)]
pub struct ConfigFieldInfo {
    pub class_name: String,
    pub field_name: String,
    pub config_key: String,
    /// LLVM type: "i8*" for String, "i64" for Int*, "i1" for Bool
    pub field_llvm_type: String,
}

#[derive(Debug, Clone)]
pub struct CliOptionInfo {
    pub field_name: String,
    pub names: Vec<String>,
    pub description: String,
    pub required: bool,
    pub field_type: String,
}

#[derive(Debug, Clone)]
pub struct CliArgumentInfo {
    pub field_name: String,
    pub index: usize,
    pub description: String,
    pub required: bool,
    pub field_type: String,
}

#[derive(Debug, Clone)]
pub struct CliCommandInfo {
    pub class_name: String,
    pub cmd_name: String,
    pub description: String,
    pub version: Option<String>,
    pub options: Vec<CliOptionInfo>,
    pub arguments: Vec<CliArgumentInfo>,
}

/// Route entry produced by REST annotation processing.
#[derive(Debug, Clone)]
pub struct RouteEntry {
    pub http_method: String,
    pub path: String,
    pub class_name: String,
    pub method_name: String,
    pub status_code: Option<i64>,
    pub produces: Option<String>,
    pub consumes: Option<String>,
    pub auth_type: Option<String>,
    /// true = fnc (static), false = fn (instance, has self)
    pub is_static: bool,
}

pub struct CodeGen {
    ir: String,
    lambda_ir: String,
    strings: HashMap<String, String>,
    temp_count: usize,
    struct_layouts: HashMap<String, Vec<String>>,
    #[allow(dead_code)]
    closure_envs: HashMap<String, String>,
    method_ret_types: HashMap<String, String>,
    // vtable support
    /// interface_name -> ordered method names (vtable slot order)
    vtable_layouts: HashMap<String, Vec<String>>,
    /// class_name -> list of interfaces it implements
    class_implements: HashMap<String, Vec<String>>,
    /// set of class names that have a vtable pointer at slot 0
    classes_with_vtable: HashSet<String>,
    /// set of known interface names (for dispatch decisions)
    known_interfaces: HashSet<String>,
    /// child_class_name -> parent_class_name (for super calls)
    class_parents: HashMap<String, String>,
    /// class_name -> number of entries in its vtable global (computed during emit_vtable_globals)
    vtable_sizes: HashMap<String, usize>,
    /// ClassName_methodName -> OwnerClassName_methodName (resolved through inheritance)
    method_impl: HashMap<String, String>,
    /// fn_name -> (ret_llvm_ty, param_llvm_tys) for spawn codegen
    fn_sigs: HashMap<String, (String, Vec<String>)>,
    spawn_counter: usize,
    /// Generic function AST nodes (not directly compiled, monomorphized on demand)
    generic_fns: HashMap<String, tinox_parser::Function>,
    /// Generic class AST nodes (not directly compiled, monomorphized on demand)
    generic_classes: HashMap<String, tinox_parser::Class>,
    /// Already-generated specializations (mangled_name already emitted)
    generated_specializations: HashSet<String>,
    /// Set of all enum variant names (for bare-name match patterns)
    known_enum_variants: HashSet<String>,
    /// Set of enum type names (for type_to_llvm: enums are i64, not i64*)
    known_enum_types: HashSet<String>,
    /// Annotation processing: functions annotated @inline
    inline_functions: HashSet<String>,
    /// Annotation processing: (class_name, method_name) pairs for methods annotated @inline
    inline_methods: HashSet<(String, String)>,
    /// REST route entries collected from annotation processing
    route_entries: Vec<RouteEntry>,
    /// DI component info from annotation processing
    di_components: Vec<DiComponentInfo>,
    /// Class names that have @Log — get a synthetic 'log: Logger' field
    log_classes: HashSet<String>,
    /// Fields annotated with @Config — injected from application.properties at construction
    config_fields: Vec<ConfigFieldInfo>,
    /// CLI commands collected from @Command annotation processing
    cli_commands: Vec<CliCommandInfo>,
    /// If set, emit a test-runner main that calls this (class, method) and exits 0/1
    test_entry: Option<(String, String)>,
    /// Whether a user-defined main function was compiled (prevents auto-generated main)
    has_main: bool,
    /// Set of class names defined in user/imported code
    defined_classes: HashSet<String>,
    /// class_name -> field_name -> class_type_name (only for fields with Named/class types)
    struct_field_class_types: HashMap<String, HashMap<String, String>>,
    /// class_name -> field_name -> llvm_type (for FieldAccess type recovery)
    struct_field_llvm_types: HashMap<String, HashMap<String, String>>,
    /// class_name -> field_name -> (ret_llvm_ty, param_llvm_tys) for Type::Fn fields
    fn_field_sigs: HashMap<String, HashMap<String, (String, Vec<String>)>>,
}

impl CodeGen {
    pub fn new() -> Self {
        Self {
            ir: String::new(),
            lambda_ir: String::new(),
            strings: HashMap::new(),
            temp_count: 0,
            struct_layouts: HashMap::new(),
            closure_envs: HashMap::new(),
            method_ret_types: HashMap::new(),
            vtable_layouts: HashMap::new(),
            class_implements: HashMap::new(),
            classes_with_vtable: HashSet::new(),
            known_interfaces: HashSet::new(),
            class_parents: HashMap::new(),
            vtable_sizes: HashMap::new(),
            method_impl: HashMap::new(),
            fn_sigs: HashMap::new(),
            spawn_counter: 0,
            generic_fns: HashMap::new(),
            generic_classes: HashMap::new(),
            generated_specializations: HashSet::new(),
            known_enum_variants: HashSet::new(),
            known_enum_types: HashSet::new(),
            inline_functions: HashSet::new(),
            inline_methods: HashSet::new(),
            route_entries: Vec::new(),
            di_components: Vec::new(),
            log_classes: HashSet::new(),
            config_fields: Vec::new(),
            cli_commands: Vec::new(),
            test_entry: None,
            has_main: false,
            defined_classes: HashSet::new(),
            struct_field_class_types: HashMap::new(),
            struct_field_llvm_types: HashMap::new(),
            fn_field_sigs: HashMap::new(),
        }
    }

    /// Provide annotation metadata from the type checker annotation processing.
    pub fn set_annotation_info(
        &mut self,
        inline_fns: HashSet<String>,
        inline_meths: HashSet<(String, String)>,
        routes: Vec<RouteEntry>,
        di_components: Vec<DiComponentInfo>,
        log_classes: HashSet<String>,
        config_fields: Vec<ConfigFieldInfo>,
        cli_commands: Vec<CliCommandInfo>,
    ) {
        self.inline_functions = inline_fns;
        self.inline_methods = inline_meths;
        self.route_entries = routes;
        self.di_components = di_components;
        self.log_classes = log_classes;
        self.config_fields = config_fields;
        self.cli_commands = cli_commands;
    }

    /// Configure a single test to run: generates `tinox_main` that calls
    /// `ClassName_methodName()` and exits 0 (pass) or 1 (fail via panic).
    pub fn set_test_entry(&mut self, class_name: String, method_name: String) {
        self.test_entry = Some((class_name, method_name));
    }

    /// Provide interface metadata from the type checker.
    /// Must be called before `gen()`.
    pub fn set_interface_info(
        &mut self,
        vtable_layouts: HashMap<String, Vec<String>>,
        class_implements: HashMap<String, Vec<String>>,
    ) {
        self.known_interfaces = vtable_layouts.keys().cloned().collect();
        self.vtable_layouts = vtable_layouts;
        self.class_implements = class_implements;
        // Determine which classes have vtables
        for (class_name, ifaces) in &self.class_implements {
            if !ifaces.is_empty() {
                self.classes_with_vtable.insert(class_name.clone());
            }
        }
    }

    /// Collect all field names for a class in inheritance order: ancestor fields first, own last.
    fn collect_inherited_fields(
        name: &str,
        class_map: &HashMap<String, tinox_parser::Class>,
    ) -> Vec<String> {
        let Some(c) = class_map.get(name) else {
            return vec![];
        };
        let mut fields: Vec<String> = if let Some(parent) = &c.extends {
            Self::collect_inherited_fields(parent, class_map)
        } else {
            vec![]
        };
        for f in &c.fields {
            if !fields.contains(&f.name) {
                fields.push(f.name.clone());
            }
        }
        fields
    }

    /// Collect field_name -> class_type_name for all Named-typed fields (including inherited).
    fn collect_field_class_types(
        name: &str,
        class_map: &HashMap<String, tinox_parser::Class>,
    ) -> HashMap<String, String> {
        let Some(c) = class_map.get(name) else { return HashMap::new(); };
        let mut result = if let Some(parent) = &c.extends {
            Self::collect_field_class_types(parent, class_map)
        } else {
            HashMap::new()
        };
        for f in &c.fields {
            if let Some(class_name) = Self::extract_class_type_name(&f.field_type) {
                result.insert(f.name.clone(), class_name);
            }
        }
        result
    }

    /// Collect field_name -> (ret_llvm_ty, param_llvm_tys) for all Type::Fn fields (including inherited).
    fn collect_fn_field_sigs(
        name: &str,
        class_map: &HashMap<String, tinox_parser::Class>,
    ) -> HashMap<String, (String, Vec<String>)> {
        let Some(c) = class_map.get(name) else { return HashMap::new(); };
        let mut result = if let Some(parent) = &c.extends {
            Self::collect_fn_field_sigs(parent, class_map)
        } else {
            HashMap::new()
        };
        for f in &c.fields {
            if let tinox_parser::Type::Fn { params, ret } = &f.field_type {
                let ret_ty = Self::type_to_llvm(ret);
                let param_tys: Vec<String> = params.iter().map(|p| Self::type_to_llvm(p)).collect();
                result.insert(f.name.clone(), (ret_ty, param_tys));
            }
        }
        result
    }

    /// Collect field_name -> llvm_type for all fields (including inherited).
    fn collect_field_llvm_types(
        name: &str,
        class_map: &HashMap<String, tinox_parser::Class>,
    ) -> HashMap<String, String> {
        let Some(c) = class_map.get(name) else { return HashMap::new(); };
        let mut result = if let Some(parent) = &c.extends {
            Self::collect_field_llvm_types(parent, class_map)
        } else {
            HashMap::new()
        };
        for f in &c.fields {
            result.insert(f.name.clone(), Self::type_to_llvm(&f.field_type));
        }
        result
    }

    fn extract_class_type_name(ty: &tinox_parser::Type) -> Option<String> {
        use tinox_parser::Type;
        match ty {
            Type::Named(n) => Some(n.clone()),
            Type::Generic { name, .. } => Some(name.clone()),
            Type::Mutable(inner) | Type::Ref(inner) | Type::Array(inner) => {
                Self::extract_class_type_name(inner)
            }
            _ => None,
        }
    }

    /// Infer the struct/class type name for an expression (for nested field access).
    fn infer_struct_type<'a>(&'a self, expr: &tinox_parser::Expr, ctx: &GenCtx) -> Option<String> {
        use tinox_parser::ExprKind;
        match &expr.node {
            ExprKind::Ident(name) => ctx.local_types.get(name).cloned(),
            ExprKind::This => ctx.current_struct.clone(),
            ExprKind::FieldAccess { obj, field } => {
                let outer = self.infer_struct_type(obj, ctx)?;
                self.struct_field_class_types
                    .get(&outer)
                    .and_then(|m| m.get(field.as_str()))
                    .cloned()
            }
            _ => None,
        }
    }

    /// Resolve method call to the class that actually implements it (walks parent chain).
    fn resolve_method_owner(
        class: &str,
        method: &str,
        class_map: &HashMap<String, tinox_parser::Class>,
    ) -> String {
        let mut current = class.to_string();
        loop {
            if let Some(c) = class_map.get(&current) {
                if c.methods.iter().any(|m| m.name == method) {
                    return format!("{}_{}", current, method);
                }
                match &c.extends {
                    Some(parent) => current = parent.clone(),
                    None => break,
                }
            } else {
                break;
            }
        }
        format!("{}_{}", class, method)
    }

    pub fn gen(&mut self, source: &SourceFile) -> Result<(), ErrorBag> {
        writeln!(&mut self.ir, "; Module ID = \"tinox\"").unwrap();
        writeln!(&mut self.ir, "source_filename = \"tinox\"").unwrap();
        writeln!(
            &mut self.ir,
            "target datalayout = \"e-m:e-i64:64-f80:128-n8:16:32:64-S128\""
        )
        .unwrap();
        writeln!(&mut self.ir, "target triple = \"x86_64-unknown-linux-gnu\"").unwrap();
        writeln!(&mut self.ir).unwrap();
        writeln!(&mut self.ir, "declare void @tinox_print_int(i64)").unwrap();
        writeln!(&mut self.ir, "declare void @tinox_print_string(i8*)").unwrap();
        writeln!(&mut self.ir, "declare void @tinox_print_float(double)").unwrap();
        writeln!(&mut self.ir, "declare void @tinox_print_bool(i1)").unwrap();
        writeln!(&mut self.ir, "declare void @tinox_print_newline()").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_alloc(i64)").unwrap();
        writeln!(&mut self.ir, "declare void @tinox_panic(i64)").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_task_spawn(i8* (i8*)*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_task_await(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_channel_create()").unwrap();
        writeln!(&mut self.ir, "declare void @tinox_channel_send(i8*, i64)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_channel_recv(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i1 @tinox_channel_try_recv(i8*, i64*)").unwrap();
        writeln!(&mut self.ir, "declare i32 @sched_yield()").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_string_length(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_string_concat(i8*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_int_to_string(i64)").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_float_to_string(double)").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_bool_to_string(i1)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_string_to_int(i8*)").unwrap();
        writeln!(&mut self.ir, "declare double @tinox_string_to_float(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_char_at(i8*, i64)").unwrap();
        writeln!(&mut self.ir, "declare void @tinox_print_char(i32)").unwrap();
        writeln!(&mut self.ir, "declare i64* @tinox_array_push(i64*, i64)").unwrap();
        writeln!(&mut self.ir, "declare i64* @tinox_array_pop(i64*)").unwrap();
        writeln!(&mut self.ir, "declare i64* @tinox_array_slice(i64*, i64, i64)").unwrap();
        writeln!(&mut self.ir, "declare double @sqrt(double)").unwrap();
        writeln!(&mut self.ir, "declare double @pow(double, double)").unwrap();
        writeln!(&mut self.ir, "declare double @llvm.fabs.f64(double)").unwrap();
        writeln!(&mut self.ir, "declare double @llvm.floor.f64(double)").unwrap();
        writeln!(&mut self.ir, "declare double @llvm.ceil.f64(double)").unwrap();
        writeln!(&mut self.ir, "declare double @llvm.round.f64(double)").unwrap();
        writeln!(&mut self.ir, "declare void @exit(i64)").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_config_get(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_config_get_int(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_config_get_bool(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_string_contains(i8*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_string_index_of(i8*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_string_to_upper(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_string_to_lower(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_string_starts_with(i8*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_string_ends_with(i8*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_string_trim(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_string_substring(i8*, i64, i64)").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_string_replace(i8*, i8*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64* @tinox_string_split(i8*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_string_join(i64*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64* @tinox_array_sort(i64*)").unwrap();
        writeln!(&mut self.ir, "declare i64* @tinox_array_reverse(i64*)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_array_contains(i64*, i64)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_array_index_of(i64*, i64)").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_map_create()").unwrap();
        writeln!(&mut self.ir, "declare void @tinox_map_set(i8*, i8*, i64)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_map_get(i8*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_map_contains(i8*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare void @tinox_map_remove(i8*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_map_len(i8*)").unwrap();
        writeln!(&mut self.ir, "declare void @tinox_map_free(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64* @tinox_map_keys(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64* @tinox_map_values(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_file_open(i8*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare void @tinox_file_close(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_file_read(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_file_readline(i8*)").unwrap();
        writeln!(&mut self.ir, "declare void @tinox_file_write(i8*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_file_eof(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_file_exists(i8*)").unwrap();
        writeln!(&mut self.ir, "declare void @tinox_file_delete(i8*)").unwrap();
        // HTTP server C runtime (low-level)
        writeln!(&mut self.ir, "declare i64 @httpServerCreate(i64)").unwrap();
        writeln!(&mut self.ir, "declare i64 @httpServerAcceptConn(i64)").unwrap();
        writeln!(&mut self.ir, "declare i8* @httpServerReadRequest(i64)").unwrap();
        writeln!(&mut self.ir, "declare void @httpServerSendRaw(i64, i8*)").unwrap();
        writeln!(&mut self.ir, "declare void @httpServerCloseConn(i64)").unwrap();
        writeln!(&mut self.ir, "declare void @httpServerClose(i64)").unwrap();
        // CLI helpers (@Command / @Option / @Argument)
        writeln!(&mut self.ir, "declare i8* @tinox_cli_get_string(i8*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_cli_has_flag(i8*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_cli_get_int(i8*, i8*, i64)").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_cli_get_positional(i32)").unwrap();
        writeln!(&mut self.ir, "declare void @tinox_cli_print_option(i8*, i8*)").unwrap();
        writeln!(&mut self.ir).unwrap();

        // Build class AST map for inheritance helpers.
        let class_ast_map: HashMap<String, tinox_parser::Class> = source
            .decls
            .iter()
            .flat_map(|d| {
                let mut v: Vec<tinox_parser::Class> = Vec::new();
                match &d.node {
                    DeclKind::Class(c) => v.push(c.clone()),
                    DeclKind::Namespace(ns) => {
                        for inner in &ns.decls {
                            if let DeclKind::Class(c) = &inner.node {
                                v.push(c.clone());
                            }
                        }
                    }
                    _ => {}
                }
                v
            })
            .map(|c| (c.name.clone(), c))
            .collect();

        // First pass: build struct_layouts (with vtable slot at index 0 where needed)
        // and method_impl (for inherited method dispatch). Handles both top-level and
        // namespace-scoped classes.
        let all_classes: Vec<tinox_parser::Class> = source.decls.iter().flat_map(|d| {
            let mut v: Vec<tinox_parser::Class> = Vec::new();
            match &d.node {
                DeclKind::Class(c) => v.push(c.clone()),
                DeclKind::Namespace(ns) => {
                    for inner in &ns.decls {
                        if let DeclKind::Class(c) = &inner.node { v.push(c.clone()); }
                    }
                }
                _ => {}
            }
            v
        }).collect();

        // Register immutable struct layouts and new() return types (top-level + namespace-scoped)
        let all_immutables: Vec<tinox_parser::ImmutableDecl> = source.decls.iter().flat_map(|d| {
            let mut v = Vec::new();
            match &d.node {
                DeclKind::Immutable(u) => v.push(u.clone()),
                DeclKind::Namespace(ns) => {
                    for inner in &ns.decls {
                        if let DeclKind::Immutable(u) = &inner.node { v.push(u.clone()); }
                    }
                }
                _ => {}
            }
            v
        }).collect();

        for u in &all_immutables {
            self.defined_classes.insert(u.name.clone());
            let fields: Vec<String> = u.fields.iter().map(|f| f.name.clone()).collect();
            self.struct_layouts.insert(u.name.clone(), fields);
            let mut fct: HashMap<String, String> = HashMap::new();
            for field in &u.fields {
                if let tinox_parser::Type::Named(class_name) = &field.param_type {
                    fct.insert(field.name.clone(), class_name.clone());
                }
            }
            self.struct_field_class_types.insert(u.name.clone(), fct);
            let fllt: HashMap<String, String> = u.fields.iter()
                .map(|f| (f.name.clone(), Self::type_to_llvm(&f.param_type)))
                .collect();
            self.struct_field_llvm_types.insert(u.name.clone(), fllt);
            let fn_sigs: HashMap<String, (String, Vec<String>)> = u.fields.iter()
                .filter_map(|f| {
                    if let tinox_parser::Type::Fn { params, ret } = &f.param_type {
                        let r = Self::type_to_llvm(ret);
                        let ps: Vec<String> = params.iter().map(|p| Self::type_to_llvm(p)).collect();
                        Some((f.name.clone(), (r, ps)))
                    } else { None }
                })
                .collect();
            self.fn_field_sigs.insert(u.name.clone(), fn_sigs);
            self.method_ret_types.insert(format!("{}_new", u.name), "i64*".to_string());
        }

        for c in &all_classes {
            self.defined_classes.insert(c.name.clone());
            {
                if !c.type_params.is_empty() { continue; }
                if let Some(parent) = &c.extends {
                    self.class_parents.insert(c.name.clone(), parent.clone());
                }
                let has_vtable = !c.implements.is_empty()
                    || self.classes_with_vtable.contains(&c.name);
                let mut fields: Vec<String> = Vec::new();
                if has_vtable {
                    fields.push("__vtable__".to_string());
                    self.classes_with_vtable.insert(c.name.clone());
                    let mut vtable_methods: Vec<String> = Vec::new();
                    let mut seen: HashSet<String> = HashSet::new();
                    for iface in &c.implements {
                        if let Some(methods) = self.vtable_layouts.get(iface) {
                            for m in methods {
                                if seen.insert(m.clone()) {
                                    vtable_methods.push(m.clone());
                                }
                            }
                        }
                    }
                    self.vtable_sizes.insert(c.name.clone(), vtable_methods.len());
                }
                fields.extend(Self::collect_inherited_fields(&c.name, &class_ast_map));
                if c.annotations.iter().any(|a| a.name == "Log") {
                    fields.push("log".to_string());
                }
                self.struct_layouts.insert(c.name.clone(), fields);
                let mut fct = Self::collect_field_class_types(&c.name, &class_ast_map);
                if c.annotations.iter().any(|a| a.name == "Log") {
                    fct.insert("log".to_string(), "Logger".to_string());
                }
                self.struct_field_class_types.insert(c.name.clone(), fct);
                let mut fllt = Self::collect_field_llvm_types(&c.name, &class_ast_map);
                if c.annotations.iter().any(|a| a.name == "Log") {
                    fllt.insert("log".to_string(), "i64*".to_string());
                }
                self.struct_field_llvm_types.insert(c.name.clone(), fllt);
                let fn_sigs = Self::collect_fn_field_sigs(&c.name, &class_ast_map);
                self.fn_field_sigs.insert(c.name.clone(), fn_sigs);

                for method in &c.methods {
                    let key = format!("{}_{}", c.name, method.name);
                    self.method_impl.insert(key.clone(), key);
                    self.method_ret_types.insert(
                        format!("{}_{}", c.name, method.name),
                        Self::type_to_llvm(&method.ret_type),
                    );
                }
                let own_method_names: HashSet<String> =
                    c.methods.iter().map(|m| m.name.clone()).collect();
                let mut ancestor = c.extends.clone();
                while let Some(ref aname) = ancestor.clone() {
                    let Some(ac) = class_ast_map.get(aname) else { break; };
                    for method in &ac.methods {
                        if !own_method_names.contains(&method.name) {
                            let child_key = format!("{}_{}", c.name, method.name);
                            if !self.method_impl.contains_key(&child_key) {
                                let owner_key = Self::resolve_method_owner(
                                    aname, &method.name, &class_ast_map,
                                );
                                self.method_impl.insert(child_key.clone(), owner_key.clone());
                                self.method_ret_types.insert(child_key, Self::type_to_llvm(&method.ret_type));
                            }
                        }
                    }
                    ancestor = ac.extends.clone();
                }
            }
        }

        // Pre-pass: collect all function signatures; store generic fns/classes separately
        for decl in &source.decls {
            match &decl.node {
                DeclKind::Function(f) => {
                    if !f.type_params.is_empty() {
                        self.generic_fns.insert(f.name.clone(), f.clone());
                    } else {
                        let fn_name = if f.name == "main" { "tinox_main".to_string() } else { f.name.clone() };
                        let ret_ty = self.type_to_llvm_inst(&f.ret_type);
                        let param_tys: Vec<String> = f.params.iter().map(|p| Self::type_to_llvm(&p.param_type)).collect();
                        self.fn_sigs.insert(fn_name, (ret_ty, param_tys));
                    }
                }
                DeclKind::Class(c) if !c.type_params.is_empty() => {
                    self.generic_classes.insert(c.name.clone(), c.clone());
                }
                DeclKind::Enum(e) => {
                    self.known_enum_types.insert(e.name.clone());
                    for variant in &e.variants {
                        self.known_enum_variants.insert(variant.name.clone());
                    }
                }
                DeclKind::Namespace(ns) => {
                    for inner in &ns.decls {
                        if let DeclKind::Class(c) = &inner.node {
                            if !c.type_params.is_empty() {
                                self.generic_classes.insert(c.name.clone(), c.clone());
                            }
                        } else if let DeclKind::Enum(e) = &inner.node {
                            self.known_enum_types.insert(e.name.clone());
                            for variant in &e.variants {
                                self.known_enum_variants.insert(variant.name.clone());
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Second pass: generate code (skip generic functions — they are monomorphized on demand)
        for decl in &source.decls {
            match &decl.node {
                DeclKind::Function(f) => {
                    if f.type_params.is_empty() {
                        self.gen_fn(f)?;
                    }
                }
                DeclKind::Class(c) if c.type_params.is_empty() => {
                    for method in &c.methods {
                        self.gen_class_method(&c.name, method)?;
                    }
                }
                DeclKind::Immutable(u) => {
                    self.emit_immutable_new(u);
                }
                DeclKind::Namespace(ns) => {
                    for inner in &ns.decls {
                        match &inner.node {
                            DeclKind::Class(c) => {
                                if c.type_params.is_empty() {
                                    for method in &c.methods {
                                        self.gen_class_method(&c.name, method)?;
                                    }
                                }
                            }
                            DeclKind::Immutable(u) => {
                                self.emit_immutable_new(u);
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        // Emit vtable globals for classes that implement interfaces
        self.emit_vtable_globals(source);

        // Emit REST route shims and registration function
        self.emit_route_code();

        // Emit DI globals, getters, factories, and startup initializer
        self.emit_di_code();

        // Emit CLI main (tinox_main) for @Command classes
        self.emit_cli_code();

        // Emit test-runner main if set_test_entry() was called
        self.emit_test_code();

        for (name, s) in &self.strings {
            let escaped = Self::escape_llvm_string(s);
            writeln!(
                &mut self.ir,
                "@{} = private constant [{} x i8] c\"{}\\00\"",
                name,
                s.len() + 1,
                escaped
            )
            .unwrap();
        }

        Ok(())
    }

    /// Generates route handler shims, `__tinox_register_routes`, and (if needed) a `main`
    /// for all routes collected via REST annotations (@GET, @POST, …).
    fn emit_route_code(&mut self) {
        if self.route_entries.is_empty() {
            return;
        }

        // External declares for the C runtime route-based HTTP server API.
        // tinox_HttpServer_* are distinct from any user-defined HttpServer class methods.
        writeln!(&mut self.lambda_ir, "declare i64* @tinox_HttpServer_new(i64)").unwrap();
        writeln!(&mut self.lambda_ir, "declare void @tinox_HttpServer_get(i64*, i8*, i64)").unwrap();
        writeln!(&mut self.lambda_ir, "declare void @tinox_HttpServer_post(i64*, i8*, i64)").unwrap();
        writeln!(&mut self.lambda_ir, "declare void @tinox_HttpServer_put(i64*, i8*, i64)").unwrap();
        writeln!(&mut self.lambda_ir, "declare void @tinox_HttpServer_patch(i64*, i8*, i64)").unwrap();
        writeln!(&mut self.lambda_ir, "declare void @tinox_HttpServer_delete(i64*, i8*, i64)").unwrap();
        writeln!(&mut self.lambda_ir, "declare void @tinox_HttpServer_listen(i64*)").unwrap();
        writeln!(&mut self.lambda_ir).unwrap();

        let routes = self.route_entries.clone();

        // ── String constant globals for annotations ─────────────────────────────
        // Emitted into self.ir before lambda_ir is appended.
        for (idx, route) in routes.iter().enumerate() {
            let path = &route.path;
            let escaped = Self::escape_llvm_string(path);
            writeln!(&mut self.ir,
                "@__route_path_{idx} = private constant [{} x i8] c\"{escaped}\\00\"",
                path.len() + 1).unwrap();

            if let Some(ref ct) = route.produces {
                let esc = Self::escape_llvm_string(ct);
                writeln!(&mut self.ir,
                    "@__route_produces_{idx} = private constant [{} x i8] c\"{esc}\\00\"",
                    ct.len() + 1).unwrap();
            }
            if let Some(ref ct) = route.consumes {
                let esc = Self::escape_llvm_string(ct);
                writeln!(&mut self.ir,
                    "@__route_consumes_{idx} = private constant [{} x i8] c\"{esc}\\00\"",
                    ct.len() + 1).unwrap();
            }
            if let Some(ref auth) = route.auth_type {
                // "Bearer " or "Basic " prefix for Authorization header check
                let prefix = format!("{} ", Self::capitalize_first(auth));
                let esc = Self::escape_llvm_string(&prefix);
                writeln!(&mut self.ir,
                    "@__route_auth_prefix_{idx} = private constant [{} x i8] c\"{esc}\\00\"",
                    prefix.len() + 1).unwrap();
            }
        }
        // Static string constants shared across shims
        writeln!(&mut self.ir,
            "@__hdr_content_type = private constant [13 x i8] c\"Content-Type\\00\"").unwrap();
        writeln!(&mut self.ir,
            "@__hdr_authorization = private constant [14 x i8] c\"Authorization\\00\"").unwrap();
        writeln!(&mut self.ir,
            "@__str_401 = private constant [13 x i8] c\"Unauthorized\\00\"").unwrap();
        writeln!(&mut self.ir,
            "@__str_415 = private constant [23 x i8] c\"Unsupported Media Type\\00\"").unwrap();

        // ── Shim functions ──────────────────────────────────────────────────────
        // Signature: void (i64) — ctx_i64 is a ptrtoint of the HttpContext* pointer.
        //
        // HttpContext layout (no vtable): [request: i64*, response: i64*]  → offsets 0, 1
        // HttpResponse layout:            [statusCode: i64, headers: i8*, body: i8*] → offsets 0, 1, 2
        // HttpRequest layout:             [method, path, queryString, headers, body, params] → offset 3 = headers
        for (idx, route) in routes.iter().enumerate() {
            let shim = format!("__route_{}_{}", route.class_name, route.method_name);
            let method_fn = format!("{}_{}", route.class_name, route.method_name);
            let ctrl_size = self
                .struct_layouts
                .get(&route.class_name)
                .map(|f| f.len().max(1) * 8)
                .unwrap_or(8);

            writeln!(&mut self.lambda_ir, "define void @{shim}(i64 %ctx_i64) {{").unwrap();
            writeln!(&mut self.lambda_ir, "entry:").unwrap();
            writeln!(&mut self.lambda_ir, "  %ctx_ptr = inttoptr i64 %ctx_i64 to i64*").unwrap();

            // ── @Auth guard ──────────────────────────────────────────────────────
            if let Some(ref _auth) = route.auth_type {
                // Load request.headers (HttpContext[0] = request, HttpRequest[3] = headers)
                writeln!(&mut self.lambda_ir, "  %req_field_{idx} = getelementptr i64, i64* %ctx_ptr, i64 0").unwrap();
                writeln!(&mut self.lambda_ir, "  %req_i64_{idx} = load i64, i64* %req_field_{idx}").unwrap();
                writeln!(&mut self.lambda_ir, "  %req_ptr_{idx} = inttoptr i64 %req_i64_{idx} to i64*").unwrap();
                writeln!(&mut self.lambda_ir, "  %req_hdrs_field_{idx} = getelementptr i64, i64* %req_ptr_{idx}, i64 3").unwrap();
                writeln!(&mut self.lambda_ir, "  %req_hdrs_i64_{idx} = load i64, i64* %req_hdrs_field_{idx}").unwrap();
                writeln!(&mut self.lambda_ir, "  %req_hdrs_{idx} = inttoptr i64 %req_hdrs_i64_{idx} to i8*").unwrap();
                writeln!(&mut self.lambda_ir, "  %auth_key_{idx} = getelementptr [14 x i8], [14 x i8]* @__hdr_authorization, i64 0, i64 0").unwrap();
                writeln!(&mut self.lambda_ir, "  %auth_val_{idx} = call i64 @tinox_map_get(i8* %req_hdrs_{idx}, i8* %auth_key_{idx})").unwrap();
                writeln!(&mut self.lambda_ir, "  %auth_str_{idx} = inttoptr i64 %auth_val_{idx} to i8*").unwrap();
                // Get prefix string
                let prefix_len = _auth.len() + 2; // "Bearer " or "Basic "
                writeln!(&mut self.lambda_ir, "  %auth_prefix_{idx} = getelementptr [{prefix_len} x i8], [{prefix_len} x i8]* @__route_auth_prefix_{idx}, i64 0, i64 0").unwrap();
                writeln!(&mut self.lambda_ir, "  %auth_ok_{idx} = call i64 @tinox_string_starts_with(i8* %auth_str_{idx}, i8* %auth_prefix_{idx})").unwrap();
                writeln!(&mut self.lambda_ir, "  %auth_cmp_{idx} = icmp eq i64 %auth_ok_{idx}, 1").unwrap();
                writeln!(&mut self.lambda_ir, "  br i1 %auth_cmp_{idx}, label %auth_pass_{idx}, label %auth_fail_{idx}").unwrap();
                writeln!(&mut self.lambda_ir, "auth_fail_{idx}:").unwrap();
                // Set 401 status and body then return
                writeln!(&mut self.lambda_ir, "  %resp_f401_{idx} = getelementptr i64, i64* %ctx_ptr, i64 1").unwrap();
                writeln!(&mut self.lambda_ir, "  %resp_i401_{idx} = load i64, i64* %resp_f401_{idx}").unwrap();
                writeln!(&mut self.lambda_ir, "  %resp_p401_{idx} = inttoptr i64 %resp_i401_{idx} to i64*").unwrap();
                writeln!(&mut self.lambda_ir, "  %sc_f401_{idx} = getelementptr i64, i64* %resp_p401_{idx}, i64 0").unwrap();
                writeln!(&mut self.lambda_ir, "  store i64 401, i64* %sc_f401_{idx}").unwrap();
                writeln!(&mut self.lambda_ir, "  ret void").unwrap();
                writeln!(&mut self.lambda_ir, "auth_pass_{idx}:").unwrap();
            }

            // ── @Consumes: validate request Content-Type ─────────────────────────
            if let Some(ref expected_ct) = route.consumes {
                let ct_len = expected_ct.len() + 1;
                writeln!(&mut self.lambda_ir, "  %req_fct_{idx} = getelementptr i64, i64* %ctx_ptr, i64 0").unwrap();
                writeln!(&mut self.lambda_ir, "  %req_ict_{idx} = load i64, i64* %req_fct_{idx}").unwrap();
                writeln!(&mut self.lambda_ir, "  %req_pct_{idx} = inttoptr i64 %req_ict_{idx} to i64*").unwrap();
                writeln!(&mut self.lambda_ir, "  %req_hf_ct_{idx} = getelementptr i64, i64* %req_pct_{idx}, i64 3").unwrap();
                writeln!(&mut self.lambda_ir, "  %req_hi_ct_{idx} = load i64, i64* %req_hf_ct_{idx}").unwrap();
                writeln!(&mut self.lambda_ir, "  %req_hm_ct_{idx} = inttoptr i64 %req_hi_ct_{idx} to i8*").unwrap();
                writeln!(&mut self.lambda_ir, "  %ct_key_{idx} = getelementptr [13 x i8], [13 x i8]* @__hdr_content_type, i64 0, i64 0").unwrap();
                writeln!(&mut self.lambda_ir, "  %req_ct_val_{idx} = call i64 @tinox_map_get(i8* %req_hm_ct_{idx}, i8* %ct_key_{idx})").unwrap();
                writeln!(&mut self.lambda_ir, "  %req_ct_str_{idx} = inttoptr i64 %req_ct_val_{idx} to i8*").unwrap();
                writeln!(&mut self.lambda_ir, "  %expected_ct_{idx} = getelementptr [{ct_len} x i8], [{ct_len} x i8]* @__route_consumes_{idx}, i64 0, i64 0").unwrap();
                writeln!(&mut self.lambda_ir, "  %ct_match_{idx} = call i64 @tinox_string_starts_with(i8* %req_ct_str_{idx}, i8* %expected_ct_{idx})").unwrap();
                writeln!(&mut self.lambda_ir, "  %ct_ok_{idx} = icmp eq i64 %ct_match_{idx}, 1").unwrap();
                writeln!(&mut self.lambda_ir, "  br i1 %ct_ok_{idx}, label %ct_pass_{idx}, label %ct_fail_{idx}").unwrap();
                writeln!(&mut self.lambda_ir, "ct_fail_{idx}:").unwrap();
                writeln!(&mut self.lambda_ir, "  %resp_f415_{idx} = getelementptr i64, i64* %ctx_ptr, i64 1").unwrap();
                writeln!(&mut self.lambda_ir, "  %resp_i415_{idx} = load i64, i64* %resp_f415_{idx}").unwrap();
                writeln!(&mut self.lambda_ir, "  %resp_p415_{idx} = inttoptr i64 %resp_i415_{idx} to i64*").unwrap();
                writeln!(&mut self.lambda_ir, "  %sc_f415_{idx} = getelementptr i64, i64* %resp_p415_{idx}, i64 0").unwrap();
                writeln!(&mut self.lambda_ir, "  store i64 415, i64* %sc_f415_{idx}").unwrap();
                writeln!(&mut self.lambda_ir, "  ret void").unwrap();
                writeln!(&mut self.lambda_ir, "ct_pass_{idx}:").unwrap();
            }

            // ── @StatusCode: set default response status before handler runs ────
            if let Some(sc) = route.status_code {
                writeln!(&mut self.lambda_ir, "  %resp_fsc_{idx} = getelementptr i64, i64* %ctx_ptr, i64 1").unwrap();
                writeln!(&mut self.lambda_ir, "  %resp_isc_{idx} = load i64, i64* %resp_fsc_{idx}").unwrap();
                writeln!(&mut self.lambda_ir, "  %resp_psc_{idx} = inttoptr i64 %resp_isc_{idx} to i64*").unwrap();
                writeln!(&mut self.lambda_ir, "  %sc_slot_{idx} = getelementptr i64, i64* %resp_psc_{idx}, i64 0").unwrap();
                writeln!(&mut self.lambda_ir, "  store i64 {sc}, i64* %sc_slot_{idx}").unwrap();
            }

            // ── @Produces: pre-set Content-Type on response headers ─────────────
            if let Some(ref ct) = route.produces {
                let ct_len = ct.len() + 1;
                // Get response.headers (HttpResponse[1])
                writeln!(&mut self.lambda_ir, "  %resp_fprod_{idx} = getelementptr i64, i64* %ctx_ptr, i64 1").unwrap();
                writeln!(&mut self.lambda_ir, "  %resp_iprod_{idx} = load i64, i64* %resp_fprod_{idx}").unwrap();
                writeln!(&mut self.lambda_ir, "  %resp_pprod_{idx} = inttoptr i64 %resp_iprod_{idx} to i64*").unwrap();
                writeln!(&mut self.lambda_ir, "  %hdrs_fprod_{idx} = getelementptr i64, i64* %resp_pprod_{idx}, i64 1").unwrap();
                writeln!(&mut self.lambda_ir, "  %hdrs_iprod_{idx} = load i64, i64* %hdrs_fprod_{idx}").unwrap();
                writeln!(&mut self.lambda_ir, "  %hdrs_prod_{idx} = inttoptr i64 %hdrs_iprod_{idx} to i8*").unwrap();
                writeln!(&mut self.lambda_ir, "  %ct_key_prod_{idx} = getelementptr [13 x i8], [13 x i8]* @__hdr_content_type, i64 0, i64 0").unwrap();
                writeln!(&mut self.lambda_ir, "  %ct_val_prod_{idx} = getelementptr [{ct_len} x i8], [{ct_len} x i8]* @__route_produces_{idx}, i64 0, i64 0").unwrap();
                writeln!(&mut self.lambda_ir, "  %ct_val_i64_{idx} = ptrtoint i8* %ct_val_prod_{idx} to i64").unwrap();
                writeln!(&mut self.lambda_ir, "  call void @tinox_map_set(i8* %hdrs_prod_{idx}, i8* %ct_key_prod_{idx}, i64 %ct_val_i64_{idx})").unwrap();
            }

            // ── Allocate controller and call the handler ─────────────────────────
            if route.is_static {
                // fnc (static): no self pointer, called as method_fn(ctx)
                writeln!(&mut self.lambda_ir, "  call void @{method_fn}(i64* %ctx_ptr)").unwrap();
            } else {
                // fn (instance): use DI getter/factory if the controller is a DI component
                let di_scope = self.di_components.iter()
                    .find(|c| c.class_name == route.class_name)
                    .map(|c| c.scope.clone());
                match di_scope {
                    Some(DiScope::Application) | Some(DiScope::Startup) => {
                        writeln!(&mut self.lambda_ir,
                            "  %ctrl_{idx} = call i64* @{}_di_get()", route.class_name).unwrap();
                        // Re-inject any @HttpRequestScoped fields per-request
                        let inject_fields: Vec<(String, String)> = self.di_components.iter()
                            .find(|c| c.class_name == route.class_name)
                            .map(|c| c.inject_fields.iter()
                                .map(|f| (f.field_name.clone(), f.field_type.clone()))
                                .collect())
                            .unwrap_or_default();
                        for (fi, (fname, ftype)) in inject_fields.iter().enumerate() {
                            let is_request_scoped = self.di_components.iter()
                                .any(|c| c.class_name == *ftype && matches!(c.scope, DiScope::HttpRequest));
                            if is_request_scoped {
                                let foffset = self.struct_layouts.get(route.class_name.as_str())
                                    .and_then(|l| l.iter().position(|f| f == fname))
                                    .unwrap_or(0);
                                writeln!(&mut self.lambda_ir,
                                    "  %req_dep_{idx}_{fi} = call i64* @{ftype}_di_create()").unwrap();
                                writeln!(&mut self.lambda_ir,
                                    "  %req_dep_i64_{idx}_{fi} = ptrtoint i64* %req_dep_{idx}_{fi} to i64").unwrap();
                                writeln!(&mut self.lambda_ir,
                                    "  %req_fptr_{idx}_{fi} = getelementptr i64, i64* %ctrl_{idx}, i64 {foffset}").unwrap();
                                writeln!(&mut self.lambda_ir,
                                    "  store i64 %req_dep_i64_{idx}_{fi}, i64* %req_fptr_{idx}_{fi}").unwrap();
                            }
                        }
                    }
                    Some(DiScope::HttpRequest) => {
                        writeln!(&mut self.lambda_ir,
                            "  %ctrl_{idx} = call i64* @{}_di_create()", route.class_name).unwrap();
                    }
                    None => {
                        writeln!(&mut self.lambda_ir,
                            "  %raw_{idx} = call i8* @tinox_alloc(i64 {ctrl_size})").unwrap();
                        writeln!(&mut self.lambda_ir,
                            "  %ctrl_{idx} = bitcast i8* %raw_{idx} to i64*").unwrap();
                    }
                }
                writeln!(&mut self.lambda_ir, "  call void @{method_fn}(i64* %ctrl_{idx}, i64* %ctx_ptr)").unwrap();
            }
            writeln!(&mut self.lambda_ir, "  ret void").unwrap();
            writeln!(&mut self.lambda_ir, "}}").unwrap();
            writeln!(&mut self.lambda_ir).unwrap();
        }

        // ── __tinox_register_routes ─────────────────────────────────────────────
        writeln!(&mut self.lambda_ir, "define void @__tinox_register_routes(i64* %server) {{").unwrap();
        writeln!(&mut self.lambda_ir, "entry:").unwrap();

        for (idx, route) in routes.iter().enumerate() {
            let shim = format!("__route_{}_{}", route.class_name, route.method_name);
            let server_method = format!("tinox_HttpServer_{}", route.http_method.to_lowercase());
            let path_len = route.path.len() + 1;

            writeln!(&mut self.lambda_ir,
                "  %fn_{idx} = ptrtoint void (i64)* @{shim} to i64").unwrap();
            writeln!(&mut self.lambda_ir,
                "  %path_{idx} = getelementptr [{path_len} x i8], [{path_len} x i8]* @__route_path_{idx}, i64 0, i64 0").unwrap();
            writeln!(&mut self.lambda_ir,
                "  call void @{server_method}(i64* %server, i8* %path_{idx}, i64 %fn_{idx})").unwrap();
        }

        writeln!(&mut self.lambda_ir, "  ret void").unwrap();
        writeln!(&mut self.lambda_ir, "}}").unwrap();
        writeln!(&mut self.lambda_ir).unwrap();

        // ── Auto-generated main (only when no user main exists) ─────────────────
        if !self.has_main {
            let port = std::env::var("TINOX_PORT")
                .ok()
                .and_then(|s| s.parse::<u16>().ok())
                .unwrap_or(8080);
            writeln!(&mut self.lambda_ir, "define i64 @tinox_main() {{").unwrap();
            writeln!(&mut self.lambda_ir, "entry:").unwrap();
            writeln!(&mut self.lambda_ir, "  %server = call i64* @tinox_HttpServer_new(i64 {port})").unwrap();
            writeln!(&mut self.lambda_ir, "  call void @__tinox_register_routes(i64* %server)").unwrap();
            writeln!(&mut self.lambda_ir, "  call void @tinox_HttpServer_listen(i64* %server)").unwrap();
            writeln!(&mut self.lambda_ir, "  ret i64 0").unwrap();
            writeln!(&mut self.lambda_ir, "}}").unwrap();
            writeln!(&mut self.lambda_ir).unwrap();
        }
    }

    fn emit_di_code(&mut self) {
        let components = self.di_components.clone();
        if components.is_empty() {
            return;
        }

        // Global instance pointers for application/startup scoped components
        for comp in &components {
            if matches!(comp.scope, DiScope::Application | DiScope::Startup) {
                writeln!(&mut self.lambda_ir,
                    "@{}_di_instance = global i8* null", comp.class_name).unwrap();
            }
        }
        writeln!(&mut self.lambda_ir).unwrap();

        // Getter / factory for each component
        for comp in &components {
            let name = &comp.class_name;
            let size = self.struct_layouts.get(name.as_str())
                .map(|f| (f.len().max(1) * 8) as i64)
                .unwrap_or(8);

            match comp.scope {
                DiScope::Application | DiScope::Startup => {
                    writeln!(&mut self.lambda_ir, "define i64* @{name}_di_get() {{").unwrap();
                    writeln!(&mut self.lambda_ir, "entry:").unwrap();
                    writeln!(&mut self.lambda_ir, "  %inst_raw = load i8*, i8** @{name}_di_instance").unwrap();
                    writeln!(&mut self.lambda_ir, "  %is_null = icmp eq i8* %inst_raw, null").unwrap();
                    writeln!(&mut self.lambda_ir, "  br i1 %is_null, label %create, label %done").unwrap();
                    writeln!(&mut self.lambda_ir, "create:").unwrap();
                    writeln!(&mut self.lambda_ir, "  %raw = call i8* @tinox_alloc(i64 {size})").unwrap();
                    writeln!(&mut self.lambda_ir, "  %new_inst = bitcast i8* %raw to i64*").unwrap();

                    for (fi, field) in comp.inject_fields.iter().enumerate() {
                        let field_offset = self.struct_layouts.get(name.as_str())
                            .and_then(|layout| layout.iter().position(|f| f == &field.field_name))
                            .unwrap_or(0);
                        let dep = &field.field_type;
                        let dep_is_app = components.iter().any(|c|
                            c.class_name == *dep && matches!(c.scope, DiScope::Application | DiScope::Startup));
                        if dep_is_app {
                            writeln!(&mut self.lambda_ir, "  %dep_{fi} = call i64* @{dep}_di_get()").unwrap();
                        } else {
                            writeln!(&mut self.lambda_ir, "  %dep_{fi} = call i64* @{dep}_di_create()").unwrap();
                        }
                        writeln!(&mut self.lambda_ir, "  %dep_i64_{fi} = ptrtoint i64* %dep_{fi} to i64").unwrap();
                        writeln!(&mut self.lambda_ir, "  %fptr_{fi} = getelementptr i64, i64* %new_inst, i64 {field_offset}").unwrap();
                        writeln!(&mut self.lambda_ir, "  store i64 %dep_i64_{fi}, i64* %fptr_{fi}").unwrap();
                    }

                    writeln!(&mut self.lambda_ir, "  %new_raw = bitcast i64* %new_inst to i8*").unwrap();
                    writeln!(&mut self.lambda_ir, "  store i8* %new_raw, i8** @{name}_di_instance").unwrap();
                    writeln!(&mut self.lambda_ir, "  br label %done").unwrap();
                    writeln!(&mut self.lambda_ir, "done:").unwrap();
                    writeln!(&mut self.lambda_ir, "  %result_raw = load i8*, i8** @{name}_di_instance").unwrap();
                    writeln!(&mut self.lambda_ir, "  %result = bitcast i8* %result_raw to i64*").unwrap();
                    writeln!(&mut self.lambda_ir, "  ret i64* %result").unwrap();
                    writeln!(&mut self.lambda_ir, "}}").unwrap();
                    writeln!(&mut self.lambda_ir).unwrap();
                }
                DiScope::HttpRequest => {
                    writeln!(&mut self.lambda_ir, "define i64* @{name}_di_create() {{").unwrap();
                    writeln!(&mut self.lambda_ir, "entry:").unwrap();
                    writeln!(&mut self.lambda_ir, "  %raw = call i8* @tinox_alloc(i64 {size})").unwrap();
                    writeln!(&mut self.lambda_ir, "  %inst = bitcast i8* %raw to i64*").unwrap();

                    for (fi, field) in comp.inject_fields.iter().enumerate() {
                        let field_offset = self.struct_layouts.get(name.as_str())
                            .and_then(|layout| layout.iter().position(|f| f == &field.field_name))
                            .unwrap_or(0);
                        let dep = &field.field_type;
                        let dep_is_app = components.iter().any(|c|
                            c.class_name == *dep && matches!(c.scope, DiScope::Application | DiScope::Startup));
                        if dep_is_app {
                            writeln!(&mut self.lambda_ir, "  %dep_{fi} = call i64* @{dep}_di_get()").unwrap();
                        } else {
                            writeln!(&mut self.lambda_ir, "  %dep_{fi} = call i64* @{dep}_di_create()").unwrap();
                        }
                        writeln!(&mut self.lambda_ir, "  %dep_i64_{fi} = ptrtoint i64* %dep_{fi} to i64").unwrap();
                        writeln!(&mut self.lambda_ir, "  %fptr_{fi} = getelementptr i64, i64* %inst, i64 {field_offset}").unwrap();
                        writeln!(&mut self.lambda_ir, "  store i64 %dep_i64_{fi}, i64* %fptr_{fi}").unwrap();
                    }

                    writeln!(&mut self.lambda_ir, "  ret i64* %inst").unwrap();
                    writeln!(&mut self.lambda_ir, "}}").unwrap();
                    writeln!(&mut self.lambda_ir).unwrap();
                }
            }
        }

        // tinox_di_startup(): eagerly initialize all @Startup components
        let has_startup = components.iter().any(|c| matches!(c.scope, DiScope::Startup));
        if has_startup {
            writeln!(&mut self.lambda_ir, "define void @tinox_di_startup() {{").unwrap();
            writeln!(&mut self.lambda_ir, "entry:").unwrap();
            for comp in components.iter().filter(|c| matches!(c.scope, DiScope::Startup)) {
                writeln!(&mut self.lambda_ir, "  call i64* @{}_di_get()", comp.class_name).unwrap();
            }
            writeln!(&mut self.lambda_ir, "  ret void").unwrap();
            writeln!(&mut self.lambda_ir, "}}").unwrap();
            writeln!(&mut self.lambda_ir).unwrap();

            // Register tinox_di_startup as a global constructor so it runs before main
            writeln!(&mut self.lambda_ir,
                "@llvm.global_ctors = appending global [1 x {{ i32, void ()*, i8* }}] \
                [{{ i32, void ()*, i8* }} {{ i32 65535, void ()* @tinox_di_startup, i8* null }}]").unwrap();
            writeln!(&mut self.lambda_ir).unwrap();
        }
    }

    fn escape_llvm_string(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            match c {
                '"'  => out.push_str("\\22"),
                '\\'  => out.push_str("\\5C"),
                '\n' => out.push_str("\\0A"),
                '\r' => out.push_str("\\0D"),
                '\t' => out.push_str("\\09"),
                c if (c as u32) < 0x20 => {
                    out.push_str(&format!("\\{:02X}", c as u32));
                }
                c => out.push(c),
            }
        }
        out
    }

    fn capitalize_first(s: &str) -> String {
        let mut chars = s.chars();
        match chars.next() {
            None => String::new(),
            Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        }
    }

    pub fn into_ir(self) -> String {
        let mut result = self.ir;
        result.push_str(&self.lambda_ir);
        result
    }

    fn gen_fn(&mut self, f: &tinox_parser::Function) -> Result<(), ErrorBag> {
        // extern fn — no body, emit a declare instead of a define
        if matches!(f.body.node, tinox_parser::StmtKind::Empty) {
            let ret_type = self.type_to_llvm_inst(&f.ret_type);
            let params_str = f.params.iter()
                .map(|p| self.type_to_llvm_inst(&p.param_type))
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(&mut self.ir, "declare {} @{}({})", ret_type, f.name, params_str).unwrap();
            return Ok(());
        }
        let ret_type = self.type_to_llvm_inst(&f.ret_type);
        let mut params_str = String::new();
        let mut ctx = GenCtx {
            locals: HashMap::new(),
            local_slots: HashMap::new(),
            range_vars: HashSet::new(),
            params: HashSet::new(),
            struct_fields: Vec::new(),
            current_struct: None,
            local_types: HashMap::new(),
            break_target: None,
            continue_target: None,
            error_catch: None,
            defer_stack: Vec::new(),
            in_defer_exec: false,
            ret_type: ret_type.clone(),
        };

        for (i, p) in f.params.iter().enumerate() {
            if i > 0 {
                params_str.push_str(", ");
            }
            let llvm_ty = self.type_to_llvm_inst(&p.param_type);
            params_str.push_str(&format!("{} %{}", llvm_ty, p.name));
            ctx.locals.insert(p.name.clone(), (llvm_ty.clone(), i));
            ctx.params.insert(p.name.clone());

            // Track parameter types for struct/class types and arrays
            if let Type::Named(class_name) = &p.param_type {
                ctx.local_types.insert(p.name.clone(), class_name.clone());
            } else if matches!(&p.param_type, Type::Array(inner) if matches!(inner.as_ref(), Type::String))
                || matches!(&p.param_type, Type::Generic { name, .. } if name == "Array") {
                ctx.local_types.insert(p.name.clone(), "Array:String".to_string());
            }
        }

        let fn_name = if f.name == "main" {
            self.has_main = true;
            "tinox_main".to_string()
        } else {
            f.name.clone()
        };

        let is_inline = f.annotations.iter().any(|a| a.name == "inline")
            || self.inline_functions.contains(&fn_name);
        let linkage = if is_inline {
            "define alwaysinline "
        } else {
            "define "
        };

        writeln!(
            &mut self.ir,
            "{}{} @{}({}) {{",
            linkage, ret_type, fn_name, params_str
        )
        .unwrap();
        writeln!(&mut self.ir, "entry:").unwrap();

        self.gen_stmt_body(&f.body, &mut ctx)?;

        let has_terminator = self.ir.lines().last().map_or(false, |l| {
            let t = l.trim();
            t.starts_with("ret ") || t.starts_with("br ")
        });
        if !has_terminator {
            if ret_type == "void" {
                writeln!(&mut self.ir, "ret void").unwrap();
            } else if ret_type.ends_with('*') {
                writeln!(&mut self.ir, "ret {} null", ret_type).unwrap();
            } else {
                writeln!(&mut self.ir, "ret {} 0", ret_type).unwrap();
            }
        }

        writeln!(&mut self.ir, "}}").unwrap();
        writeln!(&mut self.ir).unwrap();

        Ok(())
    }

    fn gen_class_method(
        &mut self,
        class_name: &str,
        method: &Method,
    ) -> Result<(), ErrorBag> {
        let ret_type = self.type_to_llvm_inst(&method.ret_type);
        let fn_name = format!("{}_{}", class_name, method.name);
        self.method_ret_types.insert(fn_name.clone(), ret_type.clone());

        let mut ctx = GenCtx {
            locals: HashMap::new(),
            local_slots: HashMap::new(),
            range_vars: HashSet::new(),
            params: HashSet::new(),
            struct_fields: Vec::new(),
            current_struct: Some(class_name.to_string()),
            local_types: HashMap::new(),
            break_target: None,
            continue_target: None,
            error_catch: None,
            defer_stack: Vec::new(),
            in_defer_exec: false,
            ret_type: ret_type.clone(),
        };

        let mut params_str = if method.static_ {
            String::new()
        } else {
            "i64* %self".to_string()
        };
        if !method.static_ {
            ctx.locals.insert("self".to_string(), ("i64*".to_string(), 0));
            ctx.params.insert("self".to_string());
            ctx.local_types.insert("self".to_string(), class_name.to_string());
        }

        for p in &method.params {
            let llvm_ty = self.type_to_llvm_inst(&p.param_type);
            if !params_str.is_empty() {
                params_str.push_str(", ");
            }
            params_str.push_str(&format!("{} %{}", llvm_ty, p.name));
            ctx.locals
                .insert(p.name.clone(), (llvm_ty.clone(), ctx.locals.len()));
            ctx.params.insert(p.name.clone());
            if let Type::Named(cn) = &p.param_type {
                ctx.local_types.insert(p.name.clone(), cn.clone());
            } else if matches!(&p.param_type, Type::Array(inner) if matches!(inner.as_ref(), Type::String))
                || matches!(&p.param_type, Type::Generic { name, .. } if name == "Array") {
                ctx.local_types.insert(p.name.clone(), "Array:String".to_string());
            }
        }

        let is_inline = method.annotations.iter().any(|a| a.name == "inline")
            || self.inline_methods.contains(&(class_name.to_string(), method.name.clone()));
        let linkage = if is_inline {
            "define alwaysinline "
        } else {
            "define "
        };

        writeln!(
            &mut self.ir,
            "{}{} @{}({}) {{",
            linkage, ret_type, fn_name, params_str
        )
        .unwrap();
        writeln!(&mut self.ir, "entry:").unwrap();

        self.gen_stmt_body(&method.body, &mut ctx)?;

        let has_terminator = self.ir.lines().last().map_or(false, |l| {
            let t = l.trim();
            t.starts_with("ret ") || t.starts_with("br ")
        });
        if !has_terminator {
            if ret_type == "void" {
                writeln!(&mut self.ir, "ret void").unwrap();
            } else if ret_type.ends_with('*') {
                writeln!(&mut self.ir, "ret {} null", ret_type).unwrap();
            } else {
                writeln!(&mut self.ir, "ret {} 0", ret_type).unwrap();
            }
        }

        writeln!(&mut self.ir, "}}").unwrap();
        writeln!(&mut self.ir).unwrap();

        Ok(())
    }

    /// Emit the auto-generated `ClassName_new(field1, field2, ...) -> i64*` function
    /// for an `immutable` declaration.
    fn emit_immutable_new(&mut self, u: &tinox_parser::ImmutableDecl) {
        let class_name = &u.name;
        let n_fields = u.fields.len();
        let size = n_fields * 8;

        let params_str: Vec<String> = u.fields.iter()
            .map(|f| format!("{} %{}", Self::type_to_llvm(&f.param_type), f.name))
            .collect();

        writeln!(&mut self.ir, "define i64* @{class_name}_new({}) {{", params_str.join(", ")).unwrap();
        writeln!(&mut self.ir, "entry:").unwrap();
        writeln!(&mut self.ir, "  %raw = call i8* @tinox_alloc(i64 {size})").unwrap();
        writeln!(&mut self.ir, "  %ptr = bitcast i8* %raw to i64*").unwrap();

        for (i, field) in u.fields.iter().enumerate() {
            let llvm_ty = Self::type_to_llvm(&field.param_type);
            let store_val = if llvm_ty == "i8*" {
                writeln!(&mut self.ir, "  %fconv_{i} = ptrtoint i8* %{} to i64", field.name).unwrap();
                format!("%fconv_{i}")
            } else if llvm_ty == "i64*" {
                writeln!(&mut self.ir, "  %fconv_{i} = ptrtoint i64* %{} to i64", field.name).unwrap();
                format!("%fconv_{i}")
            } else if llvm_ty == "i1" {
                writeln!(&mut self.ir, "  %fconv_{i} = zext i1 %{} to i64", field.name).unwrap();
                format!("%fconv_{i}")
            } else if llvm_ty == "double" {
                writeln!(&mut self.ir, "  %fconv_{i} = bitcast double %{} to i64", field.name).unwrap();
                format!("%fconv_{i}")
            } else if llvm_ty == "float" {
                writeln!(&mut self.ir, "  %fconv_ext_{i} = fpext float %{} to double", field.name).unwrap();
                writeln!(&mut self.ir, "  %fconv_{i} = bitcast double %fconv_ext_{i} to i64").unwrap();
                format!("%fconv_{i}")
            } else if llvm_ty != "i64" {
                writeln!(&mut self.ir, "  %fconv_{i} = sext {llvm_ty} %{} to i64", field.name).unwrap();
                format!("%fconv_{i}")
            } else {
                format!("%{}", field.name)
            };
            writeln!(&mut self.ir, "  %gep_{i} = getelementptr i64, i64* %ptr, i64 {i}").unwrap();
            writeln!(&mut self.ir, "  store i64 {store_val}, i64* %gep_{i}").unwrap();
        }

        writeln!(&mut self.ir, "  ret i64* %ptr").unwrap();
        writeln!(&mut self.ir, "}}").unwrap();
        writeln!(&mut self.ir).unwrap();
    }

    /// Emit `tinox_main` for classes annotated with `@Command`.
    /// Generates: help function, --help/--version handling, arg parsing, and `run()` call.
    fn emit_cli_code(&mut self) {
        if self.cli_commands.is_empty() || self.has_main {
            return;
        }

        let commands = self.cli_commands.clone();

        // Only the first @Command class acts as the entry point.
        let cmd = &commands[0];
        let class = cmd.class_name.clone();

        // ── String constants ────────────────────────────────────────────────
        let mut str_defs = String::new();

        let emit_str = |buf: &mut String, label: &str, text: &str| {
            let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
            let len = text.len() + 1;
            writeln!(buf,
                "@{label} = private constant [{len} x i8] c\"{escaped}\\00\""
            ).unwrap();
        };

        emit_str(&mut str_defs, "__cli_help_long",    "--help");
        emit_str(&mut str_defs, "__cli_help_short",   "-h");
        emit_str(&mut str_defs, "__cli_ver_long",     "--version");
        emit_str(&mut str_defs, "__cli_empty",        "");
        emit_str(&mut str_defs, "__cli_cmd_name",     &cmd.cmd_name);
        emit_str(&mut str_defs, "__cli_cmd_desc",     &cmd.description);
        emit_str(&mut str_defs, "__cli_cmd_ver",      cmd.version.as_deref().unwrap_or(""));

        let mut help_lines: Vec<(String, String)> = Vec::new();

        for (i, opt) in cmd.options.iter().enumerate() {
            let long_name  = opt.names.iter().find(|n| n.starts_with("--")).cloned().unwrap_or_default();
            let short_name = opt.names.iter().find(|n| n.starts_with('-') && !n.starts_with("--")).cloned().unwrap_or_default();
            emit_str(&mut str_defs, &format!("__cli_opt{i}_long"),  &long_name);
            emit_str(&mut str_defs, &format!("__cli_opt{i}_short"), &short_name);
            emit_str(&mut str_defs, &format!("__cli_opt{i}_desc"),  &opt.description);
            let display = if short_name.is_empty() { long_name.clone() }
                          else { format!("{short_name}, {long_name}") };
            help_lines.push((display, opt.description.clone()));
        }
        for (i, arg) in cmd.arguments.iter().enumerate() {
            let placeholder = format!("<{}>", arg.field_name);
            emit_str(&mut str_defs, &format!("__cli_arg{i}_desc"), &arg.description);
            help_lines.push((placeholder, arg.description.clone()));
        }
        for (i, (names, desc)) in help_lines.iter().enumerate() {
            emit_str(&mut str_defs, &format!("__cli_help_entry{i}_names"), names);
            emit_str(&mut str_defs, &format!("__cli_help_entry{i}_desc"),  desc);
        }
        // --help / --version strings for help output
        emit_str(&mut str_defs, "__cli_usage_prefix", "Usage: ");
        emit_str(&mut str_defs, "__cli_usage_suffix", " [options]\n");
        emit_str(&mut str_defs, "__cli_nl",           "\n");
        emit_str(&mut str_defs, "__cli_ver_prefix",   "Version: ");
        emit_str(&mut str_defs, "__cli_help_hdr",     "Options:");

        self.ir.push_str(&str_defs);
        writeln!(&mut self.ir).unwrap();

        // ── Helper: getelementptr shorthand ─────────────────────────────────
        let mut body = String::new();

        // Helper macro (Rust closure) to get i8* from a named string constant
        let gep = |b: &mut String, tmp: &str, label: &str, len: usize| {
            writeln!(b,
                "  {tmp} = getelementptr [{len} x i8], [{len} x i8]* @{label}, i64 0, i64 0"
            ).unwrap();
        };

        // ── __tinox_cli_help ─────────────────────────────────────────────────
        writeln!(&mut body, "define void @__tinox_cli_help() {{").unwrap();

        if !cmd.description.is_empty() {
            let len = cmd.description.len() + 1;
            gep(&mut body, "%desc_ptr", "__cli_cmd_desc", len);
            writeln!(&mut body, "  call void @tinox_print_string(i8* %desc_ptr)").unwrap();
            writeln!(&mut body, "  call void @tinox_print_newline()").unwrap();
        }

        let usage_prefix_len = "Usage: ".len() + 1;
        let usage_suffix_len = " [options]\n".len() + 1;
        let cmd_name_len = cmd.cmd_name.len() + 1;
        gep(&mut body, "%usage_pfx", "__cli_usage_prefix", usage_prefix_len);
        gep(&mut body, "%cmd_name_ptr", "__cli_cmd_name", cmd_name_len);
        gep(&mut body, "%usage_sfx", "__cli_usage_suffix", usage_suffix_len);
        writeln!(&mut body, "  call void @tinox_print_string(i8* %usage_pfx)").unwrap();
        writeln!(&mut body, "  call void @tinox_print_string(i8* %cmd_name_ptr)").unwrap();
        writeln!(&mut body, "  call void @tinox_print_string(i8* %usage_sfx)").unwrap();

        if !help_lines.is_empty() {
            let hdr_len = "Options:".len() + 1;
            let nl_len = "\n".len() + 1;
            gep(&mut body, "%hdr_ptr", "__cli_help_hdr", hdr_len);
            gep(&mut body, "%nl_ptr", "__cli_nl", nl_len);
            writeln!(&mut body, "  call void @tinox_print_string(i8* %hdr_ptr)").unwrap();
            writeln!(&mut body, "  call void @tinox_print_newline()").unwrap();
            for (i, (names, desc)) in help_lines.iter().enumerate() {
                let nlen = names.len() + 1;
                let dlen = desc.len() + 1;
                gep(&mut body, &format!("%hn{i}"), &format!("__cli_help_entry{i}_names"), nlen);
                gep(&mut body, &format!("%hd{i}"), &format!("__cli_help_entry{i}_desc"),  dlen);
                writeln!(&mut body,
                    "  call void @tinox_cli_print_option(i8* %hn{i}, i8* %hd{i})"
                ).unwrap();
            }
        }

        if let Some(ref ver) = cmd.version {
            let vp_len = "Version: ".len() + 1;
            let ver_len = ver.len() + 1;
            gep(&mut body, "%vp_ptr", "__cli_ver_prefix", vp_len);
            gep(&mut body, "%ver_ptr", "__cli_cmd_ver", ver_len);
            writeln!(&mut body, "  call void @tinox_print_string(i8* %vp_ptr)").unwrap();
            writeln!(&mut body, "  call void @tinox_print_string(i8* %ver_ptr)").unwrap();
            writeln!(&mut body, "  call void @tinox_print_newline()").unwrap();
        }

        writeln!(&mut body, "  ret void").unwrap();
        writeln!(&mut body, "}}").unwrap();
        writeln!(&mut body).unwrap();

        // ── tinox_main ───────────────────────────────────────────────────────
        writeln!(&mut body, "define i64 @tinox_main() {{").unwrap();
        writeln!(&mut body, "entry:").unwrap();

        // Check --help / -h
        gep(&mut body, "%help_long",  "__cli_help_long",  7);
        gep(&mut body, "%help_short", "__cli_help_short", 3);
        writeln!(&mut body,
            "  %has_help = call i64 @tinox_cli_has_flag(i8* %help_long, i8* %help_short)"
        ).unwrap();
        writeln!(&mut body, "  %help_cond = icmp ne i64 %has_help, 0").unwrap();
        writeln!(&mut body, "  br i1 %help_cond, label %show_help, label %check_version").unwrap();
        writeln!(&mut body, "show_help:").unwrap();
        writeln!(&mut body, "  call void @__tinox_cli_help()").unwrap();
        writeln!(&mut body, "  ret i64 0").unwrap();

        // Check --version
        writeln!(&mut body, "check_version:").unwrap();
        gep(&mut body, "%ver_long", "__cli_ver_long", 10);
        gep(&mut body, "%empty_str", "__cli_empty", 1);
        writeln!(&mut body,
            "  %has_ver = call i64 @tinox_cli_has_flag(i8* %ver_long, i8* %empty_str)"
        ).unwrap();
        writeln!(&mut body, "  %ver_cond = icmp ne i64 %has_ver, 0").unwrap();
        writeln!(&mut body, "  br i1 %ver_cond, label %show_version, label %parse_args").unwrap();
        writeln!(&mut body, "show_version:").unwrap();
        let ver_str = cmd.version.as_deref().unwrap_or("");
        let ver_len = ver_str.len() + 1;
        gep(&mut body, "%ver_val", "__cli_cmd_ver", ver_len);
        writeln!(&mut body, "  call void @tinox_print_string(i8* %ver_val)").unwrap();
        writeln!(&mut body, "  call void @tinox_print_newline()").unwrap();
        writeln!(&mut body, "  ret i64 0").unwrap();

        let layout = self.struct_layouts.get(&class).cloned().unwrap_or_default();

        // Create command instance — allocate and zero-initialise (no new() needed)
        writeln!(&mut body, "parse_args:").unwrap();
        let n_fields = layout.len();
        let byte_size = (n_fields * 8).max(8);
        writeln!(&mut body, "  %cmd_raw = call i8* @tinox_alloc(i64 {byte_size})").unwrap();
        writeln!(&mut body, "  %cmd_obj = bitcast i8* %cmd_raw to i64*").unwrap();
        for fi in 0..n_fields {
            writeln!(&mut body, "  %zinit_{fi} = getelementptr i64, i64* %cmd_obj, i64 {fi}").unwrap();
            writeln!(&mut body, "  store i64 0, i64* %zinit_{fi}").unwrap();
        }

        // Parse options
        for (i, opt) in cmd.options.iter().enumerate() {
            let long_name  = opt.names.iter().find(|n| n.starts_with("--")).cloned().unwrap_or_default();
            let short_name = opt.names.iter().find(|n| n.starts_with('-') && !n.starts_with("--")).cloned().unwrap_or_default();
            let long_len   = long_name.len() + 1;
            let short_len  = short_name.len() + 1;
            let field_idx  = layout.iter().position(|f| f == &opt.field_name).unwrap_or(usize::MAX);
            if field_idx == usize::MAX { continue; }

            gep(&mut body, &format!("%olong{i}"),  &format!("__cli_opt{i}_long"),  long_len);
            gep(&mut body, &format!("%oshort{i}"), &format!("__cli_opt{i}_short"), short_len);

            match opt.field_type.as_str() {
                "Bool" => {
                    writeln!(&mut body,
                        "  %opt_flag{i} = call i64 @tinox_cli_has_flag(i8* %olong{i}, i8* %oshort{i})"
                    ).unwrap();
                    writeln!(&mut body,
                        "  %opt_fp{i} = getelementptr i64, i64* %cmd_obj, i64 {field_idx}"
                    ).unwrap();
                    writeln!(&mut body,
                        "  store i64 %opt_flag{i}, i64* %opt_fp{i}"
                    ).unwrap();
                }
                "Int" => {
                    writeln!(&mut body,
                        "  %opt_int{i} = call i64 @tinox_cli_get_int(i8* %olong{i}, i8* %oshort{i}, i64 0)"
                    ).unwrap();
                    writeln!(&mut body,
                        "  %opt_fp{i} = getelementptr i64, i64* %cmd_obj, i64 {field_idx}"
                    ).unwrap();
                    writeln!(&mut body,
                        "  store i64 %opt_int{i}, i64* %opt_fp{i}"
                    ).unwrap();
                }
                _ => {
                    // String
                    writeln!(&mut body,
                        "  %opt_str{i} = call i8* @tinox_cli_get_string(i8* %olong{i}, i8* %oshort{i})"
                    ).unwrap();
                    writeln!(&mut body,
                        "  %opt_null{i} = icmp eq i8* %opt_str{i}, null"
                    ).unwrap();
                    writeln!(&mut body,
                        "  br i1 %opt_null{i}, label %skip_opt{i}, label %set_opt{i}"
                    ).unwrap();
                    writeln!(&mut body, "set_opt{i}:").unwrap();
                    writeln!(&mut body,
                        "  %opt_i64_{i} = ptrtoint i8* %opt_str{i} to i64"
                    ).unwrap();
                    writeln!(&mut body,
                        "  %opt_fp{i} = getelementptr i64, i64* %cmd_obj, i64 {field_idx}"
                    ).unwrap();
                    writeln!(&mut body,
                        "  store i64 %opt_i64_{i}, i64* %opt_fp{i}"
                    ).unwrap();
                    writeln!(&mut body, "  br label %skip_opt{i}").unwrap();
                    writeln!(&mut body, "skip_opt{i}:").unwrap();
                }
            }
        }

        // Parse positional arguments
        for (i, arg) in cmd.arguments.iter().enumerate() {
            let field_idx = layout.iter().position(|f| f == &arg.field_name).unwrap_or(usize::MAX);
            if field_idx == usize::MAX { continue; }

            writeln!(&mut body,
                "  %pos_str{i} = call i8* @tinox_cli_get_positional(i32 {})", arg.index
            ).unwrap();

            match arg.field_type.as_str() {
                "Int" => {
                    writeln!(&mut body, "  %pos_null{i} = icmp eq i8* %pos_str{i}, null").unwrap();
                    writeln!(&mut body, "  br i1 %pos_null{i}, label %skip_pos{i}, label %set_pos{i}").unwrap();
                    writeln!(&mut body, "set_pos{i}:").unwrap();
                    writeln!(&mut body, "  %pos_int{i} = call i64 @tinox_string_to_int(i8* %pos_str{i})").unwrap();
                    writeln!(&mut body, "  %pos_fp{i} = getelementptr i64, i64* %cmd_obj, i64 {field_idx}").unwrap();
                    writeln!(&mut body, "  store i64 %pos_int{i}, i64* %pos_fp{i}").unwrap();
                    writeln!(&mut body, "  br label %skip_pos{i}").unwrap();
                    writeln!(&mut body, "skip_pos{i}:").unwrap();
                }
                _ => {
                    // String (or Bool treated as string — uncommon but safe)
                    writeln!(&mut body, "  %pos_null{i} = icmp eq i8* %pos_str{i}, null").unwrap();
                    writeln!(&mut body, "  br i1 %pos_null{i}, label %skip_pos{i}, label %set_pos{i}").unwrap();
                    writeln!(&mut body, "set_pos{i}:").unwrap();
                    writeln!(&mut body, "  %pos_i64_{i} = ptrtoint i8* %pos_str{i} to i64").unwrap();
                    writeln!(&mut body, "  %pos_fp{i} = getelementptr i64, i64* %cmd_obj, i64 {field_idx}").unwrap();
                    writeln!(&mut body, "  store i64 %pos_i64_{i}, i64* %pos_fp{i}").unwrap();
                    writeln!(&mut body, "  br label %skip_pos{i}").unwrap();
                    writeln!(&mut body, "skip_pos{i}:").unwrap();
                }
            }
        }

        // Call run()
        writeln!(&mut body, "  %cli_ret = call i64 @{class}_run(i64* %cmd_obj)").unwrap();
        writeln!(&mut body, "  ret i64 %cli_ret").unwrap();
        writeln!(&mut body, "}}").unwrap();
        writeln!(&mut body).unwrap();

        self.lambda_ir.push_str(&body);
        self.has_main = true;
    }

    /// Emit `tinox_main` for a single test method: allocate object, call method,
    /// exit 0 on true/non-zero return, 1 on false/0.
    fn emit_test_code(&mut self) {
        let (class, method) = match self.test_entry.clone() {
            Some(e) if !self.has_main => e,
            _ => return,
        };

        let layout = self.struct_layouts.get(&class).cloned().unwrap_or_default();
        let n_fields = layout.len();
        let byte_size = (n_fields * 8).max(8);

        let mut b = String::new();
        writeln!(&mut b, "define i64 @tinox_main() {{").unwrap();
        writeln!(&mut b, "  %raw = call i8* @tinox_alloc(i64 {byte_size})").unwrap();
        writeln!(&mut b, "  %obj = bitcast i8* %raw to i64*").unwrap();
        for fi in 0..n_fields {
            writeln!(&mut b, "  %zi{fi} = getelementptr i64, i64* %obj, i64 {fi}").unwrap();
            writeln!(&mut b, "  store i64 0, i64* %zi{fi}").unwrap();
        }
        writeln!(&mut b, "  %result = call i64 @{class}_{method}(i64* %obj)").unwrap();
        // result != 0 → pass (exit 0), result == 0 → fail (exit 1)
        writeln!(&mut b, "  %pass = icmp ne i64 %result, 0").unwrap();
        writeln!(&mut b, "  %code = select i1 %pass, i64 0, i64 1").unwrap();
        writeln!(&mut b, "  ret i64 %code").unwrap();
        writeln!(&mut b, "}}").unwrap();
        writeln!(&mut b).unwrap();

        self.lambda_ir.push_str(&b);
        self.has_main = true;
    }

    /// Emit a vtable global for each class that implements at least one interface.
    fn emit_vtable_globals(&mut self, source: &SourceFile) {
        let class_names: Vec<(String, Vec<String>)> = source
            .decls
            .iter()
            .flat_map(|d| {
                let mut v = Vec::new();
                match &d.node {
                    DeclKind::Class(c) if !c.implements.is_empty() => {
                        v.push((c.name.clone(), c.implements.clone()));
                    }
                    DeclKind::Namespace(ns) => {
                        for inner in &ns.decls {
                            if let DeclKind::Class(c) = &inner.node {
                                if !c.implements.is_empty() {
                                    v.push((c.name.clone(), c.implements.clone()));
                                }
                            }
                        }
                    }
                    _ => {}
                }
                v
            })
            .collect();

        for (class_name, implements) in class_names {
            let mut vtable_methods: Vec<String> = Vec::new();
            let mut seen: HashSet<String> = HashSet::new();
            for iface in &implements {
                if let Some(methods) = self.vtable_layouts.get(iface) {
                    for m in methods {
                        if seen.insert(m.clone()) {
                            vtable_methods.push(m.clone());
                        }
                    }
                }
            }

            if vtable_methods.is_empty() {
                continue;
            }

            let n = vtable_methods.len();
            let mut entries = String::new();
            for (i, method_name) in vtable_methods.iter().enumerate() {
                if i > 0 {
                    entries.push_str(", ");
                }
                let full_fn = format!("{}_{}", class_name, method_name);
                entries.push_str(&format!(
                    "i64 ptrtoint (i64* (i64*)* @{} to i64)",
                    full_fn
                ));
            }
            writeln!(
                &mut self.ir,
                "@{}_vtable = global [{} x i64] [{}]",
                class_name, n, entries
            )
            .unwrap();
        }
    }

    fn gen_stmt_body(&mut self, stmt: &Stmt, ctx: &mut GenCtx) -> Result<(), ErrorBag> {
        match &stmt.node {
            StmtKind::Defer(inner) => {
                if let Some(scope) = ctx.defer_stack.last_mut() {
                    scope.push((**inner).clone());
                }
                return Ok(());
            }
            StmtKind::Block(stmts) => {
                if !ctx.in_defer_exec {
                    ctx.defer_stack.push(Vec::new());
                }
                for s in stmts {
                    self.gen_stmt_body(s, ctx)?;
                }
                if !ctx.in_defer_exec {
                    self.gen_defer_scope(ctx)?;
                    ctx.defer_stack.pop();
                }
                return Ok(());
            }
            StmtKind::Return(Some(expr)) => {
                let stmts_to_run: Vec<_> = ctx
                    .defer_stack
                    .last()
                    .map(|s| s.clone())
                    .unwrap_or_default();
                for stmt in stmts_to_run.into_iter().rev() {
                    self.gen_stmt_body(&Box::new(stmt), ctx)?;
                }
                if let Some(scope) = ctx.defer_stack.last_mut() {
                    scope.clear();
                }
                let (val, ty) = self.gen_expr(expr, ctx)?;
                let expected = &ctx.ret_type.clone();
                let (final_val, final_ty) = if !expected.is_empty() && &ty != expected {
                    let cast_op = match (ty.as_str(), expected.as_str()) {
                        (from, to) if from.ends_with('*') && to.ends_with('*') => "bitcast",
                        (from, to) if from.starts_with('i') && to.starts_with('i') && !from.contains('*') && !to.contains('*') => {
                            let from_bits: u32 = from[1..].parse().unwrap_or(64);
                            let to_bits: u32 = to[1..].parse().unwrap_or(64);
                            if from_bits > to_bits { "trunc" } else { "zext" }
                        }
                        _ => "",
                    };
                    if !cast_op.is_empty() {
                        let tmp = self.temp();
                        writeln!(&mut self.ir, "{} = {} {} {} to {}", tmp, cast_op, ty, val, expected).unwrap();
                        (tmp, expected.clone())
                    } else {
                        (val, ty)
                    }
                } else {
                    (val, ty)
                };
                writeln!(&mut self.ir, "ret {} {}", final_ty, final_val).unwrap();
            }
            StmtKind::Return(None) => {
                self.gen_defer_scope(ctx)?;
                writeln!(&mut self.ir, "ret void").unwrap();
            }
            StmtKind::Expr(expr) => {
                self.gen_expr(expr, ctx)?;
            }
            StmtKind::Let {
                name, ty, value, ..
            } => {
                let mut llvm_ty = Self::type_to_llvm(ty.as_ref().unwrap_or(&Type::Int64));
                let mut struct_name: Option<String> = None;
                let is_heap_ptr = if let Some(v) = value {
                    if let ExprKind::StructLiteral { name: n, .. } = &v.node {
                        llvm_ty = "i64*".to_string();
                        struct_name = Some(n.clone());
                        true
                    } else if let ExprKind::New { class, type_args, .. } = &v.node {
                        llvm_ty = "i64*".to_string();
                        struct_name = Some(self.effective_class_name(class, type_args));
                        true
                    } else if let ExprKind::MapLiteral(_) = &v.node {
                        llvm_ty = "i8*".to_string();
                        struct_name = Some("Map".to_string());
                        true
                    } else if let ExprKind::Call { func, .. } = &v.node {
                        if matches!(&func.node, ExprKind::Ident(n) if n == "open") {
                            llvm_ty = "i8*".to_string();
                            struct_name = Some("File".to_string());
                            true
                        } else if matches!(&func.node, ExprKind::Ident(n) if n == "split") {
                            llvm_ty = "i64*".to_string();
                            struct_name = Some("Array:String".to_string());
                            true
                        } else { false }
                    } else if let ExprKind::MethodCall { method, .. } = &v.node {
                        if method == "split" {
                            llvm_ty = "i64*".to_string();
                            struct_name = Some("Array:String".to_string());
                            true
                        } else { false }
                    } else if let ExprKind::ArrayLiteral(elems) = &v.node {
                        llvm_ty = "i64*".to_string();
                        let is_str_ann = matches!(ty, Some(Type::Array(inner)) if matches!(inner.as_ref(), Type::String))
                            || matches!(ty, Some(Type::Generic { name, args }) if name == "Array" && args.first().map(|a| matches!(a, Type::String)).unwrap_or(false));
                        let is_str_lit = elems.first().map(|e| matches!(&e.node, ExprKind::Literal(Literal::String(_)))).unwrap_or(false);
                        if is_str_ann || is_str_lit {
                            struct_name = Some("Array:String".to_string());
                            llvm_ty = "i64*".to_string();
                        }
                        true
                    } else if matches!(&v.node, ExprKind::Tuple(_)) {
                        llvm_ty = "i64*".to_string();
                        true
                    } else if matches!(&v.node, ExprKind::Lambda { .. }) {
                        llvm_ty = "i64*".to_string();
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };

                if struct_name.is_none() {
                    if let Some(Type::Named(ann)) = ty {
                        struct_name = Some(ann.clone());
                        if self.defined_classes.contains(ann.as_str()) {
                            llvm_ty = "i64*".to_string();
                        }
                    } else if let Some(Type::Map(_, _)) = ty {
                        struct_name = Some("Map".to_string());
                        llvm_ty = "i8*".to_string();
                    } else if matches!(ty, Some(Type::Array(inner)) if matches!(inner.as_ref(), Type::String))
                        || matches!(ty, Some(Type::Generic { name, .. }) if name == "Array") {
                        struct_name = Some("Array:String".to_string());
                        llvm_ty = "i64*".to_string();
                    }
                }

                if let Some(val) = value {
                    let (v, val_ty) = self.gen_expr(val, ctx)?;
                    let actual_ty = if matches!(&val.node, ExprKind::Lambda { .. }) {
                        val_ty.clone()
                    } else if is_heap_ptr {
                        llvm_ty.clone()
                    } else if ty.is_none() || matches!(ty, Some(Type::Infer)) {
                        // No annotation: use the value's actual type (enables correct float/generic inference)
                        val_ty.clone()
                    } else {
                        llvm_ty.clone()
                    };
                    let slot = ctx.locals.len();
                    ctx.locals.insert(name.clone(), (actual_ty.clone(), slot));
                    if matches!(&val.node, ExprKind::Range { .. }) {
                        ctx.range_vars.insert(name.clone());
                    }
                    // If the declared type annotation is an interface, record the
                    // interface name so vtable dispatch is used for method calls.
                    let effective_type = if let Some(Type::Named(ann)) = ty {
                        if self.known_interfaces.contains(ann.as_str()) {
                            Some(ann.clone())
                        } else {
                            struct_name.clone()
                        }
                    } else {
                        struct_name.clone()
                    };
                    if let Some(sn) = effective_type {
                        ctx.local_types.insert(name.clone(), sn);
                    }
                    if is_heap_ptr {
                        writeln!(&mut self.ir, "%{} = alloca {}", name, actual_ty).unwrap();
                        writeln!(
                            &mut self.ir,
                            "store {} {}, {}* %{}",
                            val_ty, v, actual_ty, name
                        )
                        .unwrap();
                    } else {
                        writeln!(&mut self.ir, "%{} = alloca {}", name, llvm_ty).unwrap();
                        writeln!(
                            &mut self.ir,
                            "store {} {}, {}* %{}",
                            val_ty, v, llvm_ty, name
                        )
                        .unwrap();
                    }
                } else {
                    let slot = ctx.locals.len();
                    ctx.locals.insert(name.clone(), (llvm_ty.clone(), slot));
                    if let Some(sn) = &struct_name {
                        ctx.local_types.insert(name.clone(), sn.clone());
                    }
                    writeln!(&mut self.ir, "%{} = alloca {}", name, llvm_ty).unwrap();
                }
            }
            StmtKind::Var {
                name, ty, value, ..
            } => {
                let mut llvm_ty = Self::type_to_llvm(ty.as_ref().unwrap_or(&Type::Int64));
                let mut struct_name: Option<String> = None;
                let is_ptr = if let Some(v) = value {
                    if let ExprKind::StructLiteral { name: n, .. } = &v.node {
                        llvm_ty = "i64*".to_string();
                        struct_name = Some(n.clone());
                        true
                    } else if let ExprKind::New { class, type_args, .. } = &v.node {
                        llvm_ty = "i64*".to_string();
                        struct_name = Some(self.effective_class_name(class, type_args));
                        true
                    } else if let ExprKind::MapLiteral(_) = &v.node {
                        llvm_ty = "i8*".to_string();
                        struct_name = Some("Map".to_string());
                        true
                    } else if let ExprKind::Call { func, .. } = &v.node {
                        if matches!(&func.node, ExprKind::Ident(n) if n == "open") {
                            llvm_ty = "i8*".to_string();
                            struct_name = Some("File".to_string());
                            true
                        } else if matches!(&func.node, ExprKind::Ident(n) if n == "split") {
                            llvm_ty = "i64*".to_string();
                            struct_name = Some("Array:String".to_string());
                            true
                        } else { false }
                    } else if let ExprKind::MethodCall { method, .. } = &v.node {
                        if method == "split" {
                            llvm_ty = "i64*".to_string();
                            struct_name = Some("Array:String".to_string());
                            true
                        } else { false }
                    } else if let ExprKind::ArrayLiteral(elems) = &v.node {
                        llvm_ty = "i64*".to_string();
                        let is_str_ann = matches!(ty, Some(Type::Array(inner)) if matches!(inner.as_ref(), Type::String))
                            || matches!(ty, Some(Type::Generic { name, args }) if name == "Array" && args.first().map(|a| matches!(a, Type::String)).unwrap_or(false));
                        let is_str_lit = elems.first().map(|e| matches!(&e.node, ExprKind::Literal(Literal::String(_)))).unwrap_or(false);
                        if is_str_ann || is_str_lit {
                            struct_name = Some("Array:String".to_string());
                            llvm_ty = "i64*".to_string();
                        }
                        true
                    } else if matches!(&v.node, ExprKind::Tuple(_)) {
                        llvm_ty = "i64*".to_string();
                        true
                    } else if matches!(&v.node, ExprKind::Lambda { .. }) {
                        llvm_ty = "i64*".to_string();
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };

                if struct_name.is_none() {
                    if let Some(Type::Named(ann)) = ty {
                        struct_name = Some(ann.clone());
                        if self.defined_classes.contains(ann.as_str()) {
                            llvm_ty = "i64*".to_string();
                        }
                    } else if let Some(Type::Map(_, _)) = ty {
                        struct_name = Some("Map".to_string());
                        llvm_ty = "i8*".to_string();
                    } else if matches!(ty, Some(Type::Array(inner)) if matches!(inner.as_ref(), Type::String))
                        || matches!(ty, Some(Type::Generic { name, .. }) if name == "Array") {
                        struct_name = Some("Array:String".to_string());
                        llvm_ty = "i64*".to_string();
                    }
                }

                if let Some(val) = value {
                    let (v, val_ty) = self.gen_expr(val, ctx)?;
                    let actual_ty = if matches!(&val.node, ExprKind::Lambda { .. }) {
                        val_ty.clone()
                    } else if is_ptr {
                        llvm_ty.clone()
                    } else {
                        llvm_ty.clone()
                    };
                    let slot = ctx.locals.len();
                    ctx.locals.insert(name.clone(), (actual_ty.clone(), slot));
                    // If the declared type annotation is an interface, use it for vtable dispatch.
                    let effective_type = if let Some(Type::Named(ann)) = ty {
                        if self.known_interfaces.contains(ann.as_str()) {
                            Some(ann.clone())
                        } else {
                            struct_name.clone()
                        }
                    } else {
                        struct_name.clone()
                    };
                    if let Some(sn) = effective_type {
                        ctx.local_types.insert(name.clone(), sn);
                    }
                    writeln!(&mut self.ir, "%{} = alloca {}", name, actual_ty).unwrap();
                    writeln!(
                        &mut self.ir,
                        "store {} {}, {}* %{}",
                        val_ty, v, actual_ty, name
                    )
                    .unwrap();
                } else {
                    let slot = ctx.locals.len();
                    ctx.locals.insert(name.clone(), (llvm_ty.clone(), slot));
                    if let Some(sn) = &struct_name {
                        ctx.local_types.insert(name.clone(), sn.clone());
                    }
                    writeln!(&mut self.ir, "%{} = alloca {}", name, llvm_ty).unwrap();
                }
            }
            StmtKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let (cond_val, cond_ty) = self.gen_expr(cond, ctx)?;
                let cond_i1 = if cond_ty != "i1" {
                    let tmp = self.temp();
                    writeln!(&mut self.ir, "{} = icmp ne {} {}, 0", tmp, cond_ty, cond_val).unwrap();
                    tmp
                } else {
                    cond_val
                };
                let then_bb = self.new_bb("then");
                let else_bb = self.new_bb("else");
                let merge_bb = self.new_bb("ifcont");
                writeln!(
                    &mut self.ir,
                    "br i1 {}, label %{}, label %{}",
                    cond_i1, then_bb, else_bb
                )
                .unwrap();
                writeln!(&mut self.ir, "{}:", then_bb).unwrap();
                self.gen_stmt_body(then_branch, ctx)?;
                writeln!(&mut self.ir, "br label %{}", merge_bb).unwrap();
                writeln!(&mut self.ir, "{}:", else_bb).unwrap();
                if let Some(else_stmt) = else_branch {
                    self.gen_stmt_body(else_stmt, ctx)?;
                }
                writeln!(&mut self.ir, "br label %{}", merge_bb).unwrap();
                writeln!(&mut self.ir, "{}:", merge_bb).unwrap();
            }
            StmtKind::While { cond, body } => {
                let loop_bb = self.new_bb("loop");
                let body_bb = self.new_bb("loopbody");
                let end_bb = self.new_bb("loopend");

                let old_break = ctx.break_target.take();
                let old_continue = ctx.continue_target.take();
                ctx.break_target = Some(end_bb.clone());
                ctx.continue_target = Some(loop_bb.clone());

                writeln!(&mut self.ir, "br label %{}", loop_bb).unwrap();
                writeln!(&mut self.ir, "{}:", loop_bb).unwrap();
                let (cond_val, cond_ty) = self.gen_expr(cond, ctx)?;
                let cond_i1 = if cond_ty != "i1" {
                    let tmp = self.temp();
                    writeln!(&mut self.ir, "{} = icmp ne {} {}, 0", tmp, cond_ty, cond_val).unwrap();
                    tmp
                } else {
                    cond_val
                };
                writeln!(
                    &mut self.ir,
                    "br i1 {}, label %{}, label %{}",
                    cond_i1, body_bb, end_bb
                )
                .unwrap();
                writeln!(&mut self.ir, "{}:", body_bb).unwrap();
                self.gen_stmt_body(body, ctx)?;
                writeln!(&mut self.ir, "br label %{}", loop_bb).unwrap();
                writeln!(&mut self.ir, "{}:", end_bb).unwrap();

                ctx.break_target = old_break;
                ctx.continue_target = old_continue;
            }
            StmtKind::ForC {
                init,
                cond,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    self.gen_stmt_body(init_stmt, ctx)?;
                }

                let loop_bb = self.new_bb("forcond");
                let body_bb = self.new_bb("forbody");
                let update_bb = self.new_bb("forupdate");
                let end_bb = self.new_bb("forend");

                writeln!(&mut self.ir, "br label %{}", loop_bb).unwrap();
                writeln!(&mut self.ir, "{}:", loop_bb).unwrap();

                if let Some(cond_expr) = cond {
                    let (cond_val, cond_ty) = self.gen_expr(cond_expr, ctx)?;
                    let cond_i1 = if cond_ty != "i1" {
                        let tmp = self.temp();
                        writeln!(&mut self.ir, "{} = icmp ne {} {}, 0", tmp, cond_ty, cond_val).unwrap();
                        tmp
                    } else { cond_val };
                    writeln!(
                        &mut self.ir,
                        "br i1 {}, label %{}, label %{}",
                        cond_i1, body_bb, end_bb
                    )
                    .unwrap();
                } else {
                    writeln!(&mut self.ir, "br label %{}", body_bb).unwrap();
                }

                writeln!(&mut self.ir, "{}:", body_bb).unwrap();
                self.gen_stmt_body(body, ctx)?;
                writeln!(&mut self.ir, "br label %{}", update_bb).unwrap();

                writeln!(&mut self.ir, "{}:", update_bb).unwrap();
                if let Some(update_expr) = update {
                    self.gen_expr(update_expr, ctx)?;
                }
                writeln!(&mut self.ir, "br label %{}", loop_bb).unwrap();

                writeln!(&mut self.ir, "{}:", end_bb).unwrap();
            }
            StmtKind::Break => {
                if let Some(ref break_bb) = ctx.break_target.clone() {
                    writeln!(&mut self.ir, "br label %{}", break_bb).unwrap();
                }
            }
            StmtKind::Continue => {
                if let Some(ref cont_bb) = ctx.continue_target.clone() {
                    writeln!(&mut self.ir, "br label %{}", cont_bb).unwrap();
                }
            }
            StmtKind::Throw(expr) => {
                let (val, val_ty) = self.gen_expr(expr, ctx)?;
                if let Some((catch_bb, error_var)) = &ctx.error_catch {
                    let store_val = if val_ty == "double" || val_ty == "float" {
                        let cast = self.temp();
                        writeln!(&mut self.ir, "{} = bitcast {} {} to i64", cast, val_ty, val).unwrap();
                        cast
                    } else if val_ty != "i64" && val_ty != "i1" && !val_ty.is_empty() {
                        let cast = self.temp();
                        writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", cast, val_ty, val).unwrap();
                        cast
                    } else {
                        val
                    };
                    let (catch_bb, error_var) = (catch_bb.clone(), error_var.clone());
                    writeln!(&mut self.ir, "store i64 {}, i64* {}", store_val, error_var).unwrap();
                    writeln!(&mut self.ir, "br label %{}", catch_bb).unwrap();
                }
            }
            StmtKind::Try {
                body,
                catches,
                finally,
            } => {
                self.gen_try_stmt(body, catches, finally.as_deref(), ctx)?;
            }
            StmtKind::For { var, iter, body } => {
                let is_range = matches!(iter.node, ExprKind::Range { .. })
                    || matches!(&iter.node, ExprKind::Ident(n) if ctx.range_vars.contains(n));
                let is_str_arr = if let ExprKind::Ident(n) = &iter.node {
                    ctx.local_types.get(n).map(|t| t == "Array:String").unwrap_or(false)
                } else { false };
                let (iter_ptr, iter_ty) = self.gen_expr(iter, ctx)?;
                let is_string = iter_ty == "i8*";

                // arr_ptr: Some(ptr) for array/string, None for range
                // str_ptr: Some(i8*) for string iteration
                let (start_val, end_val, arr_ptr, str_ptr) = if is_range {
                    let s_gep = self.temp();
                    writeln!(&mut self.ir, "{} = getelementptr i64, ptr {}, i64 0", s_gep, iter_ptr).unwrap();
                    let sv = self.temp();
                    writeln!(&mut self.ir, "{} = load i64, i64* {}", sv, s_gep).unwrap();
                    let e_gep = self.temp();
                    writeln!(&mut self.ir, "{} = getelementptr i64, ptr {}, i64 1", e_gep, iter_ptr).unwrap();
                    let ev = self.temp();
                    writeln!(&mut self.ir, "{} = load i64, i64* {}", ev, e_gep).unwrap();
                    (sv, ev, None, None)
                } else if is_string {
                    // String: iterate bytes, length via tinox_string_length
                    let len_val = self.temp();
                    writeln!(&mut self.ir, "{} = call i64 @tinox_string_length(i8* {})", len_val, iter_ptr).unwrap();
                    ("0".to_string(), len_val, None, Some(iter_ptr))
                } else {
                    // Array: length at data_ptr[-1]
                    let len_ptr = self.temp();
                    writeln!(&mut self.ir, "{} = getelementptr i64, ptr {}, i64 -1", len_ptr, iter_ptr).unwrap();
                    let len_val = self.temp();
                    writeln!(&mut self.ir, "{} = load i64, i64* {}", len_val, len_ptr).unwrap();
                    ("0".to_string(), len_val, Some(iter_ptr), None)
                };

                // Give loop variable a unique LLVM slot to avoid duplicate alloca on re-use
                let var_slot = format!("{}_{}", var, self.temp_count);
                self.temp_count += 1;
                writeln!(&mut self.ir, "%{} = alloca i64", var_slot).unwrap();
                writeln!(&mut self.ir, "store i64 {}, i64* %{}", start_val, var_slot).unwrap();
                ctx.locals.insert(var.clone(), ("i64".to_string(), ctx.locals.len()));
                ctx.local_slots.insert(var.clone(), var_slot.clone());

                let needs_separate_idx = arr_ptr.is_some() || str_ptr.is_some();
                let idx_slot = if needs_separate_idx {
                    let s = format!("for_idx_{}", self.temp_count);
                    self.temp_count += 1;
                    writeln!(&mut self.ir, "%{} = alloca i64", s).unwrap();
                    writeln!(&mut self.ir, "store i64 0, i64* %{}", s).unwrap();
                    s
                } else {
                    // Range: var_slot IS the counter
                    var_slot.clone()
                };

                let cond_bb = self.new_bb("for_cond");
                let body_bb = self.new_bb("for_body");
                let end_bb = self.new_bb("for_end");

                let old_break = ctx.break_target.take();
                let old_continue = ctx.continue_target.take();
                ctx.break_target = Some(end_bb.clone());
                ctx.continue_target = Some(cond_bb.clone());

                writeln!(&mut self.ir, "br label %{}", cond_bb).unwrap();
                writeln!(&mut self.ir, "{}:", cond_bb).unwrap();
                let cur_idx = self.temp();
                writeln!(&mut self.ir, "{} = load i64, i64* %{}", cur_idx, idx_slot).unwrap();
                let cmp = self.temp();
                writeln!(&mut self.ir, "{} = icmp slt i64 {}, {}", cmp, cur_idx, end_val).unwrap();
                writeln!(&mut self.ir, "br i1 {}, label %{}, label %{}", cmp, body_bb, end_bb).unwrap();

                writeln!(&mut self.ir, "{}:", body_bb).unwrap();
                if let Some(aptr) = &arr_ptr {
                    let elem_ptr = self.temp();
                    writeln!(&mut self.ir, "{} = getelementptr i64, ptr {}, i64 {}", elem_ptr, aptr, cur_idx).unwrap();
                    let elem_raw = self.temp();
                    writeln!(&mut self.ir, "{} = load i64, i64* {}", elem_raw, elem_ptr).unwrap();
                    if is_str_arr {
                        // store the raw i64 (which holds a pointer), loop body casts via local_types
                        writeln!(&mut self.ir, "store i64 {}, i64* %{}", elem_raw, var_slot).unwrap();
                        ctx.local_types.insert(var.clone(), "Array:String:elem".to_string());
                    } else {
                        writeln!(&mut self.ir, "store i64 {}, i64* %{}", elem_raw, var_slot).unwrap();
                    }
                } else if let Some(sptr) = &str_ptr {
                    // Load byte at sptr[cur_idx], zext to i64, store to var
                    let bptr = self.temp();
                    writeln!(&mut self.ir, "{} = getelementptr i8, ptr {}, i64 {}", bptr, sptr, cur_idx).unwrap();
                    let byte = self.temp();
                    writeln!(&mut self.ir, "{} = load i8, i8* {}", byte, bptr).unwrap();
                    let ext = self.temp();
                    writeln!(&mut self.ir, "{} = zext i8 {} to i64", ext, byte).unwrap();
                    writeln!(&mut self.ir, "store i64 {}, i64* %{}", ext, var_slot).unwrap();
                }
                self.gen_stmt_body(body, ctx)?;

                let next_idx = self.temp();
                writeln!(&mut self.ir, "{} = load i64, i64* %{}", next_idx, idx_slot).unwrap();
                let inc = self.temp();
                writeln!(&mut self.ir, "{} = add i64 {}, 1", inc, next_idx).unwrap();
                writeln!(&mut self.ir, "store i64 {}, i64* %{}", inc, idx_slot).unwrap();
                writeln!(&mut self.ir, "br label %{}", cond_bb).unwrap();
                writeln!(&mut self.ir, "{}:", end_bb).unwrap();

                ctx.break_target = old_break;
                ctx.continue_target = old_continue;
            }
            StmtKind::Loop { body } => {
                let loop_bb = self.new_bb("loop_body");
                let end_bb = self.new_bb("loop_end");

                let old_break = ctx.break_target.take();
                let old_continue = ctx.continue_target.take();
                ctx.break_target = Some(end_bb.clone());
                ctx.continue_target = Some(loop_bb.clone());

                writeln!(&mut self.ir, "br label %{}", loop_bb).unwrap();
                writeln!(&mut self.ir, "{}:", loop_bb).unwrap();
                self.gen_stmt_body(body, ctx)?;
                writeln!(&mut self.ir, "br label %{}", loop_bb).unwrap();
                writeln!(&mut self.ir, "{}:", end_bb).unwrap();

                ctx.break_target = old_break;
                ctx.continue_target = old_continue;
            }
            StmtKind::Select { arms, default } => {
                let select_bb = self.new_bb("select_try");
                let end_bb = self.new_bb("select_end");

                // Allocate a slot for each arm's received value
                let arm_slots: Vec<String> = arms.iter().map(|_| {
                    let slot = format!("%sel_val_{}", self.temp_count);
                    self.temp_count += 1;
                    writeln!(&mut self.ir, "{} = alloca i64", slot).unwrap();
                    slot
                }).collect();

                writeln!(&mut self.ir, "br label %{}", select_bb).unwrap();
                writeln!(&mut self.ir, "{}:", select_bb).unwrap();

                let next_bb = if default.is_some() {
                    self.new_bb("select_default")
                } else {
                    self.new_bb("select_retry")
                };

                let _arm_body_bbs: Vec<String> = arms.iter().map(|arm| {
                    format!("sel_arm_{}_{}", arm.var, self.temp_count)
                }).collect();
                // Patch arm_body_bbs to have unique names
                let arm_body_bbs: Vec<String> = (0..arms.len()).map(|i| {
                    format!("sel_arm_{}", self.temp_count + i)
                }).collect();
                self.temp_count += arms.len();

                // Emit try_recv checks for each arm, chained
                for (i, (arm, slot)) in arms.iter().zip(arm_slots.iter()).enumerate() {
                    let fail_bb = if i + 1 < arms.len() {
                        format!("sel_try_{}", self.temp_count + i)
                    } else {
                        next_bb.clone()
                    };
                    let (ch_ptr, _) = self.gen_expr(&arm.channel, ctx)?;
                    // cast channel to i8* if needed
                    let ch_i8 = self.temp();
                    writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", ch_i8, ch_ptr).unwrap();
                    let ok = self.temp();
                    writeln!(&mut self.ir, "{} = call i1 @tinox_channel_try_recv(i8* {}, i64* {})", ok, ch_i8, slot).unwrap();
                    writeln!(&mut self.ir, "br i1 {}, label %{}, label %{}", ok, arm_body_bbs[i], fail_bb).unwrap();
                    if i + 1 < arms.len() {
                        writeln!(&mut self.ir, "{}:", fail_bb).unwrap();
                    }
                }
                self.temp_count += arms.len();

                // Emit arm bodies
                for (i, (arm, slot)) in arms.iter().zip(arm_slots.iter()).enumerate() {
                    writeln!(&mut self.ir, "{}:", arm_body_bbs[i]).unwrap();
                    let val = self.temp();
                    writeln!(&mut self.ir, "{} = load i64, i64* {}", val, slot).unwrap();
                    // Bind the received value to arm.var
                    writeln!(&mut self.ir, "%{} = alloca i64", arm.var).unwrap();
                    writeln!(&mut self.ir, "store i64 {}, i64* %{}", val, arm.var).unwrap();
                    let slot_i = ctx.locals.len();
                    ctx.locals.insert(arm.var.clone(), ("i64".to_string(), slot_i));
                    ctx.params.insert(arm.var.clone());
                    self.gen_stmt_body(&arm.body, ctx)?;
                    ctx.locals.remove(&arm.var);
                    ctx.params.remove(&arm.var);
                    writeln!(&mut self.ir, "br label %{}", end_bb).unwrap();
                }

                // Default or blocking retry with yield
                if let Some(def_body) = default {
                    writeln!(&mut self.ir, "{}:", next_bb).unwrap();
                    self.gen_stmt_body(def_body, ctx)?;
                    writeln!(&mut self.ir, "br label %{}", end_bb).unwrap();
                } else {
                    writeln!(&mut self.ir, "{}:", next_bb).unwrap();
                    let yield_tmp = self.temp();
                    writeln!(&mut self.ir, "{} = call i32 @sched_yield()", yield_tmp).unwrap();
                    writeln!(&mut self.ir, "br label %{}", select_bb).unwrap();
                }

                writeln!(&mut self.ir, "{}:", end_bb).unwrap();
            }
            StmtKind::Assignment { target, value } => {
                if let ExprKind::Ident(name) = &target.node {
                    let name = name.clone();
                    if let Some((ty, _)) = ctx.locals.get(&name) {
                        let ty = ty.clone();
                        let slot = ctx.local_slots.get(&name).cloned().unwrap_or_else(|| name.clone());
                        let (val, _) = self.gen_expr(value, ctx)?;
                        writeln!(&mut self.ir, "store {} {}, {}* %{}", ty, val, ty, slot).unwrap();
                    }
                } else if let ExprKind::FieldAccess { obj, field } = &target.node {
                    let (obj_raw, obj_ty) = self.gen_expr(obj, ctx)?;
                    // If the obj evaluated to i64 (a loaded pointer), restore it to a ptr
                    let obj_ptr = if obj_ty == "i64" {
                        let cast = self.temp();
                        writeln!(&mut self.ir, "{} = inttoptr i64 {} to i64*", cast, obj_raw).unwrap();
                        cast
                    } else {
                        obj_raw
                    };
                    let struct_name = self.infer_struct_type(obj, ctx);
                    let offset = if let Some(sname) = struct_name {
                        if let Some(fields) = self.struct_layouts.get(&sname) {
                            fields.iter().position(|f| f == field).unwrap_or(0) as i64
                        } else {
                            0
                        }
                    } else {
                        0
                    };
                    let (val, val_ty) = self.gen_expr(value, ctx)?;
                    // Uniform i64 field storage: floats → bitcast, i1 → zext, pointers → ptrtoint
                    let store_val = if val_ty == "i1" {
                        let cast = self.temp();
                        writeln!(&mut self.ir, "{} = zext i1 {} to i64", cast, val).unwrap();
                        cast
                    } else if val_ty == "double" || val_ty == "float" {
                        let cast = self.temp();
                        writeln!(&mut self.ir, "{} = bitcast {} {} to i64", cast, val_ty, val).unwrap();
                        cast
                    } else if val_ty != "i64" && !val_ty.is_empty() {
                        let cast = self.temp();
                        writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", cast, val_ty, val).unwrap();
                        cast
                    } else {
                        val
                    };
                    let field_ptr = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = getelementptr i64, ptr {}, i64 {}",
                        field_ptr, obj_ptr, offset
                    )
                    .unwrap();
                    writeln!(&mut self.ir, "store i64 {}, i64* {}", store_val, field_ptr)
                        .unwrap();
                } else if let ExprKind::Index { obj, index } = &target.node {
                    // Detect Map type for map[key] = val → tinox_map_set
                    let obj_declared_type = if let ExprKind::Ident(n) = &obj.node {
                        ctx.local_types.get(n.as_str()).cloned()
                    } else { None };
                    let is_map = obj_declared_type.as_deref() == Some("Map");

                    let (idx_val, idx_ty) = self.gen_expr(index, ctx)?;
                    let (base_ptr, base_ty) = if let ExprKind::Ident(name) = &obj.node {
                        if ctx.params.contains(name) {
                            self.gen_expr(obj, ctx)?
                        } else if ctx.locals.contains_key(name) {
                            let (var_ty, _) = ctx.locals.get(name).unwrap();
                            let loaded_ptr = self.temp();
                            writeln!(
                                &mut self.ir,
                                "{} = load {}, {}* %{}",
                                loaded_ptr, var_ty, var_ty, name
                            )
                            .unwrap();
                            (loaded_ptr, var_ty.clone())
                        } else {
                            self.gen_expr(obj, ctx)?
                        }
                    } else {
                        self.gen_expr(obj, ctx)?
                    };
                    let (val, val_ty) = self.gen_expr(value, ctx)?;

                    if is_map {
                        // Map: tinox_map_set(i8* map, i8* key, i64 val)
                        let map_i8 = if base_ty == "i8*" { base_ptr.clone() } else {
                            let c = self.temp();
                            writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, base_ptr).unwrap();
                            c
                        };
                        let key_i8 = if idx_ty == "i8*" { idx_val.clone() } else {
                            let c = self.temp();
                            writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, idx_val).unwrap();
                            c
                        };
                        let store_val = if val_ty == "i8*" {
                            let c = self.temp();
                            writeln!(&mut self.ir, "{} = ptrtoint i8* {} to i64", c, val).unwrap();
                            c
                        } else { val };
                        writeln!(&mut self.ir, "call void @tinox_map_set(i8* {}, i8* {}, i64 {})", map_i8, key_i8, store_val).unwrap();
                    } else {
                        let ptr_name = self.temp();
                        writeln!(
                            &mut self.ir,
                            "{} = getelementptr i64, ptr {}, i64 {}",
                            ptr_name, base_ptr, idx_val
                        )
                        .unwrap();
                        // Strings stored as i64 (ptrtoint); bools need zext; others direct
                        let store_val = if val_ty == "i8*" || val_ty == "i64*" {
                            let cast = self.temp();
                            writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", cast, val_ty, val).unwrap();
                            cast
                        } else if val_ty == "i1" {
                            let cast = self.temp();
                            writeln!(&mut self.ir, "{} = zext i1 {} to i64", cast, val).unwrap();
                            cast
                        } else {
                            val
                        };
                        writeln!(&mut self.ir, "store i64 {}, i64* {}", store_val, ptr_name).unwrap();
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn gen_expr(
        &mut self,
        expr: &tinox_parser::Expr,
        ctx: &mut GenCtx,
    ) -> Result<(String, String), ErrorBag> {
        match &expr.node {
            ExprKind::Literal(lit) => self.gen_literal(lit),
            ExprKind::Ident(name) => {
                if ctx.params.contains(name) {
                    let ty = ctx.locals.get(name)
                        .map(|(t, _)| t.clone())
                        .unwrap_or_else(|| "i64".to_string());
                    Ok((format!("%{}", name), ty))
                } else if let Some((ty, _)) = ctx.locals.get(name) {
                    let ty = ty.clone();
                    let slot = ctx.local_slots.get(name).cloned().unwrap_or_else(|| name.clone());
                    let val = self.temp();
                    writeln!(&mut self.ir, "{} = load {}, {}* %{}", val, ty, ty, slot).unwrap();
                    if ctx.local_types.get(name).map(|t| t == "Array:String:elem").unwrap_or(false) {
                        let str_ptr = self.temp();
                        writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", str_ptr, val).unwrap();
                        Ok((str_ptr, "i8*".to_string()))
                    } else {
                        Ok((val, ty))
                    }
                } else {
                    Ok((format!("%{}", name), "i64".to_string()))
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let (l, lt) = self.gen_expr(lhs, ctx)?;
                let (r, _rt) = self.gen_expr(rhs, ctx)?;
                let result = self.temp();
                let float = Self::is_float(&lt);
                match op {
                    tinox_parser::BinaryOp::Add => {
                        if lt == "i8*" {
                            writeln!(&mut self.ir, "{} = call i8* @tinox_string_concat(i8* {}, i8* {})", result, l, r).unwrap()
                        } else if float {
                            writeln!(&mut self.ir, "{} = fadd {} {}, {}", result, lt, l, r).unwrap()
                        } else {
                            writeln!(&mut self.ir, "{} = add {} {}, {}", result, lt, l, r).unwrap()
                        }
                    }
                    tinox_parser::BinaryOp::Sub => {
                        if float {
                            writeln!(&mut self.ir, "{} = fsub {} {}, {}", result, lt, l, r).unwrap()
                        } else {
                            writeln!(&mut self.ir, "{} = sub {} {}, {}", result, lt, l, r).unwrap()
                        }
                    }
                    tinox_parser::BinaryOp::Mul => {
                        if float {
                            writeln!(&mut self.ir, "{} = fmul {} {}, {}", result, lt, l, r).unwrap()
                        } else {
                            writeln!(&mut self.ir, "{} = mul {} {}, {}", result, lt, l, r).unwrap()
                        }
                    }
                    tinox_parser::BinaryOp::Div => {
                        if float {
                            writeln!(&mut self.ir, "{} = fdiv {} {}, {}", result, lt, l, r).unwrap()
                        } else {
                            writeln!(&mut self.ir, "{} = sdiv {} {}, {}", result, lt, l, r).unwrap()
                        }
                    }
                    tinox_parser::BinaryOp::Mod => {
                        if float {
                            writeln!(&mut self.ir, "{} = frem {} {}, {}", result, lt, l, r).unwrap()
                        } else {
                            writeln!(&mut self.ir, "{} = srem {} {}, {}", result, lt, l, r).unwrap()
                        }
                    }
                    tinox_parser::BinaryOp::Eq => {
                        if float {
                            writeln!(&mut self.ir, "{} = fcmp oeq {} {}, {}", result, lt, l, r).unwrap()
                        } else {
                            writeln!(&mut self.ir, "{} = icmp eq {} {}, {}", result, lt, l, r).unwrap()
                        }
                        return Ok((result, "i1".to_string()));
                    }
                    tinox_parser::BinaryOp::Ne => {
                        if float {
                            writeln!(&mut self.ir, "{} = fcmp one {} {}, {}", result, lt, l, r).unwrap()
                        } else {
                            writeln!(&mut self.ir, "{} = icmp ne {} {}, {}", result, lt, l, r).unwrap()
                        }
                        return Ok((result, "i1".to_string()));
                    }
                    tinox_parser::BinaryOp::Lt => {
                        if float {
                            writeln!(&mut self.ir, "{} = fcmp olt {} {}, {}", result, lt, l, r).unwrap()
                        } else {
                            writeln!(&mut self.ir, "{} = icmp slt {} {}, {}", result, lt, l, r).unwrap()
                        }
                        return Ok((result, "i1".to_string()));
                    }
                    tinox_parser::BinaryOp::Le => {
                        if float {
                            writeln!(&mut self.ir, "{} = fcmp ole {} {}, {}", result, lt, l, r).unwrap()
                        } else {
                            writeln!(&mut self.ir, "{} = icmp sle {} {}, {}", result, lt, l, r).unwrap()
                        }
                        return Ok((result, "i1".to_string()));
                    }
                    tinox_parser::BinaryOp::Gt => {
                        if float {
                            writeln!(&mut self.ir, "{} = fcmp ogt {} {}, {}", result, lt, l, r).unwrap()
                        } else {
                            writeln!(&mut self.ir, "{} = icmp sgt {} {}, {}", result, lt, l, r).unwrap()
                        }
                        return Ok((result, "i1".to_string()));
                    }
                    tinox_parser::BinaryOp::Ge => {
                        if float {
                            writeln!(&mut self.ir, "{} = fcmp oge {} {}, {}", result, lt, l, r).unwrap()
                        } else {
                            writeln!(&mut self.ir, "{} = icmp sge {} {}, {}", result, lt, l, r).unwrap()
                        }
                        return Ok((result, "i1".to_string()));
                    }
                    tinox_parser::BinaryOp::And => {
                        writeln!(&mut self.ir, "{} = and i1 {}, {}", result, l, r).unwrap();
                        return Ok((result, "i1".to_string()));
                    }
                    tinox_parser::BinaryOp::Or => {
                        writeln!(&mut self.ir, "{} = or i1 {}, {}", result, l, r).unwrap();
                        return Ok((result, "i1".to_string()));
                    }
                    tinox_parser::BinaryOp::BitAnd => {
                        writeln!(&mut self.ir, "{} = and {} {}, {}", result, lt, l, r).unwrap()
                    }
                    tinox_parser::BinaryOp::BitOr => {
                        writeln!(&mut self.ir, "{} = or {} {}, {}", result, lt, l, r).unwrap()
                    }
                    tinox_parser::BinaryOp::Xor => {
                        writeln!(&mut self.ir, "{} = xor {} {}, {}", result, lt, l, r).unwrap()
                    }
                    tinox_parser::BinaryOp::Shl => {
                        writeln!(&mut self.ir, "{} = shl {} {}, {}", result, lt, l, r).unwrap()
                    }
                    tinox_parser::BinaryOp::Shr => {
                        writeln!(&mut self.ir, "{} = lshr {} {}, {}", result, lt, l, r).unwrap()
                    }
                    tinox_parser::BinaryOp::ShrArith => {
                        writeln!(&mut self.ir, "{} = ashr {} {}, {}", result, lt, l, r).unwrap()
                    }
                }
                Ok((result, lt))
            }
            ExprKind::Unary { op, operand } => {
                let (val, ty) = self.gen_expr(operand, ctx)?;
                let result = self.temp();
                match op {
                    tinox_parser::UnaryOp::Neg => {
                        if Self::is_float(&ty) {
                            writeln!(&mut self.ir, "{} = fneg {} {}", result, ty, val).unwrap()
                        } else {
                            writeln!(&mut self.ir, "{} = sub {} 0, {}", result, ty, val).unwrap()
                        }
                    }
                    tinox_parser::UnaryOp::Not => {
                        writeln!(&mut self.ir, "{} = xor {} 1, {}", result, ty, val).unwrap()
                    }
                    tinox_parser::UnaryOp::BitNot => {
                        writeln!(&mut self.ir, "{} = xor {} -1, {}", result, ty, val).unwrap()
                    }
                }
                Ok((result, ty))
            }
            ExprKind::Call { func, args } => {
                let mut args_str = String::new();
                let mut arg_types = Vec::new();
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        args_str.push_str(", ");
                    }
                    let (val, ty) = self.gen_expr(arg, ctx)?;
                    args_str.push_str(&format!("{} {}", ty, val));
                    arg_types.push(ty);
                }
                let fn_name = match &func.node {
                    ExprKind::Ident(name) => match name.as_str() {
                        "main" => "tinox_main".to_string(),
                        "print" | "println" => {
                            if !args.is_empty() {
                                let ty = &arg_types[0];
                                let llvm_fn = match ty.as_str() {
                                    "i8*" => "tinox_print_string",
                                    "double" => "tinox_print_float",
                                    "i1" => "tinox_print_bool",
                                    "i32" => "tinox_print_char",
                                    _ => "tinox_print_int",
                                };
                                writeln!(&mut self.ir, "call void @{}({})", llvm_fn, args_str).unwrap();
                            }
                            if name == "println" {
                                writeln!(&mut self.ir, "call void @tinox_print_newline()").unwrap();
                            }
                            return Ok(("0".to_string(), "void".to_string()));
                        }
                        "len" => {
                            let (ptr, ty) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            if ty == "i8*" {
                                writeln!(&mut self.ir, "{} = call i64 @tinox_string_length(i8* {})", result, ptr).unwrap();
                            } else {
                                // Array: length is stored at index -1 by convention (or 0 offset)
                                let len_ptr = self.temp();
                                writeln!(&mut self.ir, "{} = getelementptr i64, ptr {}, i64 -1", len_ptr, ptr).unwrap();
                                writeln!(&mut self.ir, "{} = load i64, i64* {}", result, len_ptr).unwrap();
                            }
                            return Ok((result, "i64".to_string()));
                        }
                        "assert" => {
                            let (cond, _) = self.gen_expr(&args[0], ctx)?;
                            let ok_bb = self.new_bb("assert_ok");
                            let fail_bb = self.new_bb("assert_fail");
                            writeln!(&mut self.ir, "br i1 {}, label %{}, label %{}", cond, ok_bb, fail_bb).unwrap();
                            writeln!(&mut self.ir, "{}:", fail_bb).unwrap();
                            writeln!(&mut self.ir, "call void @tinox_panic(i64 1)").unwrap();
                            writeln!(&mut self.ir, "unreachable").unwrap();
                            writeln!(&mut self.ir, "{}:", ok_bb).unwrap();
                            return Ok(("0".to_string(), "void".to_string()));
                        }
                        "push" => {
                            let (arr, _) = self.gen_expr(&args[0], ctx)?;
                            let (val, _) = self.gen_expr(&args[1], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64* @tinox_array_push(i64* {}, i64 {})", result, arr, val).unwrap();
                            return Ok((result, "i64*".to_string()));
                        }
                        "pop" => {
                            let (arr, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64* @tinox_array_pop(i64* {})", result, arr).unwrap();
                            return Ok((result, "i64*".to_string()));
                        }
                        "first" => {
                            let (arr, _) = self.gen_expr(&args[0], ctx)?;
                            let ptr = self.temp();
                            writeln!(&mut self.ir, "{} = getelementptr i64, ptr {}, i64 0", ptr, arr).unwrap();
                            let val = self.temp();
                            writeln!(&mut self.ir, "{} = load i64, i64* {}", val, ptr).unwrap();
                            return Ok((val, "i64".to_string()));
                        }
                        "last" => {
                            let (arr, _) = self.gen_expr(&args[0], ctx)?;
                            let len_ptr = self.temp();
                            writeln!(&mut self.ir, "{} = getelementptr i64, ptr {}, i64 -1", len_ptr, arr).unwrap();
                            let len_val = self.temp();
                            writeln!(&mut self.ir, "{} = load i64, i64* {}", len_val, len_ptr).unwrap();
                            let last_idx = self.temp();
                            writeln!(&mut self.ir, "{} = sub i64 {}, 1", last_idx, len_val).unwrap();
                            let elem_ptr = self.temp();
                            writeln!(&mut self.ir, "{} = getelementptr i64, ptr {}, i64 {}", elem_ptr, arr, last_idx).unwrap();
                            let val = self.temp();
                            writeln!(&mut self.ir, "{} = load i64, i64* {}", val, elem_ptr).unwrap();
                            return Ok((val, "i64".to_string()));
                        }
                        "slice" => {
                            let (arr, _) = self.gen_expr(&args[0], ctx)?;
                            let (from, _) = self.gen_expr(&args[1], ctx)?;
                            let (to, _) = self.gen_expr(&args[2], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64* @tinox_array_slice(i64* {}, i64 {}, i64 {})", result, arr, from, to).unwrap();
                            return Ok((result, "i64*".to_string()));
                        }
                        "abs" => {
                            let (val, ty) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            if ty == "double" {
                                writeln!(&mut self.ir, "{} = call double @llvm.fabs.f64(double {})", result, val).unwrap();
                                return Ok((result, "double".to_string()));
                            } else {
                                let neg = self.temp();
                                writeln!(&mut self.ir, "{} = sub i64 0, {}", neg, val).unwrap();
                                let cond = self.temp();
                                writeln!(&mut self.ir, "{} = icmp slt i64 {}, 0", cond, val).unwrap();
                                writeln!(&mut self.ir, "{} = select i1 {}, i64 {}, i64 {}", result, cond, neg, val).unwrap();
                                return Ok((result, "i64".to_string()));
                            }
                        }
                        "min" => {
                            let (a, ty) = self.gen_expr(&args[0], ctx)?;
                            let (b, _) = self.gen_expr(&args[1], ctx)?;
                            let cond = self.temp();
                            let result = self.temp();
                            if ty == "double" {
                                writeln!(&mut self.ir, "{} = fcmp olt double {}, {}", cond, a, b).unwrap();
                                writeln!(&mut self.ir, "{} = select i1 {}, double {}, double {}", result, cond, a, b).unwrap();
                                return Ok((result, "double".to_string()));
                            } else {
                                writeln!(&mut self.ir, "{} = icmp slt i64 {}, {}", cond, a, b).unwrap();
                                writeln!(&mut self.ir, "{} = select i1 {}, i64 {}, i64 {}", result, cond, a, b).unwrap();
                                return Ok((result, "i64".to_string()));
                            }
                        }
                        "max" => {
                            let (a, ty) = self.gen_expr(&args[0], ctx)?;
                            let (b, _) = self.gen_expr(&args[1], ctx)?;
                            let cond = self.temp();
                            let result = self.temp();
                            if ty == "double" {
                                writeln!(&mut self.ir, "{} = fcmp ogt double {}, {}", cond, a, b).unwrap();
                                writeln!(&mut self.ir, "{} = select i1 {}, double {}, double {}", result, cond, a, b).unwrap();
                                return Ok((result, "double".to_string()));
                            } else {
                                writeln!(&mut self.ir, "{} = icmp sgt i64 {}, {}", cond, a, b).unwrap();
                                writeln!(&mut self.ir, "{} = select i1 {}, i64 {}, i64 {}", result, cond, a, b).unwrap();
                                return Ok((result, "i64".to_string()));
                            }
                        }
                        "sqrt" => {
                            let (val, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call double @sqrt(double {})", result, val).unwrap();
                            return Ok((result, "double".to_string()));
                        }
                        "charAt" => {
                            let (ptr, _) = self.gen_expr(&args[0], ctx)?;
                            let (idx, _) = self.gen_expr(&args[1], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i8* @tinox_char_at(i8* {}, i64 {})", result, ptr, idx).unwrap();
                            return Ok((result, "i8*".to_string()));
                        }
                        "toInt" => {
                            let (val, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_string_to_int(i8* {})", result, val).unwrap();
                            return Ok((result, "i64".to_string()));
                        }
                        "toFloat" => {
                            let (val, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call double @tinox_string_to_float(i8* {})", result, val).unwrap();
                            return Ok((result, "double".to_string()));
                        }
                        "toString" => {
                            let (val, ty) = self.gen_expr(&args[0], ctx)?;
                            if ty == "i8*" {
                                // Already a string — return as-is
                                return Ok((val, "i8*".to_string()));
                            }
                            let result = self.temp();
                            let (fn_name, arg_ty) = match ty.as_str() {
                                "double" => ("tinox_float_to_string", "double"),
                                "i1"     => ("tinox_bool_to_string", "i1"),
                                _        => ("tinox_int_to_string", "i64"),
                            };
                            writeln!(&mut self.ir, "{} = call i8* @{}({} {})", result, fn_name, arg_ty, val).unwrap();
                            return Ok((result, "i8*".to_string()));
                        }
                        "pow" => {
                            let (base, _) = self.gen_expr(&args[0], ctx)?;
                            let (exp, _) = self.gen_expr(&args[1], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call double @pow(double {}, double {})", result, base, exp).unwrap();
                            return Ok((result, "double".to_string()));
                        }
                        "floor" => {
                            let (val, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call double @llvm.floor.f64(double {})", result, val).unwrap();
                            return Ok((result, "double".to_string()));
                        }
                        "ceil" => {
                            let (val, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call double @llvm.ceil.f64(double {})", result, val).unwrap();
                            return Ok((result, "double".to_string()));
                        }
                        "round" => {
                            let (val, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call double @llvm.round.f64(double {})", result, val).unwrap();
                            return Ok((result, "double".to_string()));
                        }
                        "exit" => {
                            let (code, _) = self.gen_expr(&args[0], ctx)?;
                            writeln!(&mut self.ir, "call void @exit(i64 {})", code).unwrap();
                            writeln!(&mut self.ir, "unreachable").unwrap();
                            return Ok(("0".to_string(), "void".to_string()));
                        }
                        "contains" => {
                            let (haystack, ty) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            if ty == "i8*" {
                                let (needle, _) = self.gen_expr(&args[1], ctx)?;
                                writeln!(&mut self.ir, "{} = call i64 @tinox_string_contains(i8* {}, i8* {})", result, haystack, needle).unwrap();
                                let bool_val = self.temp();
                                writeln!(&mut self.ir, "{} = trunc i64 {} to i1", bool_val, result).unwrap();
                                return Ok((bool_val, "i1".to_string()));
                            } else {
                                let (val, _) = self.gen_expr(&args[1], ctx)?;
                                writeln!(&mut self.ir, "{} = call i64 @tinox_array_contains(i64* {}, i64 {})", result, haystack, val).unwrap();
                                let bool_val = self.temp();
                                writeln!(&mut self.ir, "{} = trunc i64 {} to i1", bool_val, result).unwrap();
                                return Ok((bool_val, "i1".to_string()));
                            }
                        }
                        "indexOf" => {
                            let (haystack, ty) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            if ty == "i8*" {
                                let (needle, _) = self.gen_expr(&args[1], ctx)?;
                                writeln!(&mut self.ir, "{} = call i64 @tinox_string_index_of(i8* {}, i8* {})", result, haystack, needle).unwrap();
                            } else {
                                let (val, _) = self.gen_expr(&args[1], ctx)?;
                                writeln!(&mut self.ir, "{} = call i64 @tinox_array_index_of(i64* {}, i64 {})", result, haystack, val).unwrap();
                            }
                            return Ok((result, "i64".to_string()));
                        }
                        "toUpper" | "toUpperCase" => {
                            let (val, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i8* @tinox_string_to_upper(i8* {})", result, val).unwrap();
                            return Ok((result, "i8*".to_string()));
                        }
                        "toLower" | "toLowerCase" => {
                            let (val, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i8* @tinox_string_to_lower(i8* {})", result, val).unwrap();
                            return Ok((result, "i8*".to_string()));
                        }
                        "startsWith" => {
                            let (s, _) = self.gen_expr(&args[0], ctx)?;
                            let (prefix, _) = self.gen_expr(&args[1], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_string_starts_with(i8* {}, i8* {})", result, s, prefix).unwrap();
                            let bool_val = self.temp();
                            writeln!(&mut self.ir, "{} = trunc i64 {} to i1", bool_val, result).unwrap();
                            return Ok((bool_val, "i1".to_string()));
                        }
                        "endsWith" => {
                            let (s, _) = self.gen_expr(&args[0], ctx)?;
                            let (suffix, _) = self.gen_expr(&args[1], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_string_ends_with(i8* {}, i8* {})", result, s, suffix).unwrap();
                            let bool_val = self.temp();
                            writeln!(&mut self.ir, "{} = trunc i64 {} to i1", bool_val, result).unwrap();
                            return Ok((bool_val, "i1".to_string()));
                        }
                        "trim" => {
                            let (val, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i8* @tinox_string_trim(i8* {})", result, val).unwrap();
                            return Ok((result, "i8*".to_string()));
                        }
                        "sort" => {
                            let (arr, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64* @tinox_array_sort(i64* {})", result, arr).unwrap();
                            return Ok((result, "i64*".to_string()));
                        }
                        "reverse" => {
                            let (arr, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64* @tinox_array_reverse(i64* {})", result, arr).unwrap();
                            return Ok((result, "i64*".to_string()));
                        }
                        "split" => {
                            let (s, _) = self.gen_expr(&args[0], ctx)?;
                            let (delim, _) = self.gen_expr(&args[1], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64* @tinox_string_split(i8* {}, i8* {})", result, s, delim).unwrap();
                            return Ok((result, "i64*".to_string()));
                        }
                        "join" => {
                            let (arr, _) = self.gen_expr(&args[0], ctx)?;
                            let (sep, _) = self.gen_expr(&args[1], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i8* @tinox_string_join(i64* {}, i8* {})", result, arr, sep).unwrap();
                            return Ok((result, "i8*".to_string()));
                        }
                        "open" => {
                            let (path, _) = self.gen_expr(&args[0], ctx)?;
                            let mode = if args.len() > 1 {
                                let (m, _) = self.gen_expr(&args[1], ctx)?;
                                m
                            } else {
                                let sname = format!("str{}", self.strings.len());
                                self.strings.insert(sname.clone(), "r".to_string());
                                let ptr = self.temp();
                                writeln!(&mut self.ir, "{} = getelementptr [2 x i8], [2 x i8]* @{}, i64 0, i64 0", ptr, sname).unwrap();
                                ptr
                            };
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i8* @tinox_file_open(i8* {}, i8* {})", result, path, mode).unwrap();
                            return Ok((result, "i8*".to_string()));
                        }
                        "fileExists" => {
                            let (path, _) = self.gen_expr(&args[0], ctx)?;
                            let raw = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_file_exists(i8* {})", raw, path).unwrap();
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = icmp ne i64 {}, 0", result, raw).unwrap();
                            return Ok((result, "i1".to_string()));
                        }
                        "deleteFile" => {
                            let (path, _) = self.gen_expr(&args[0], ctx)?;
                            writeln!(&mut self.ir, "call void @tinox_file_delete(i8* {})", path).unwrap();
                            return Ok(("0".to_string(), "void".to_string()));
                        }
                        _ => name.clone(),
                    },
                    _ => "unknown_fn".to_string(),
                };
                // Check if this is a call to a generic function — monomorphize if so
                if let ExprKind::Ident(callee_name) = &func.node {
                    if let Some(gf) = self.generic_fns.get(callee_name).cloned() {
                        // Infer type bindings from argument types
                        let bindings: HashMap<String, String> = gf
                            .type_params
                            .iter()
                            .enumerate()
                            .filter_map(|(i, tp)| {
                                arg_types.get(i).map(|at| (tp.clone(), at.clone()))
                            })
                            .collect();
                        let mangled = Self::mangle_generic_name(&gf.name, &gf.type_params, &bindings);
                        // Generate specialization if not already done
                        if !self.generated_specializations.contains(&mangled) {
                            self.generated_specializations.insert(mangled.clone());
                            let specialized = Self::substitute_fn(&gf, &mangled, &bindings);
                            // emit into lambda_ir so it doesn't interrupt current function
                            let saved_ir = std::mem::take(&mut self.ir);
                            let saved_temp = self.temp_count;
                            self.temp_count = 0;
                            self.gen_fn(&specialized)?;
                            let spec_ir = std::mem::take(&mut self.ir);
                            self.ir = saved_ir;
                            self.temp_count = saved_temp;
                            self.lambda_ir.push_str(&spec_ir);
                        }
                        // Emit the call to the mangled name
                        let ret_ty = Self::type_to_llvm_with_bindings(&gf.ret_type, &bindings);
                        let result = self.temp();
                        writeln!(&mut self.ir, "  {} = call {} @{}({})", result, ret_ty, mangled, args_str).unwrap();
                        return Ok((result, ret_ty));
                    }
                }

                // Look up actual return type from pre-collected signatures
                let ret_ty = if let ExprKind::Ident(callee) = &func.node {
                    self.fn_sigs.get(callee)
                        .map(|(r, _)| r.clone())
                        .unwrap_or_else(|| arg_types.first().cloned().unwrap_or_else(|| "i64".to_string()))
                } else {
                    arg_types.first().cloned().unwrap_or_else(|| "i64".to_string())
                };
                let result = self.temp();
                let is_local_fn = if let ExprKind::Ident(name) = &func.node {
                    ctx.locals.contains_key(name)
                } else {
                    false
                };
                if is_local_fn {
                    let (fn_ptr, fn_ty) = self.gen_expr(func, ctx)?;
                    if fn_ty == "i64*" {
                        let fp_val = self.temp();
                        writeln!(&mut self.ir, "{} = load i64, i64* {}", fp_val, fn_ptr).unwrap();
                        let env_ptr = self.temp();
                        writeln!(
                            &mut self.ir,
                            "{} = getelementptr i64, ptr {}, i64 1",
                            env_ptr, fn_ptr
                        )
                        .unwrap();
                        let env_val = self.temp();
                        writeln!(&mut self.ir, "{} = load i64*, i64* {}", env_val, env_ptr)
                            .unwrap();
                        let casted_fn = self.temp();
                        writeln!(
                            &mut self.ir,
                            "{} = inttoptr i64 {} to i64 (i64, i64*)*",
                            casted_fn, fp_val
                        )
                        .unwrap();
                        writeln!(
                            &mut self.ir,
                            "{} = call {} {}({}, i64* {})",
                            result,
                            ret_ty,
                            casted_fn,
                            args_str.trim(),
                            env_val
                        )
                        .unwrap();
                    } else {
                        let casted_fn = self.temp();
                        let fn_type_str = format!("{} (i64)*", ret_ty);
                        writeln!(
                            &mut self.ir,
                            "{} = inttoptr i64 {} to {}",
                            casted_fn, fn_ptr, fn_type_str
                        )
                        .unwrap();
                        writeln!(
                            &mut self.ir,
                            "{} = call {} {}({})",
                            result, ret_ty, casted_fn, args_str
                        )
                        .unwrap();
                    }
                } else if ret_ty == "void" {
                    writeln!(
                        &mut self.ir,
                        "call void @{}({})",
                        fn_name, args_str
                    )
                    .unwrap();
                } else {
                    writeln!(
                        &mut self.ir,
                        "{} = call {} @{}({})",
                        result, ret_ty, fn_name, args_str
                    )
                    .unwrap();
                }
                Ok((result, ret_ty))
            }
            ExprKind::MethodCall { obj, method, args } => {
                // Static method call: ClassName.fnc(args) — obj is a class name, not an instance
                if let ExprKind::Ident(class_name) = &obj.node {
                    let method_key = format!("{}_{}", class_name, method);
                    if self.method_ret_types.contains_key(&method_key) {
                        // Check it really is a static method (no self in fn signature)
                        if let Some((_, param_tys)) = self.fn_sigs.get(&method_key) {
                            let _ = param_tys; // static confirmed via fn_sigs absence of self
                        }
                        // Only treat as static if the class name is not a local variable
                        if !ctx.locals.contains_key(class_name.as_str()) && !ctx.params.contains(class_name.as_str()) {
                            if self.struct_layouts.contains_key(class_name.as_str()) {
                                let mut args_str = String::new();
                                for (i, arg) in args.iter().enumerate() {
                                    if i > 0 { args_str.push_str(", "); }
                                    let (v, t) = self.gen_expr(arg, ctx)?;
                                    args_str.push_str(&format!("{} {}", t, v));
                                }
                                let ret_ty = self.method_ret_types.get(&method_key).cloned()
                                    .unwrap_or_else(|| "i64".to_string());
                                if ret_ty == "void" {
                                    writeln!(&mut self.ir, "call void @{}({})", method_key, args_str).unwrap();
                                    return Ok(("0".to_string(), "void".to_string()));
                                }
                                let result = self.temp();
                                writeln!(&mut self.ir, "{} = call {} @{}({})",
                                    result, ret_ty, method_key, args_str).unwrap();
                                return Ok((result, ret_ty));
                            }
                        }
                    }
                }

                let (obj_ptr, obj_ty) = self.gen_expr(obj, ctx)?;

                let declared_type = match &obj.node {
                    ExprKind::Ident(name) => ctx.local_types.get(name).cloned(),
                    ExprKind::This => ctx.current_struct.clone(),
                    _ => self.infer_struct_type(obj, ctx),
                };

                // Array method dispatch
                let is_array_type = declared_type.as_deref().map(|t| t == "Array:String" || t == "Array").unwrap_or(false)
                    || obj_ty == "i64*";
                if is_array_type && obj_ty != "i8*" {
                    let is_str = declared_type.as_deref() == Some("Array:String");
                    match method.as_str() {
                        "len" => {
                            let len_ptr = self.temp();
                            writeln!(&mut self.ir, "{} = getelementptr i64, ptr {}, i64 -1", len_ptr, obj_ptr).unwrap();
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = load i64, i64* {}", result, len_ptr).unwrap();
                            return Ok((result, "i64".to_string()));
                        }
                        "push" => {
                            let (val, val_ty) = self.gen_expr(&args[0], ctx)?;
                            let store_val = if val_ty == "i8*" {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = ptrtoint i8* {} to i64", c, val).unwrap();
                                c
                            } else { val };
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64* @tinox_array_push(i64* {}, i64 {})", result, obj_ptr, store_val).unwrap();
                            // Write the new pointer back to the variable (push may realloc)
                            if let ExprKind::Ident(var_name) = &obj.node {
                                let slot = ctx.local_slots.get(var_name.as_str()).cloned().unwrap_or_else(|| var_name.clone());
                                writeln!(&mut self.ir, "store i64* {}, i64** %{}", result, slot).unwrap();
                            }
                            return Ok((result, "i64*".to_string()));
                        }
                        "pop" => {
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64* @tinox_array_pop(i64* {})", result, obj_ptr).unwrap();
                            return Ok((result, "i64*".to_string()));
                        }
                        "first" => {
                            let ptr = self.temp();
                            writeln!(&mut self.ir, "{} = getelementptr i64, ptr {}, i64 0", ptr, obj_ptr).unwrap();
                            let raw = self.temp();
                            writeln!(&mut self.ir, "{} = load i64, i64* {}", raw, ptr).unwrap();
                            if is_str {
                                let s = self.temp();
                                writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", s, raw).unwrap();
                                return Ok((s, "i8*".to_string()));
                            }
                            return Ok((raw, "i64".to_string()));
                        }
                        "last" => {
                            let len_ptr = self.temp();
                            writeln!(&mut self.ir, "{} = getelementptr i64, ptr {}, i64 -1", len_ptr, obj_ptr).unwrap();
                            let len_val = self.temp();
                            writeln!(&mut self.ir, "{} = load i64, i64* {}", len_val, len_ptr).unwrap();
                            let last_idx = self.temp();
                            writeln!(&mut self.ir, "{} = sub i64 {}, 1", last_idx, len_val).unwrap();
                            let elem_ptr = self.temp();
                            writeln!(&mut self.ir, "{} = getelementptr i64, ptr {}, i64 {}", elem_ptr, obj_ptr, last_idx).unwrap();
                            let raw = self.temp();
                            writeln!(&mut self.ir, "{} = load i64, i64* {}", raw, elem_ptr).unwrap();
                            if is_str {
                                let s = self.temp();
                                writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", s, raw).unwrap();
                                return Ok((s, "i8*".to_string()));
                            }
                            return Ok((raw, "i64".to_string()));
                        }
                        "contains" => {
                            let (val, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_array_contains(i64* {}, i64 {})", result, obj_ptr, val).unwrap();
                            let b = self.temp();
                            writeln!(&mut self.ir, "{} = icmp ne i64 {}, 0", b, result).unwrap();
                            return Ok((b, "i1".to_string()));
                        }
                        "indexOf" => {
                            let (val, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_array_index_of(i64* {}, i64 {})", result, obj_ptr, val).unwrap();
                            return Ok((result, "i64".to_string()));
                        }
                        "sort" => {
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64* @tinox_array_sort(i64* {})", result, obj_ptr).unwrap();
                            return Ok((result, "i64*".to_string()));
                        }
                        "reverse" => {
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64* @tinox_array_reverse(i64* {})", result, obj_ptr).unwrap();
                            return Ok((result, "i64*".to_string()));
                        }
                        "slice" => {
                            let (from, _) = self.gen_expr(&args[0], ctx)?;
                            let (to, _) = self.gen_expr(&args[1], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64* @tinox_array_slice(i64* {}, i64 {}, i64 {})", result, obj_ptr, from, to).unwrap();
                            return Ok((result, "i64*".to_string()));
                        }
                        "join" => {
                            let (sep, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i8* @tinox_string_join(i64* {}, i8* {})", result, obj_ptr, sep).unwrap();
                            return Ok((result, "i8*".to_string()));
                        }
                        _ => {}
                    }
                }

                // String method dispatch for split
                if obj_ty == "i8*" {
                    match method.as_str() {
                        "split" => {
                            let (delim, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64* @tinox_string_split(i8* {}, i8* {})", result, obj_ptr, delim).unwrap();
                            return Ok((result, "i64*".to_string()));
                        }
                        _ => {}
                    }
                }

                // Map method dispatch
                if declared_type.as_deref() == Some("Map") {
                    match method.as_str() {
                        "get" => {
                            let (key, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_map_get(i8* {}, i8* {})", result, obj_ptr, key).unwrap();
                            return Ok((result, "i64".to_string()));
                        }
                        "insert" => {
                            let (key, _) = self.gen_expr(&args[0], ctx)?;
                            let (val, _) = self.gen_expr(&args[1], ctx)?;
                            writeln!(&mut self.ir, "call void @tinox_map_set(i8* {}, i8* {}, i64 {})", obj_ptr, key, val).unwrap();
                            return Ok(("0".to_string(), "void".to_string()));
                        }
                        "contains" => {
                            let (key, _) = self.gen_expr(&args[0], ctx)?;
                            let raw = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_map_contains(i8* {}, i8* {})", raw, obj_ptr, key).unwrap();
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = icmp ne i64 {}, 0", result, raw).unwrap();
                            return Ok((result, "i1".to_string()));
                        }
                        "remove" => {
                            let (key, _) = self.gen_expr(&args[0], ctx)?;
                            writeln!(&mut self.ir, "call void @tinox_map_remove(i8* {}, i8* {})", obj_ptr, key).unwrap();
                            return Ok(("0".to_string(), "void".to_string()));
                        }
                        "len" => {
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_map_len(i8* {})", result, obj_ptr).unwrap();
                            return Ok((result, "i64".to_string()));
                        }
                        "keys" => {
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64* @tinox_map_keys(i8* {})", result, obj_ptr).unwrap();
                            return Ok((result, "i64*".to_string()));
                        }
                        "values" => {
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64* @tinox_map_values(i8* {})", result, obj_ptr).unwrap();
                            return Ok((result, "i64*".to_string()));
                        }
                        _ => {}
                    }
                }

                // File method dispatch
                if declared_type.as_deref() == Some("File") {
                    match method.as_str() {
                        "read" => {
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i8* @tinox_file_read(i8* {})", result, obj_ptr).unwrap();
                            return Ok((result, "i8*".to_string()));
                        }
                        "readLine" => {
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i8* @tinox_file_readline(i8* {})", result, obj_ptr).unwrap();
                            return Ok((result, "i8*".to_string()));
                        }
                        "write" => {
                            let (s, _) = self.gen_expr(&args[0], ctx)?;
                            writeln!(&mut self.ir, "call void @tinox_file_write(i8* {}, i8* {})", obj_ptr, s).unwrap();
                            return Ok(("0".to_string(), "void".to_string()));
                        }
                        "close" => {
                            writeln!(&mut self.ir, "call void @tinox_file_close(i8* {})", obj_ptr).unwrap();
                            return Ok(("0".to_string(), "void".to_string()));
                        }
                        "eof" => {
                            let raw = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_file_eof(i8* {})", raw, obj_ptr).unwrap();
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = icmp ne i64 {}, 0", result, raw).unwrap();
                            return Ok((result, "i1".to_string()));
                        }
                        _ => {}
                    }
                }

                // String method dispatch (obj_ty == "i8*")
                if obj_ty == "i8*" {
                    match method.as_str() {
                        "len" => {
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_string_length(i8* {})", result, obj_ptr).unwrap();
                            return Ok((result, "i64".to_string()));
                        }
                        "toUpper" | "toUpperCase" => {
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i8* @tinox_string_to_upper(i8* {})", result, obj_ptr).unwrap();
                            return Ok((result, "i8*".to_string()));
                        }
                        "toLower" | "toLowerCase" => {
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i8* @tinox_string_to_lower(i8* {})", result, obj_ptr).unwrap();
                            return Ok((result, "i8*".to_string()));
                        }
                        "trim" => {
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i8* @tinox_string_trim(i8* {})", result, obj_ptr).unwrap();
                            return Ok((result, "i8*".to_string()));
                        }
                        "contains" => {
                            let (arg, _) = self.gen_expr(&args[0], ctx)?;
                            let raw = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_string_contains(i8* {}, i8* {})", raw, obj_ptr, arg).unwrap();
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = icmp ne i64 {}, 0", result, raw).unwrap();
                            return Ok((result, "i1".to_string()));
                        }
                        "startsWith" => {
                            let (arg, _) = self.gen_expr(&args[0], ctx)?;
                            let raw = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_string_starts_with(i8* {}, i8* {})", raw, obj_ptr, arg).unwrap();
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = icmp ne i64 {}, 0", result, raw).unwrap();
                            return Ok((result, "i1".to_string()));
                        }
                        "endsWith" => {
                            let (arg, _) = self.gen_expr(&args[0], ctx)?;
                            let raw = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_string_ends_with(i8* {}, i8* {})", raw, obj_ptr, arg).unwrap();
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = icmp ne i64 {}, 0", result, raw).unwrap();
                            return Ok((result, "i1".to_string()));
                        }
                        "indexOf" => {
                            let (arg, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_string_index_of(i8* {}, i8* {})", result, obj_ptr, arg).unwrap();
                            return Ok((result, "i64".to_string()));
                        }
                        "charAt" => {
                            let (arg, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i8* @tinox_char_at(i8* {}, i64 {})", result, obj_ptr, arg).unwrap();
                            return Ok((result, "i8*".to_string()));
                        }
                        "charCodeAt" => {
                            let (idx, _) = self.gen_expr(&args[0], ctx)?;
                            let ptr = self.temp();
                            writeln!(&mut self.ir, "{} = getelementptr i8, ptr {}, i64 {}", ptr, obj_ptr, idx).unwrap();
                            let byte = self.temp();
                            writeln!(&mut self.ir, "{} = load i8, i8* {}", byte, ptr).unwrap();
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = zext i8 {} to i64", result, byte).unwrap();
                            return Ok((result, "i64".to_string()));
                        }
                        "substring" => {
                            let (from, _) = self.gen_expr(&args[0], ctx)?;
                            let (to, _) = self.gen_expr(&args[1], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i8* @tinox_string_substring(i8* {}, i64 {}, i64 {})", result, obj_ptr, from, to).unwrap();
                            return Ok((result, "i8*".to_string()));
                        }
                        "replace" => {
                            let (from, _) = self.gen_expr(&args[0], ctx)?;
                            let (to, _) = self.gen_expr(&args[1], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i8* @tinox_string_replace(i8* {}, i8* {}, i8* {})", result, obj_ptr, from, to).unwrap();
                            return Ok((result, "i8*".to_string()));
                        }
                        "toInt" => {
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_string_to_int(i8* {})", result, obj_ptr).unwrap();
                            return Ok((result, "i64".to_string()));
                        }
                        "toFloat" => {
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call double @tinox_string_to_float(i8* {})", result, obj_ptr).unwrap();
                            return Ok((result, "double".to_string()));
                        }
                        _ => {}
                    }
                }

                // Int/Float/Bool method dispatch (toString, charCodeAt, etc.)
                if obj_ty == "i64" || obj_ty == "double" || obj_ty == "i1" {
                    match method.as_str() {
                        "toString" => {
                            let result = self.temp();
                            let (fn_name, arg_ty) = match obj_ty.as_str() {
                                "double" => ("tinox_float_to_string", "double"),
                                "i1"     => ("tinox_bool_to_string", "i1"),
                                _        => ("tinox_int_to_string", "i64"),
                            };
                            writeln!(&mut self.ir, "{} = call i8* @{}({} {})", result, fn_name, arg_ty, obj_ptr).unwrap();
                            return Ok((result, "i8*".to_string()));
                        }
                        _ => {}
                    }
                }

                // Check if the declared type is an interface — if so, use vtable dispatch.
                let is_interface_dispatch = declared_type
                    .as_deref()
                    .map(|t| self.known_interfaces.contains(t))
                    .unwrap_or(false);

                // Evaluate extra arguments first (used in both paths).
                let mut extra_args: Vec<(String, String)> = Vec::new();
                for arg in args {
                    let (val, ty) = self.gen_expr(arg, ctx)?;
                    extra_args.push((val, ty));
                }

                let mut full_args_str = format!("{} {}", obj_ty, obj_ptr);
                for (val, ty) in &extra_args {
                    full_args_str.push_str(&format!(", {} {}", ty, val));
                }

                if is_interface_dispatch {
                    let iface_name = declared_type.as_deref().unwrap();

                    // Find the method slot index in the vtable.
                    let slot_idx = self
                        .vtable_layouts
                        .get(iface_name)
                        .and_then(|methods| methods.iter().position(|m| m == method))
                        .unwrap_or(0) as i64;

                    // Load vtable pointer from slot 0 of the object.
                    // The object is an i64* pointer; slot 0 holds the vtable address as i64.
                    let vtable_i64_ptr = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = getelementptr i64, ptr {}, i64 0",
                        vtable_i64_ptr, obj_ptr
                    )
                    .unwrap();
                    let vtable_i64 = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = load i64, i64* {}",
                        vtable_i64, vtable_i64_ptr
                    )
                    .unwrap();
                    // Cast the i64 vtable base address to i64*.
                    let vtable_ptr = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = inttoptr i64 {} to i64*",
                        vtable_ptr, vtable_i64
                    )
                    .unwrap();

                    // Load the function pointer at vtable[slot_idx].
                    let fn_slot_ptr = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = getelementptr i64, ptr {}, i64 {}",
                        fn_slot_ptr, vtable_ptr, slot_idx
                    )
                    .unwrap();
                    let fn_i64 = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = load i64, i64* {}",
                        fn_i64, fn_slot_ptr
                    )
                    .unwrap();

                    // Build the function type string based on args.
                    let ret_ty = "i64".to_string(); // vtable methods return i64 (uniform representation)
                    let mut param_types = vec!["i64*".to_string()]; // self
                    for (_, ty) in &extra_args {
                        param_types.push(ty.clone());
                    }
                    let param_types_str = param_types.join(", ");
                    let fn_type_str = format!("{} ({})*", ret_ty, param_types_str);

                    let casted_fn = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = inttoptr i64 {} to {}",
                        casted_fn, fn_i64, fn_type_str
                    )
                    .unwrap();

                    let result = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = call {} {}({})",
                        result, ret_ty, casted_fn, full_args_str
                    )
                    .unwrap();
                    Ok((result, ret_ty))
                } else if let Some(fn_sig) = declared_type.as_deref()
                    .and_then(|dt| self.fn_field_sigs.get(dt))
                    .and_then(|m| m.get(method.as_str()))
                    .cloned()
                {
                    // Fn-type field call: load i64, inttoptr to function pointer, call
                    let struct_name = declared_type.as_deref().unwrap();
                    let field_offset = self.struct_layouts.get(struct_name)
                        .and_then(|fields| fields.iter().position(|f| f == method))
                        .unwrap_or(0) as i64;
                    let obj_struct_ptr = if obj_ty == "i64" {
                        let cast = self.temp();
                        writeln!(&mut self.ir, "{} = inttoptr i64 {} to i64*", cast, obj_ptr).unwrap();
                        cast
                    } else {
                        obj_ptr.clone()
                    };
                    let field_gep = self.temp();
                    writeln!(&mut self.ir, "{} = getelementptr i64, ptr {}, i64 {}", field_gep, obj_struct_ptr, field_offset).unwrap();
                    let fp_i64 = self.temp();
                    writeln!(&mut self.ir, "{} = load i64, i64* {}", fp_i64, field_gep).unwrap();
                    let (ret_ty, param_tys) = fn_sig;
                    let fn_ptr_ty = format!("{} ({})*", ret_ty, param_tys.join(", "));
                    let fp = self.temp();
                    writeln!(&mut self.ir, "{} = inttoptr i64 {} to {}", fp, fp_i64, fn_ptr_ty).unwrap();
                    let mut call_args_str = String::new();
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 { call_args_str.push_str(", "); }
                        let (v, t) = self.gen_expr(arg, ctx)?;
                        call_args_str.push_str(&format!("{} {}", t, v));
                    }
                    let result = self.temp();
                    if ret_ty == "void" {
                        writeln!(&mut self.ir, "call void {}({})", fp, call_args_str).unwrap();
                        Ok((result, "void".to_string()))
                    } else {
                        writeln!(&mut self.ir, "{} = call {} {}({})", result, ret_ty, fp, call_args_str).unwrap();
                        Ok((result, ret_ty))
                    }
                } else {
                    // Direct (static) dispatch — resolve through inheritance chain.
                    let logical_name = if let Some(class) = declared_type {
                        format!("{}_{}", class, method)
                    } else {
                        method.clone()
                    };
                    let full_method_name = self
                        .method_impl
                        .get(&logical_name)
                        .cloned()
                        .unwrap_or(logical_name);

                    let ret_ty = self
                        .method_ret_types
                        .get(&full_method_name)
                        .cloned()
                        .unwrap_or_else(|| "i64".to_string());

                    let result = self.temp();
                    if ret_ty == "void" {
                        writeln!(&mut self.ir, "call void @{}({})", full_method_name, full_args_str).unwrap();
                    } else {
                        writeln!(&mut self.ir, "{} = call {} @{}({})", result, ret_ty, full_method_name, full_args_str).unwrap();
                    }
                    Ok((result, ret_ty))
                }
            }
            ExprKind::Index { obj, index } => {
                let arr_name = if let ExprKind::Ident(n) = &obj.node { Some(n.clone()) } else { None };
                let declared_elem_type = arr_name.as_ref().and_then(|n| ctx.local_types.get(n)).cloned();
                let is_str_arr = declared_elem_type.as_deref() == Some("Array:String");
                let is_map = declared_elem_type.as_deref() == Some("Map");

                let (idx_val, idx_ty) = self.gen_expr(index, ctx)?;
                let (base_ptr, base_ty) = if let ExprKind::Ident(name) = &obj.node {
                    if ctx.params.contains(name) {
                        self.gen_expr(obj, ctx)?
                    } else if ctx.locals.contains_key(name) {
                        let (var_ty, _) = ctx.locals.get(name).unwrap();
                        let loaded_ptr = self.temp();
                        writeln!(&mut self.ir, "{} = load {}, {}* %{}", loaded_ptr, var_ty, var_ty, name).unwrap();
                        (loaded_ptr, var_ty.clone())
                    } else {
                        self.gen_expr(obj, ctx)?
                    }
                } else {
                    self.gen_expr(obj, ctx)?
                };

                if is_map {
                    // Map[key] → tinox_map_get(i8* map, i8* key) -> i64
                    let map_i8 = if base_ty == "i8*" { base_ptr.clone() } else {
                        let c = self.temp();
                        writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, base_ptr).unwrap();
                        c
                    };
                    let key_i8 = if idx_ty == "i8*" { idx_val.clone() } else {
                        let c = self.temp();
                        writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, idx_val).unwrap();
                        c
                    };
                    let result = self.temp();
                    writeln!(&mut self.ir, "{} = call i64 @tinox_map_get(i8* {}, i8* {})", result, map_i8, key_i8).unwrap();
                    Ok((result, "i64".to_string()))
                } else if base_ty == "i8*" {
                    // String indexing → return byte as i64
                    let ptr_name = self.temp();
                    writeln!(&mut self.ir, "{} = getelementptr i8, ptr {}, i64 {}", ptr_name, base_ptr, idx_val).unwrap();
                    let byte = self.temp();
                    writeln!(&mut self.ir, "{} = load i8, i8* {}", byte, ptr_name).unwrap();
                    let extended = self.temp();
                    writeln!(&mut self.ir, "{} = zext i8 {} to i64", extended, byte).unwrap();
                    Ok((extended, "i64".to_string()))
                } else {
                    let ptr_name = self.temp();
                    writeln!(&mut self.ir, "{} = getelementptr i64, ptr {}, i64 {}", ptr_name, base_ptr, idx_val).unwrap();
                    let raw = self.temp();
                    writeln!(&mut self.ir, "{} = load i64, i64* {}", raw, ptr_name).unwrap();
                    if is_str_arr {
                        let str_ptr = self.temp();
                        writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", str_ptr, raw).unwrap();
                        Ok((str_ptr, "i8*".to_string()))
                    } else {
                        Ok((raw, "i64".to_string()))
                    }
                }
            }
            ExprKind::ArrayLiteral(elements) => {
                let n = elements.len();
                let raw = self.temp();
                writeln!(&mut self.ir, "{} = call i8* @tinox_alloc(i64 {})", raw, (n + 1) * 8).unwrap();
                let full_ptr = self.temp();
                writeln!(&mut self.ir, "{} = bitcast i8* {} to i64*", full_ptr, raw).unwrap();
                writeln!(&mut self.ir, "store i64 {}, i64* {}", n, full_ptr).unwrap();
                let data_ptr = self.temp();
                writeln!(&mut self.ir, "{} = getelementptr i64, ptr {}, i64 1", data_ptr, full_ptr).unwrap();
                for (i, elem) in elements.iter().enumerate() {
                    let (val, val_ty) = self.gen_expr(elem, ctx)?;
                    let store_val = if val_ty == "i8*" {
                        let cast = self.temp();
                        writeln!(&mut self.ir, "{} = ptrtoint i8* {} to i64", cast, val).unwrap();
                        cast
                    } else {
                        val
                    };
                    let elem_ptr = self.temp();
                    writeln!(&mut self.ir, "{} = getelementptr i64, ptr {}, i64 {}", elem_ptr, data_ptr, i).unwrap();
                    writeln!(&mut self.ir, "store i64 {}, i64* {}", store_val, elem_ptr).unwrap();
                }
                Ok((data_ptr, "i64*".to_string()))
            }
            ExprKind::MapLiteral(entries) => {
                let map_ptr = self.temp();
                writeln!(&mut self.ir, "{} = call i8* @tinox_map_create()", map_ptr).unwrap();
                for (key_expr, val_expr) in entries {
                    let (key_val, _) = self.gen_expr(key_expr, ctx)?;
                    let (val_val, _) = self.gen_expr(val_expr, ctx)?;
                    writeln!(&mut self.ir, "call void @tinox_map_set(i8* {}, i8* {}, i64 {})", map_ptr, key_val, val_val).unwrap();
                }
                Ok((map_ptr, "i8*".to_string()))
            }
            ExprKind::FieldAccess { obj, field } => {
                let (obj_raw, obj_ty) = self.gen_expr(obj, ctx)?;

                // Fields are stored as i64; if the loaded value is i64, restore it to a ptr
                let obj_ptr = if obj_ty == "i64" {
                    let cast = self.temp();
                    writeln!(&mut self.ir, "{} = inttoptr i64 {} to i64*", cast, obj_raw).unwrap();
                    cast
                } else {
                    obj_raw
                };

                // Find the struct type and field offset
                let struct_name = match &obj.node {
                    ExprKind::Ident(name) => ctx.local_types.get(name).cloned(),
                    ExprKind::This => ctx.current_struct.clone(),
                    _ => self.infer_struct_type(obj, ctx),
                };

                let (offset, field_llvm_ty) = if let Some(ref sname) = struct_name {
                    let off = self.struct_layouts.get(sname.as_str())
                        .and_then(|fields| fields.iter().position(|f| f == field))
                        .unwrap_or(0) as i64;
                    let fty = self.struct_field_llvm_types.get(sname.as_str())
                        .and_then(|m| m.get(field.as_str()))
                        .cloned()
                        .unwrap_or_else(|| "i64".to_string());
                    (off, fty)
                } else {
                    (0i64, "i64".to_string())
                };

                let field_ptr = self.temp();
                writeln!(
                    &mut self.ir,
                    "{} = getelementptr i64, ptr {}, i64 {}",
                    field_ptr, obj_ptr, offset
                )
                .unwrap();

                // Load the raw i64 value from the field
                let loaded = self.temp();
                writeln!(&mut self.ir, "{} = load i64, i64* {}", loaded, field_ptr).unwrap();

                // Restore the value from its uniform i64 storage representation
                if field_llvm_ty == "double" || field_llvm_ty == "float" {
                    let cast = self.temp();
                    writeln!(&mut self.ir, "{} = bitcast i64 {} to {}", cast, loaded, field_llvm_ty).unwrap();
                    Ok((cast, field_llvm_ty))
                } else if field_llvm_ty != "i64" && field_llvm_ty.ends_with('*') {
                    let cast = self.temp();
                    writeln!(&mut self.ir, "{} = inttoptr i64 {} to {}", cast, loaded, field_llvm_ty).unwrap();
                    Ok((cast, field_llvm_ty))
                } else {
                    Ok((loaded, "i64".to_string()))
                }
            }
            ExprKind::StructLiteral { name, fields } => {
                let ptr = self.temp();
                let layout = self.struct_layouts.get(name).cloned().unwrap_or_default();
                let size = layout.len() * 8;
                writeln!(
                    &mut self.ir,
                    "{} = call i8* @tinox_alloc(i64 {})",
                    ptr, size
                )
                .unwrap();
                let typed_ptr = self.temp();
                writeln!(&mut self.ir, "{} = bitcast i8* {} to i64*", typed_ptr, ptr).unwrap();

                // If this class has a vtable, store the vtable pointer at index 0.
                let has_vtable = self.classes_with_vtable.contains(name);
                if has_vtable {
                    let n_vtable = self.vtable_sizes.get(name).copied().unwrap_or(1);
                    let vtable_gep = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = getelementptr i64, ptr {}, i64 0",
                        vtable_gep, typed_ptr
                    )
                    .unwrap();
                    let vtable_as_i64 = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = ptrtoint [{} x i64]* @{}_vtable to i64",
                        vtable_as_i64, n_vtable, name
                    )
                    .unwrap();
                    writeln!(
                        &mut self.ir,
                        "store i64 {}, i64* {}",
                        vtable_as_i64, vtable_gep
                    )
                    .unwrap();
                }

                for (fname, value) in fields.iter() {
                    let (val, val_ty) = self.gen_expr(value, ctx)?;
                    // Look up field position in layout (which includes __vtable__ at 0 if vtable class)
                    let field_idx = layout.iter().position(|f| f == fname).unwrap_or(0);
                    let field_ptr = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = getelementptr i64, ptr {}, i64 {}",
                        field_ptr, typed_ptr, field_idx
                    )
                    .unwrap();
                    // Uniform i64 field storage: pointers → ptrtoint, floats → bitcast, i1 → zext, i64 → direct
                    let store_val = if val_ty == "i1" {
                        let cast = self.temp();
                        writeln!(&mut self.ir, "{} = zext i1 {} to i64", cast, val).unwrap();
                        cast
                    } else if val_ty == "double" || val_ty == "float" {
                        let cast = self.temp();
                        writeln!(&mut self.ir, "{} = bitcast {} {} to i64", cast, val_ty, val).unwrap();
                        cast
                    } else if val_ty != "i64" && !val_ty.is_empty() {
                        let cast = self.temp();
                        writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", cast, val_ty, val).unwrap();
                        cast
                    } else {
                        val
                    };
                    writeln!(&mut self.ir, "store i64 {}, i64* {}", store_val, field_ptr).unwrap();
                }
                // @Config: inject values from application.properties for annotated fields
                let cfg_fields: Vec<ConfigFieldInfo> = self.config_fields.iter()
                    .filter(|f| &f.class_name == name)
                    .cloned()
                    .collect();
                for cf in &cfg_fields {
                    if let Some(field_idx) = layout.iter().position(|f| f == &cf.field_name) {
                        let key_label = format!("str{}", self.strings.len());
                        self.strings.insert(key_label.clone(), cf.config_key.clone());
                        let key_len = cf.config_key.len() + 1;
                        let key_ptr = self.temp();
                        writeln!(&mut self.ir,
                            "{} = getelementptr [{} x i8], [{} x i8]* @{}, i64 0, i64 0",
                            key_ptr, key_len, key_len, key_label).unwrap();
                        let field_ptr = self.temp();
                        writeln!(&mut self.ir,
                            "{} = getelementptr i64, ptr {}, i64 {}",
                            field_ptr, typed_ptr, field_idx).unwrap();
                        match cf.field_llvm_type.as_str() {
                            "i8*" => {
                                let raw = self.temp();
                                writeln!(&mut self.ir,
                                    "{} = call i8* @tinox_config_get(i8* {})",
                                    raw, key_ptr).unwrap();
                                let as_i64 = self.temp();
                                writeln!(&mut self.ir, "{} = ptrtoint i8* {} to i64", as_i64, raw).unwrap();
                                writeln!(&mut self.ir, "store i64 {}, i64* {}", as_i64, field_ptr).unwrap();
                            }
                            "i1" => {
                                let raw = self.temp();
                                writeln!(&mut self.ir,
                                    "{} = call i64 @tinox_config_get_bool(i8* {})",
                                    raw, key_ptr).unwrap();
                                writeln!(&mut self.ir, "store i64 {}, i64* {}", raw, field_ptr).unwrap();
                            }
                            _ => {
                                let raw = self.temp();
                                writeln!(&mut self.ir,
                                    "{} = call i64 @tinox_config_get_int(i8* {})",
                                    raw, key_ptr).unwrap();
                                writeln!(&mut self.ir, "store i64 {}, i64* {}", raw, field_ptr).unwrap();
                            }
                        }
                    }
                }

                // @Log: auto-initialize the synthetic 'log' field with Logger::new(ClassName)
                if self.log_classes.contains(name) {
                    if let Some(log_idx) = layout.iter().position(|f| f == "log") {
                        let str_label = format!("str{}", self.strings.len());
                        self.strings.insert(str_label.clone(), name.clone());
                        let str_len = name.len() + 1;
                        let name_ptr = self.temp();
                        writeln!(&mut self.ir,
                            "{} = getelementptr [{} x i8], [{} x i8]* @{}, i64 0, i64 0",
                            name_ptr, str_len, str_len, str_label).unwrap();
                        let logger_raw = self.temp();
                        writeln!(&mut self.ir,
                            "{} = call i64* @Logger_new(i8* {})",
                            logger_raw, name_ptr).unwrap();
                        let log_as_i64 = self.temp();
                        writeln!(&mut self.ir, "{} = ptrtoint i64* {} to i64", log_as_i64, logger_raw).unwrap();
                        let log_field_ptr = self.temp();
                        writeln!(&mut self.ir,
                            "{} = getelementptr i64, ptr {}, i64 {}",
                            log_field_ptr, typed_ptr, log_idx).unwrap();
                        writeln!(&mut self.ir, "store i64 {}, i64* {}", log_as_i64, log_field_ptr).unwrap();
                    }
                }
                Ok((typed_ptr, "i64*".to_string()))
            }
            ExprKind::TupleIndex { tuple, index } => {
                let (raw, raw_ty) = self.gen_expr(tuple, ctx)?;
                // If inner expr returned a plain i64 (ptrtoint'd pointer), restore it to ptr
                let ptr = if raw_ty == "i64" {
                    let cast = self.temp();
                    writeln!(&mut self.ir, "{} = inttoptr i64 {} to i64*", cast, raw).unwrap();
                    cast
                } else {
                    raw
                };
                let field_ptr = self.temp();
                writeln!(&mut self.ir, "{} = getelementptr i64, ptr {}, i64 {}", field_ptr, ptr, index).unwrap();
                let val = self.temp();
                writeln!(&mut self.ir, "{} = load i64, i64* {}", val, field_ptr).unwrap();
                Ok((val, "i64".to_string()))
            }
            ExprKind::Tuple(exprs) => {
                let ptr = self.temp();
                let size = exprs.len() * 8;
                writeln!(&mut self.ir, "{} = call i8* @tinox_alloc(i64 {})", ptr, size).unwrap();
                let typed_ptr = self.temp();
                writeln!(&mut self.ir, "{} = bitcast i8* {} to i64*", typed_ptr, ptr).unwrap();
                for (i, expr) in exprs.iter().enumerate() {
                    let (val, val_ty) = self.gen_expr(expr, ctx)?;
                    // Pointer elements must be ptrtoint'd to i64 for uniform storage
                    let store_val = if val_ty != "i64" && val_ty != "i1" && val_ty != "double" && val_ty != "float" {
                        let cast = self.temp();
                        writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", cast, val_ty, val).unwrap();
                        cast
                    } else {
                        val
                    };
                    let field_ptr = self.temp();
                    writeln!(&mut self.ir, "{} = getelementptr i64, ptr {}, i64 {}", field_ptr, typed_ptr, i).unwrap();
                    writeln!(&mut self.ir, "store i64 {}, i64* {}", store_val, field_ptr).unwrap();
                }
                Ok((typed_ptr, "i64*".to_string()))
            }
            ExprKind::EnumValue {
                enum_name,
                variant,
                args,
            } => {
                // Special built-in constructors
                if enum_name == "Map" && variant == "new" {
                    let result = self.temp();
                    writeln!(&mut self.ir, "{} = call i8* @tinox_map_create()", result).unwrap();
                    return Ok((result, "i8*".to_string()));
                }

                // If this is actually a static method call (ClassName::method(args)), dispatch to it
                let static_key = format!("{}_{}", enum_name, variant);
                if let Some(ret_ty) = self.method_ret_types.get(&static_key).cloned() {
                    let mut args_str = String::new();
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 { args_str.push_str(", "); }
                        let (v, t) = self.gen_expr(arg, ctx)?;
                        args_str.push_str(&format!("{} {}", t, v));
                    }
                    let result = self.temp();
                    writeln!(&mut self.ir, "{} = call {} @{}({})", result, ret_ty, static_key, args_str).unwrap();
                    return Ok((result, ret_ty));
                }

                // For simplicity, we represent enum values as:
                // - For variants without args: just a discriminator integer
                // - For variants with args: allocate memory with discriminator + args

                if args.is_empty() {
                    // Simple enum variant without arguments
                    // Use variant hash as discriminator or a simple mapping
                    let discriminator = variant.chars().map(|c| c as i64).sum::<i64>();
                    Ok((format!("{}", discriminator), "i64".to_string()))
                } else {
                    // Enum variant with arguments
                    // Allocate memory: [discriminator, arg1, arg2, ...]
                    let ptr = self.temp();
                    let size = (args.len() + 1) * 8; // +1 for discriminator
                    writeln!(
                        &mut self.ir,
                        "{} = call i8* @tinox_alloc(i64 {})",
                        ptr, size
                    )
                    .unwrap();
                    let typed_ptr = self.temp();
                    writeln!(&mut self.ir, "{} = bitcast i8* {} to i64*", typed_ptr, ptr).unwrap();

                    // Store discriminator at index 0
                    let discriminator = variant.chars().map(|c| c as i64).sum::<i64>();
                    let disc_ptr = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = getelementptr i64, ptr {}, i64 0",
                        disc_ptr, typed_ptr
                    )
                    .unwrap();
                    writeln!(
                        &mut self.ir,
                        "store i64 {}, i64* {}",
                        discriminator, disc_ptr
                    )
                    .unwrap();

                    // Store arguments starting at index 1
                    for (i, arg) in args.iter().enumerate() {
                        let (val, _) = self.gen_expr(arg, ctx)?;
                        let arg_ptr = self.temp();
                        writeln!(
                            &mut self.ir,
                            "{} = getelementptr i64, ptr {}, i64 {}",
                            arg_ptr,
                            typed_ptr,
                            i + 1
                        )
                        .unwrap();
                        writeln!(&mut self.ir, "store i64 {}, i64* {}", val, arg_ptr).unwrap();
                    }
                    Ok((typed_ptr, "i64*".to_string()))
                }
            }
            ExprKind::Return(value) => {
                let stmts_to_run: Vec<_> = ctx
                    .defer_stack
                    .last()
                    .cloned()
                    .unwrap_or_default();
                for stmt in stmts_to_run.into_iter().rev() {
                    self.gen_stmt_body(&Box::new(stmt), ctx)?;
                }
                if let Some(scope) = ctx.defer_stack.last_mut() {
                    scope.clear();
                }
                if let Some(val_expr) = value {
                    let (val, ty) = self.gen_expr(val_expr, ctx)?;
                    let llvm_ty = Self::llvm_type_str(&ty);
                    writeln!(&mut self.ir, "ret {} {}", llvm_ty, val).unwrap();
                } else {
                    writeln!(&mut self.ir, "ret void").unwrap();
                }
                // Dead-code block so subsequent IR remains in a valid block.
                let dead_bb = self.new_bb("ret_dead");
                writeln!(&mut self.ir, "{}:", dead_bb).unwrap();
                Ok(("0".to_string(), "i64".to_string()))
            }
            ExprKind::Break => {
                if let Some(ref break_bb) = ctx.break_target.clone() {
                    writeln!(&mut self.ir, "br label %{}", break_bb).unwrap();
                }
                let dead_bb = self.new_bb("break_dead");
                writeln!(&mut self.ir, "{}:", dead_bb).unwrap();
                Ok(("0".to_string(), "i64".to_string()))
            }
            ExprKind::Continue => {
                if let Some(ref cont_bb) = ctx.continue_target.clone() {
                    writeln!(&mut self.ir, "br label %{}", cont_bb).unwrap();
                }
                let dead_bb = self.new_bb("cont_dead");
                writeln!(&mut self.ir, "{}:", dead_bb).unwrap();
                Ok(("0".to_string(), "i64".to_string()))
            }
            ExprKind::Cast { expr, ty } => {
                let (val, val_ty) = self.gen_expr(expr, ctx)?;
                let llvm_ty = Self::type_to_llvm(ty);
                if llvm_ty == val_ty {
                    return Ok((val, llvm_ty));
                }
                let result = self.temp();
                let src_float = Self::is_float(&val_ty);
                let dst_float = Self::is_float(&llvm_ty);
                if src_float && dst_float {
                    // float ↔ float (fptrunc: double→float, fpext: float→double)
                    let op = if val_ty == "double" { "fptrunc" } else { "fpext" };
                    writeln!(&mut self.ir, "{} = {} {} {} to {}", result, op, val_ty, val, llvm_ty).unwrap();
                } else if src_float {
                    // float → int: fptosi
                    writeln!(&mut self.ir, "{} = fptosi {} {} to {}", result, val_ty, val, llvm_ty).unwrap();
                } else if dst_float {
                    // int → float: sitofp
                    writeln!(&mut self.ir, "{} = sitofp {} {} to {}", result, val_ty, val, llvm_ty).unwrap();
                } else if val_ty == "i1" {
                    writeln!(&mut self.ir, "{} = zext i1 {} to {}", result, val, llvm_ty).unwrap();
                } else if val_ty.starts_with('i') && llvm_ty.starts_with('i') {
                    let val_bits: u32 = val_ty[1..].parse().unwrap_or(64);
                    let tgt_bits: u32 = llvm_ty[1..].parse().unwrap_or(64);
                    let op = if val_bits < tgt_bits { "sext" } else { "trunc" };
                    writeln!(&mut self.ir, "{} = {} {} {} to {}", result, op, val_ty, val, llvm_ty).unwrap();
                } else {
                    writeln!(&mut self.ir, "{} = bitcast {} {} to {}", result, val_ty, val, llvm_ty).unwrap();
                }
                Ok((result, llvm_ty))
            }
            ExprKind::Block(stmts) => {
                if stmts.is_empty() {
                    return Ok(("0".to_string(), "i64".to_string()));
                }
                let (last, rest) = stmts.split_last().unwrap();
                for stmt in rest {
                    self.gen_stmt_body(stmt, ctx)?;
                }
                if let StmtKind::Expr(e) = &last.node {
                    self.gen_expr(e, ctx)
                } else {
                    self.gen_stmt_body(last, ctx)?;
                    Ok(("0".to_string(), "i64".to_string()))
                }
            }
            ExprKind::New { class, type_args, args } => {
                // Resolve the effective class name, monomorphizing generic classes on demand.
                let effective_class = self.ensure_generic_class_specialization(class, type_args)?;
                let layout_clone = self.struct_layouts.get(&effective_class).cloned();
                let has_vtable = self.classes_with_vtable.contains(&effective_class);
                let ptr = self.temp();
                let size = if let Some(ref layout) = layout_clone {
                    layout.len() * 8
                } else {
                    8
                };
                writeln!(
                    &mut self.ir,
                    "{} = call i8* @tinox_alloc(i64 {})",
                    ptr, size
                )
                .unwrap();
                let typed_ptr = self.temp();
                writeln!(&mut self.ir, "{} = bitcast i8* {} to i64*", typed_ptr, ptr).unwrap();

                if has_vtable {
                    let n_vtable = self.vtable_sizes.get(&effective_class).copied().unwrap_or(1);
                    let vtable_gep = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = getelementptr i64, ptr {}, i64 0",
                        vtable_gep, typed_ptr
                    )
                    .unwrap();
                    let vtable_as_i64 = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = ptrtoint [{} x i64]* @{}_vtable to i64",
                        vtable_as_i64, n_vtable, effective_class
                    )
                    .unwrap();
                    writeln!(
                        &mut self.ir,
                        "store i64 {}, i64* {}",
                        vtable_as_i64, vtable_gep
                    )
                    .unwrap();
                }

                if let Some(ref layout) = layout_clone {
                    // For vtable classes, user args start at index 1 in layout
                    let field_start = if has_vtable { 1 } else { 0 };
                    for (arg_idx, arg) in args.iter().enumerate() {
                        let layout_idx = field_start + arg_idx;
                        if layout_idx < layout.len() {
                            let (val, val_ty) = self.gen_expr(arg, ctx)?;
                            let store_val = if val_ty == "i8*" {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = ptrtoint i8* {} to i64", c, val).unwrap();
                                c
                            } else {
                                val
                            };
                            let field_ptr = self.temp();
                            writeln!(
                                &mut self.ir,
                                "{} = getelementptr i64, ptr {}, i64 {}",
                                field_ptr, typed_ptr, layout_idx
                            )
                            .unwrap();
                            writeln!(&mut self.ir, "store i64 {}, i64* {}", store_val, field_ptr)
                                .unwrap();
                        }
                    }
                }
                Ok((typed_ptr, "i64*".to_string()))
            }
            ExprKind::Lambda {
                params,
                ret_type,
                body,
            } => self.gen_lambda(params, ret_type.as_ref(), body, ctx),
            ExprKind::Range {
                start,
                end,
                inclusive,
            } => {
                let (start_val, _) = self.gen_expr(start, ctx)?;
                let (end_val, _) = self.gen_expr(end, ctx)?;
                let ptr = self.temp();
                writeln!(&mut self.ir, "{} = call i8* @tinox_alloc(i64 16)", ptr).unwrap();
                let typed_ptr = self.temp();
                writeln!(&mut self.ir, "{} = bitcast i8* {} to i64*", typed_ptr, ptr).unwrap();
                let start_ptr = self.temp();
                writeln!(
                    &mut self.ir,
                    "{} = getelementptr i64, ptr {}, i64 0",
                    start_ptr, typed_ptr
                )
                .unwrap();
                writeln!(&mut self.ir, "store i64 {}, i64* {}", start_val, start_ptr).unwrap();
                let end_ptr = self.temp();
                writeln!(
                    &mut self.ir,
                    "{} = getelementptr i64, ptr {}, i64 1",
                    end_ptr, typed_ptr
                )
                .unwrap();
                let end_stored = if *inclusive {
                    let inc = self.temp();
                    writeln!(&mut self.ir, "{} = add i64 {}, 1", inc, end_val).unwrap();
                    inc
                } else {
                    end_val
                };
                writeln!(&mut self.ir, "store i64 {}, i64* {}", end_stored, end_ptr).unwrap();
                Ok((typed_ptr, "i64*".to_string()))
            }
            ExprKind::Match { expr, cases } => {
                let (val, val_ty) = self.gen_expr(expr, ctx)?;
                let merge_bb = self.new_bb("match_end");
                let mut last_result: (String, String) = ("0".to_string(), "i64".to_string());
                for case in cases {
                    match &case.pattern {
                        Pattern::Wildcard(_) => {
                            let (body_val, body_ty) = self.gen_expr(&case.body, ctx)?;
                            last_result = (body_val, body_ty);
                            writeln!(&mut self.ir, "br label %{}", merge_bb).unwrap();
                        }
                        Pattern::Literal(lit, _) => {
                            let (lit_val, _lit_ty) = self.gen_literal(lit)?;
                            let cmp = self.temp();
                            writeln!(
                                &mut self.ir,
                                "{} = icmp eq {} {}, {}",
                                cmp, val_ty, val, lit_val
                            )
                            .unwrap();
                            let case_bb = self.new_bb("match_case");
                            let next_bb = self.new_bb("match_next");
                            writeln!(
                                &mut self.ir,
                                "br i1 {}, label %{}, label %{}",
                                cmp, case_bb, next_bb
                            )
                            .unwrap();
                            writeln!(&mut self.ir, "{}:", case_bb).unwrap();
                            let (body_val, body_ty) = self.gen_expr(&case.body, ctx)?;
                            last_result = (body_val, body_ty);
                            writeln!(&mut self.ir, "br label %{}", merge_bb).unwrap();
                            writeln!(&mut self.ir, "{}:", next_bb).unwrap();
                        }
                        Pattern::Ident(name, _, _) if self.known_enum_variants.contains(name) => {
                            // Bare enum variant name (e.g. `North` instead of `Dir::North`)
                            let discriminator = name.chars().map(|c| c as i64).sum::<i64>();
                            let cmp = self.temp();
                            writeln!(
                                &mut self.ir,
                                "{} = icmp eq i64 {}, {}",
                                cmp, val, discriminator
                            )
                            .unwrap();
                            let case_bb = self.new_bb("match_case");
                            let next_bb = self.new_bb("match_next");
                            writeln!(
                                &mut self.ir,
                                "br i1 {}, label %{}, label %{}",
                                cmp, case_bb, next_bb
                            )
                            .unwrap();
                            writeln!(&mut self.ir, "{}:", case_bb).unwrap();
                            let (body_val, body_ty) = self.gen_expr(&case.body, ctx)?;
                            last_result = (body_val, body_ty);
                            writeln!(&mut self.ir, "br label %{}", merge_bb).unwrap();
                            writeln!(&mut self.ir, "{}:", next_bb).unwrap();
                        }
                        Pattern::Ident(name, _, _) => {
                            let llvm_ty = val_ty.clone();
                            ctx.locals
                                .insert(name.clone(), (llvm_ty.clone(), ctx.locals.len()));
                            writeln!(&mut self.ir, "%{} = alloca {}", name, llvm_ty).unwrap();
                            writeln!(
                                &mut self.ir,
                                "store {} {}, {}* %{}",
                                val_ty, val, llvm_ty, name
                            )
                            .unwrap();
                            let (body_val, body_ty) = self.gen_expr(&case.body, ctx)?;
                            last_result = (body_val, body_ty);
                            writeln!(&mut self.ir, "br label %{}", merge_bb).unwrap();
                        }
                        Pattern::EnumVariant { variant, args, .. } => {
                            // For enum variants, we need to:
                            // 1. Extract and compare the discriminator
                            // 2. If it matches, bind any pattern arguments

                            let discriminator = variant.chars().map(|c| c as i64).sum::<i64>();

                            if !args.is_empty() && val_ty.ends_with("*") {
                                // Load discriminator from the enum value
                                let disc_ptr = self.temp();
                                writeln!(
                                    &mut self.ir,
                                    "{} = getelementptr i64, ptr {}, i64 0",
                                    disc_ptr, val
                                )
                                .unwrap();
                                let loaded_disc = self.temp();
                                writeln!(
                                    &mut self.ir,
                                    "{} = load i64, i64* {}",
                                    loaded_disc, disc_ptr
                                )
                                .unwrap();

                                let cmp = self.temp();
                                writeln!(
                                    &mut self.ir,
                                    "{} = icmp eq i64 {}, {}",
                                    cmp, loaded_disc, discriminator
                                )
                                .unwrap();

                                let case_bb = self.new_bb("match_case");
                                let next_bb = self.new_bb("match_next");
                                writeln!(
                                    &mut self.ir,
                                    "br i1 {}, label %{}, label %{}",
                                    cmp, case_bb, next_bb
                                )
                                .unwrap();
                                writeln!(&mut self.ir, "{}:", case_bb).unwrap();

                                // Bind arguments if present
                                for (i, arg_pattern) in args.iter().enumerate() {
                                    if let Pattern::Ident(arg_name, _, _) = arg_pattern {
                                        let arg_ptr = self.temp();
                                        writeln!(
                                            &mut self.ir,
                                            "{} = getelementptr i64, ptr {}, i64 {}",
                                            arg_ptr,
                                            val,
                                            i + 1
                                        )
                                        .unwrap();
                                        let arg_val = self.temp();
                                        writeln!(
                                            &mut self.ir,
                                            "{} = load i64, i64* {}",
                                            arg_val, arg_ptr
                                        )
                                        .unwrap();
                                        ctx.locals.insert(
                                            arg_name.clone(),
                                            ("i64".to_string(), ctx.locals.len()),
                                        );
                                        writeln!(&mut self.ir, "%{} = alloca i64", arg_name)
                                            .unwrap();
                                        writeln!(
                                            &mut self.ir,
                                            "store i64 {}, i64* %{}",
                                            arg_val, arg_name
                                        )
                                        .unwrap();
                                    }
                                }

                                let (body_val, body_ty) = self.gen_expr(&case.body, ctx)?;
                                last_result = (body_val, body_ty);
                                writeln!(&mut self.ir, "br label %{}", merge_bb).unwrap();
                                writeln!(&mut self.ir, "{}:", next_bb).unwrap();
                            } else if val_ty == "i64" {
                                // Simple enum variant (no arguments)
                                let cmp = self.temp();
                                writeln!(
                                    &mut self.ir,
                                    "{} = icmp eq i64 {}, {}",
                                    cmp, val, discriminator
                                )
                                .unwrap();
                                let case_bb = self.new_bb("match_case");
                                let next_bb = self.new_bb("match_next");
                                writeln!(
                                    &mut self.ir,
                                    "br i1 {}, label %{}, label %{}",
                                    cmp, case_bb, next_bb
                                )
                                .unwrap();
                                writeln!(&mut self.ir, "{}:", case_bb).unwrap();
                                let (body_val, body_ty) = self.gen_expr(&case.body, ctx)?;
                                last_result = (body_val, body_ty);
                                writeln!(&mut self.ir, "br label %{}", merge_bb).unwrap();
                                writeln!(&mut self.ir, "{}:", next_bb).unwrap();
                            }
                        }
                        _ => {}
                    }
                }
                writeln!(&mut self.ir, "br label %{}", merge_bb).unwrap();
                writeln!(&mut self.ir, "{}:", merge_bb).unwrap();
                Ok(last_result)
            }
            ExprKind::This => {
                if ctx.params.contains("self") {
                    Ok(("%self".to_string(), "i64*".to_string()))
                } else {
                    let mut bag = ErrorBag::new();
                    bag.push(Error::new(expr.span, "'this' used outside of a method"));
                    Err(bag)
                }
            }
            ExprKind::SuperCall { method, args } => {
                // Static dispatch to parent's method: call ParentClass_method(%self, args...)
                let parent_class = ctx.current_struct
                    .as_ref()
                    .and_then(|class| self.class_parents.get(class).cloned())
                    .unwrap_or_else(|| "__unknown__".to_string());
                let full_method_name = format!("{}_{}", parent_class, method);

                // First arg is %self (the current self pointer)
                let mut full_args_str = "i64* %self".to_string();
                for arg in args {
                    let (val, ty) = self.gen_expr(arg, ctx)?;
                    full_args_str.push_str(&format!(", {} {}", ty, val));
                }

                let ret_ty = self
                    .method_ret_types
                    .get(&full_method_name)
                    .cloned()
                    .unwrap_or_else(|| "i64".to_string());

                let result = self.temp();
                writeln!(
                    &mut self.ir,
                    "{} = call {} @{}({})",
                    result, ret_ty, full_method_name, full_args_str
                )
                .unwrap();
                Ok((result, ret_ty))
            }
            ExprKind::Is { expr, .. } => {
                let (val, _val_ty) = self.gen_expr(expr, ctx)?;
                let result = self.temp();
                writeln!(&mut self.ir, "{} = icmp ne i64 {}, 0", result, val).unwrap();
                Ok((result, "i1".to_string()))
            }
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let result_slot = self.temp();
                writeln!(&mut self.ir, "{} = alloca i64", result_slot).unwrap();

                let (cond_val, _) = self.gen_expr(cond, ctx)?;
                let then_bb = self.new_bb("if_then");
                let else_bb = self.new_bb("if_else");
                let merge_bb = self.new_bb("if_merge");

                writeln!(
                    &mut self.ir,
                    "br i1 {}, label %{}, label %{}",
                    cond_val, then_bb, else_bb
                )
                .unwrap();

                writeln!(&mut self.ir, "{}:", then_bb).unwrap();
                let (then_val, _) = self.gen_expr(then_branch, ctx)?;
                writeln!(&mut self.ir, "store i64 {}, i64* {}", then_val, result_slot).unwrap();
                writeln!(&mut self.ir, "br label %{}", merge_bb).unwrap();

                writeln!(&mut self.ir, "{}:", else_bb).unwrap();
                if let Some(else_expr) = else_branch {
                    let (else_val, _) = self.gen_expr(else_expr, ctx)?;
                    writeln!(&mut self.ir, "store i64 {}, i64* {}", else_val, result_slot)
                        .unwrap();
                } else {
                    writeln!(&mut self.ir, "store i64 0, i64* {}", result_slot).unwrap();
                }
                writeln!(&mut self.ir, "br label %{}", merge_bb).unwrap();

                writeln!(&mut self.ir, "{}:", merge_bb).unwrap();
                let result = self.temp();
                writeln!(&mut self.ir, "{} = load i64, i64* {}", result, result_slot).unwrap();
                Ok((result, "i64".to_string()))
            }
            ExprKind::While { cond, body } => {
                let loop_bb = self.new_bb("while_cond");
                let body_bb = self.new_bb("while_body");
                let end_bb = self.new_bb("while_end");

                let old_break = ctx.break_target.take();
                let old_continue = ctx.continue_target.take();
                ctx.break_target = Some(end_bb.clone());
                ctx.continue_target = Some(loop_bb.clone());

                writeln!(&mut self.ir, "br label %{}", loop_bb).unwrap();
                writeln!(&mut self.ir, "{}:", loop_bb).unwrap();
                let (cond_val, _) = self.gen_expr(cond, ctx)?;
                writeln!(
                    &mut self.ir,
                    "br i1 {}, label %{}, label %{}",
                    cond_val, body_bb, end_bb
                )
                .unwrap();
                writeln!(&mut self.ir, "{}:", body_bb).unwrap();
                self.gen_expr(body, ctx)?;
                writeln!(&mut self.ir, "br label %{}", loop_bb).unwrap();
                writeln!(&mut self.ir, "{}:", end_bb).unwrap();

                ctx.break_target = old_break;
                ctx.continue_target = old_continue;
                Ok(("0".to_string(), "i64".to_string()))
            }
            ExprKind::For { var, iter, body } => {
                let (range_ptr, _) = self.gen_expr(iter, ctx)?;

                let start_gep = self.temp();
                writeln!(
                    &mut self.ir,
                    "{} = getelementptr i64, ptr {}, i64 0",
                    start_gep, range_ptr
                )
                .unwrap();
                let start_val = self.temp();
                writeln!(&mut self.ir, "{} = load i64, i64* {}", start_val, start_gep).unwrap();

                let end_gep = self.temp();
                writeln!(
                    &mut self.ir,
                    "{} = getelementptr i64, ptr {}, i64 1",
                    end_gep, range_ptr
                )
                .unwrap();
                let end_val = self.temp();
                writeln!(&mut self.ir, "{} = load i64, i64* {}", end_val, end_gep).unwrap();

                writeln!(&mut self.ir, "%{} = alloca i64", var).unwrap();
                writeln!(&mut self.ir, "store i64 {}, i64* %{}", start_val, var).unwrap();
                ctx.locals
                    .insert(var.clone(), ("i64".to_string(), ctx.locals.len()));

                let cond_bb = self.new_bb("for_cond");
                let body_bb = self.new_bb("for_body");
                let end_bb = self.new_bb("for_end");

                let old_break = ctx.break_target.take();
                let old_continue = ctx.continue_target.take();
                ctx.break_target = Some(end_bb.clone());
                ctx.continue_target = Some(cond_bb.clone());

                writeln!(&mut self.ir, "br label %{}", cond_bb).unwrap();
                writeln!(&mut self.ir, "{}:", cond_bb).unwrap();
                let cur_val = self.temp();
                writeln!(&mut self.ir, "{} = load i64, i64* %{}", cur_val, var).unwrap();
                let cmp = self.temp();
                writeln!(&mut self.ir, "{} = icmp slt i64 {}, {}", cmp, cur_val, end_val)
                    .unwrap();
                writeln!(
                    &mut self.ir,
                    "br i1 {}, label %{}, label %{}",
                    cmp, body_bb, end_bb
                )
                .unwrap();

                writeln!(&mut self.ir, "{}:", body_bb).unwrap();
                self.gen_expr(body, ctx)?;

                let loaded_inc = self.temp();
                writeln!(&mut self.ir, "{} = load i64, i64* %{}", loaded_inc, var).unwrap();
                let next_val = self.temp();
                writeln!(&mut self.ir, "{} = add i64 {}, 1", next_val, loaded_inc).unwrap();
                writeln!(&mut self.ir, "store i64 {}, i64* %{}", next_val, var).unwrap();
                writeln!(&mut self.ir, "br label %{}", cond_bb).unwrap();

                writeln!(&mut self.ir, "{}:", end_bb).unwrap();

                ctx.break_target = old_break;
                ctx.continue_target = old_continue;
                Ok(("0".to_string(), "i64".to_string()))
            }
            ExprKind::Loop { body } => {
                let loop_bb = self.new_bb("loop_body");
                let end_bb = self.new_bb("loop_end");

                let old_break = ctx.break_target.take();
                let old_continue = ctx.continue_target.take();
                ctx.break_target = Some(end_bb.clone());
                ctx.continue_target = Some(loop_bb.clone());

                writeln!(&mut self.ir, "br label %{}", loop_bb).unwrap();
                writeln!(&mut self.ir, "{}:", loop_bb).unwrap();
                self.gen_expr(body, ctx)?;
                writeln!(&mut self.ir, "br label %{}", loop_bb).unwrap();
                writeln!(&mut self.ir, "{}:", end_bb).unwrap();

                ctx.break_target = old_break;
                ctx.continue_target = old_continue;
                Ok(("0".to_string(), "i64".to_string()))
            }
            ExprKind::Spawn(inner) => {
                let (fn_name, args) = match &inner.node {
                    ExprKind::Call { func, args } => {
                        let name = match &func.node {
                            ExprKind::Ident(n) => n.clone(),
                            _ => {
                                let mut bag = ErrorBag::new();
                                bag.push(Error::new(inner.span, "spawn requires a direct function call".to_string()));
                                return Err(bag);
                            }
                        };
                        (name, args.clone())
                    }
                    _ => {
                        let mut bag = ErrorBag::new();
                        bag.push(Error::new(inner.span, "spawn requires a function call expression".to_string()));
                        return Err(bag);
                    }
                };

                let mut arg_vals: Vec<(String, String)> = Vec::new();
                for arg in &args {
                    let (v, t) = self.gen_expr(arg, ctx)?;
                    arg_vals.push((v, t));
                }

                let n_slots = arg_vals.len() + 1;
                let wrapper_id = self.spawn_counter;
                self.spawn_counter += 1;
                let wrapper_name = format!("__spawn_wrapper_{}", wrapper_id);

                let (ret_ty, param_tys) = self.fn_sigs.get(&fn_name).cloned().unwrap_or_else(|| {
                    let ptys = arg_vals.iter().map(|(_, t)| t.clone()).collect();
                    ("i64".to_string(), ptys)
                });

                // Allocate args array [n_slots x i64]
                let raw_ptr = self.temp();
                writeln!(&mut self.ir, "  {} = call i8* @tinox_alloc(i64 {})", raw_ptr, n_slots * 8).unwrap();
                let ap = self.temp();
                writeln!(&mut self.ir, "  {} = bitcast i8* {} to [{} x i64]*", ap, raw_ptr, n_slots).unwrap();

                // Store fn ptr at slot 0
                let fp_sig = format!("{} ({})*", ret_ty, param_tys.join(", "));
                let fp_i64 = self.temp();
                writeln!(&mut self.ir, "  {} = ptrtoint {} @{} to i64", fp_i64, fp_sig, fn_name).unwrap();
                let fp_slot = self.temp();
                writeln!(&mut self.ir, "  {} = getelementptr [{} x i64], [{} x i64]* {}, i64 0, i64 0", fp_slot, n_slots, n_slots, ap).unwrap();
                writeln!(&mut self.ir, "  store i64 {}, i64* {}", fp_i64, fp_slot).unwrap();

                // Store each arg coerced to i64
                let arg_vals_clone = arg_vals.clone();
                for (i, (val, ty)) in arg_vals_clone.iter().enumerate() {
                    let slot = self.temp();
                    writeln!(&mut self.ir, "  {} = getelementptr [{} x i64], [{} x i64]* {}, i64 0, i64 {}", slot, n_slots, n_slots, ap, i + 1).unwrap();
                    let i64_val = self.coerce_to_i64(val, ty);
                    writeln!(&mut self.ir, "  store i64 {}, i64* {}", i64_val, slot).unwrap();
                }

                // Call runtime spawn
                let task_ptr = self.temp();
                writeln!(&mut self.ir, "  {} = call i8* @tinox_task_spawn(i8* (i8*)* @{}, i8* {})", task_ptr, wrapper_name, raw_ptr).unwrap();
                let task_i64 = self.temp();
                writeln!(&mut self.ir, "  {} = ptrtoint i8* {} to i64", task_i64, task_ptr).unwrap();

                // Emit wrapper function into lambda_ir
                self.emit_spawn_wrapper(&wrapper_name, n_slots, &ret_ty, &param_tys);

                Ok((task_i64, "i64".to_string()))
            }
            ExprKind::Await(inner) => {
                let (handle_i64, _) = self.gen_expr(inner, ctx)?;
                let handle_ptr = self.temp();
                writeln!(&mut self.ir, "  {} = inttoptr i64 {} to i8*", handle_ptr, handle_i64).unwrap();
                let result = self.temp();
                writeln!(&mut self.ir, "  {} = call i64 @tinox_task_await(i8* {})", result, handle_ptr).unwrap();
                Ok((result, "i64".to_string()))
            }
            ExprKind::Channel => {
                let ch_ptr = self.temp();
                writeln!(&mut self.ir, "  {} = call i8* @tinox_channel_create()", ch_ptr).unwrap();
                let ch_i64 = self.temp();
                writeln!(&mut self.ir, "  {} = ptrtoint i8* {} to i64", ch_i64, ch_ptr).unwrap();
                Ok((ch_i64, "i64".to_string()))
            }
            ExprKind::Send { channel, value } => {
                let (ch_i64, _) = self.gen_expr(channel, ctx)?;
                let (val_raw, val_ty) = self.gen_expr(value, ctx)?;
                let ch_ptr = self.temp();
                writeln!(&mut self.ir, "  {} = inttoptr i64 {} to i8*", ch_ptr, ch_i64).unwrap();
                let val_i64 = self.coerce_to_i64(&val_raw, &val_ty);
                writeln!(&mut self.ir, "  call void @tinox_channel_send(i8* {}, i64 {})", ch_ptr, val_i64).unwrap();
                Ok(("0".to_string(), "void".to_string()))
            }
            ExprKind::Recv(inner) => {
                let (ch_i64, _) = self.gen_expr(inner, ctx)?;
                let ch_ptr = self.temp();
                writeln!(&mut self.ir, "  {} = inttoptr i64 {} to i8*", ch_ptr, ch_i64).unwrap();
                let result = self.temp();
                writeln!(&mut self.ir, "  {} = call i64 @tinox_channel_recv(i8* {})", result, ch_ptr).unwrap();
                Ok((result, "i64".to_string()))
            }
            ExprKind::CompoundAssign { op, target, value } => {
                self.gen_compound_assign(op, target, value, ctx)
            }
            ExprKind::Assign { target, value } => {
                let (val, val_ty) = self.gen_expr(value, ctx)?;
                if let ExprKind::FieldAccess { obj, field } = &target.node {
                    let (obj_raw, obj_ty) = self.gen_expr(obj, ctx)?;
                    let obj_ptr = if obj_ty == "i64" {
                        let cast = self.temp();
                        writeln!(&mut self.ir, "{} = inttoptr i64 {} to i64*", cast, obj_raw).unwrap();
                        cast
                    } else {
                        obj_raw
                    };
                    let struct_name = self.infer_struct_type(obj, ctx)
                        .or_else(|| if matches!(&obj.node, ExprKind::This) { ctx.current_struct.clone() } else { None });
                    let offset = struct_name.as_deref()
                        .and_then(|sn| self.struct_layouts.get(sn))
                        .and_then(|fields| fields.iter().position(|f| f == field.as_str()))
                        .unwrap_or(0) as i64;
                    let store_val = if val_ty == "double" || val_ty == "float" {
                        let cast = self.temp();
                        writeln!(&mut self.ir, "{} = bitcast {} {} to i64", cast, val_ty, val).unwrap();
                        cast
                    } else if val_ty != "i64" && val_ty != "i1" && !val_ty.is_empty() {
                        let cast = self.temp();
                        writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", cast, val_ty, val).unwrap();
                        cast
                    } else {
                        val.clone()
                    };
                    let field_ptr = self.temp();
                    writeln!(&mut self.ir, "{} = getelementptr i64, ptr {}, i64 {}", field_ptr, obj_ptr, offset).unwrap();
                    writeln!(&mut self.ir, "store i64 {}, i64* {}", store_val, field_ptr).unwrap();
                } else if let ExprKind::Ident(name) = &target.node {
                    let store_ty = ctx.locals.get(name).map(|(t, _)| t.clone()).unwrap_or_else(|| val_ty.clone());
                    writeln!(&mut self.ir, "store {} {}, {}* %{}", val_ty, val, store_ty, name).unwrap();
                }
                Ok((val, val_ty))
            }
            _ => {
                let mut bag = ErrorBag::new();
                bag.push(Error::new(
                    expr.span,
                    format!(
                        "codegen: unsupported expression kind '{}'",
                        expr_kind_name(&expr.node)
                    ),
                ));
                Err(bag)
            }
        }
    }

    fn gen_compound_assign(
        &mut self,
        op: &tinox_parser::CompoundOp,
        target: &tinox_parser::Expr,
        value: &tinox_parser::Expr,
        ctx: &mut GenCtx,
    ) -> Result<(String, String), ErrorBag> {
        match &target.node {
            ExprKind::Ident(name) => {
                if let Some((ty, _)) = ctx.locals.get(name) {
                    let ty = ty.clone();
                    let loaded = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = load {}, {}* %{}",
                        loaded.as_str(),
                        ty,
                        ty,
                        name.as_str()
                    )
                    .unwrap();
                    let (rhs, _) = self.gen_expr(value, ctx)?;
                    let result = self.temp();
                    match op {
                        tinox_parser::CompoundOp::Add => {
                            writeln!(&mut self.ir, "{} = add {} {}, {}", result, ty, loaded, rhs)
                                .unwrap();
                        }
                        tinox_parser::CompoundOp::Sub => {
                            writeln!(&mut self.ir, "{} = sub {} {}, {}", result, ty, loaded, rhs)
                                .unwrap();
                        }
                        tinox_parser::CompoundOp::Mul => {
                            writeln!(&mut self.ir, "{} = mul {} {}, {}", result, ty, loaded, rhs)
                                .unwrap();
                        }
                        tinox_parser::CompoundOp::Div => {
                            writeln!(&mut self.ir, "{} = sdiv {} {}, {}", result, ty, loaded, rhs)
                                .unwrap();
                        }
                        tinox_parser::CompoundOp::Mod => {
                            writeln!(&mut self.ir, "{} = srem {} {}, {}", result, ty, loaded, rhs)
                                .unwrap();
                        }
                        tinox_parser::CompoundOp::BitAnd => {
                            writeln!(&mut self.ir, "{} = and {} {}, {}", result, ty, loaded, rhs)
                                .unwrap();
                        }
                        tinox_parser::CompoundOp::BitOr => {
                            writeln!(&mut self.ir, "{} = or {} {}, {}", result, ty, loaded, rhs)
                                .unwrap();
                        }
                        tinox_parser::CompoundOp::BitXor => {
                            writeln!(&mut self.ir, "{} = xor {} {}, {}", result, ty, loaded, rhs)
                                .unwrap();
                        }
                        tinox_parser::CompoundOp::Shl => {
                            writeln!(&mut self.ir, "{} = shl {} {}, {}", result, ty, loaded, rhs)
                                .unwrap();
                        }
                        tinox_parser::CompoundOp::Shr => {
                            writeln!(&mut self.ir, "{} = lshr {} {}, {}", result, ty, loaded, rhs)
                                .unwrap();
                        }
                        tinox_parser::CompoundOp::ShrArith => {
                            writeln!(&mut self.ir, "{} = ashr {} {}, {}", result, ty, loaded, rhs)
                                .unwrap();
                        }
                    }
                    writeln!(&mut self.ir, "store {} {}, {}* %{}", ty, result, ty, name).unwrap();
                    return Ok((result, ty));
                }
            }
            ExprKind::Index { obj, index } => {
                let (idx_val, _) = self.gen_expr(index, ctx)?;
                let (base_ptr, _var_ty) = if let ExprKind::Ident(name) = &obj.node {
                    if ctx.locals.contains_key(name) {
                        let (vty, _) = ctx.locals.get(name).unwrap();
                        let loaded_ptr = self.temp();
                        writeln!(
                            &mut self.ir,
                            "{} = load {}, {}* %{}",
                            loaded_ptr, vty, vty, name
                        )
                        .unwrap();
                        (loaded_ptr, vty.clone())
                    } else {
                        return self.gen_expr(obj, ctx);
                    }
                } else {
                    return self.gen_expr(obj, ctx);
                };
                let ptr_name = self.temp();
                writeln!(
                    &mut self.ir,
                    "{} = getelementptr i64, ptr {}, i64 {}",
                    ptr_name, base_ptr, idx_val
                )
                .unwrap();
                let loaded = self.temp();
                writeln!(&mut self.ir, "{} = load i64, i64* {}", loaded, ptr_name).unwrap();
                let (rhs, _) = self.gen_expr(value, ctx)?;
                let result = self.temp();
                match op {
                    tinox_parser::CompoundOp::Add => {
                        writeln!(&mut self.ir, "{} = add i64 {}, {}", result, loaded, rhs).unwrap();
                    }
                    tinox_parser::CompoundOp::Sub => {
                        writeln!(&mut self.ir, "{} = sub i64 {}, {}", result, loaded, rhs).unwrap();
                    }
                    tinox_parser::CompoundOp::Mul => {
                        writeln!(&mut self.ir, "{} = mul i64 {}, {}", result, loaded, rhs).unwrap();
                    }
                    tinox_parser::CompoundOp::Div => {
                        writeln!(&mut self.ir, "{} = sdiv i64 {}, {}", result, loaded, rhs)
                            .unwrap();
                    }
                    tinox_parser::CompoundOp::Mod => {
                        writeln!(&mut self.ir, "{} = srem i64 {}, {}", result, loaded, rhs)
                            .unwrap();
                    }
                    tinox_parser::CompoundOp::BitAnd => {
                        writeln!(&mut self.ir, "{} = and i64 {}, {}", result, loaded, rhs).unwrap();
                    }
                    tinox_parser::CompoundOp::BitOr => {
                        writeln!(&mut self.ir, "{} = or i64 {}, {}", result, loaded, rhs).unwrap();
                    }
                    tinox_parser::CompoundOp::BitXor => {
                        writeln!(&mut self.ir, "{} = xor i64 {}, {}", result, loaded, rhs).unwrap();
                    }
                    tinox_parser::CompoundOp::Shl => {
                        writeln!(&mut self.ir, "{} = shl i64 {}, {}", result, loaded, rhs).unwrap();
                    }
                    tinox_parser::CompoundOp::Shr => {
                        writeln!(&mut self.ir, "{} = lshr i64 {}, {}", result, loaded, rhs)
                            .unwrap();
                    }
                    tinox_parser::CompoundOp::ShrArith => {
                        writeln!(&mut self.ir, "{} = ashr i64 {}, {}", result, loaded, rhs)
                            .unwrap();
                    }
                }
                writeln!(&mut self.ir, "store i64 {}, i64* {}", result, ptr_name).unwrap();
                return Ok((result, "i64".to_string()));
            }
            _ => {
                let mut bag = ErrorBag::new();
                bag.push(Error::new(
                    target.span,
                    "codegen: unsupported compound-assignment target",
                ));
                return Err(bag);
            }
        }
        Ok(("0".to_string(), "i64".to_string()))
    }

    fn gen_literal(&mut self, lit: &Literal) -> Result<(String, String), ErrorBag> {
        match lit {
            Literal::Integer(n) => Ok((format!("{}", n), "i64".to_string())),
            Literal::Float(f) => {
                let s = format!("{}", f);
                let val = if s.contains('.') || s.contains('e') || s.contains('E') {
                    s
                } else {
                    format!("{}.0", s)
                };
                Ok((val, "double".to_string()))
            }
            Literal::String(s) => {
                let name = format!("str{}", self.strings.len());
                self.strings.insert(name.clone(), s.clone());
                let len = s.len() + 1;
                let ptr = self.temp();
                writeln!(&mut self.ir, "{} = getelementptr [{} x i8], [{} x i8]* @{}, i64 0, i64 0", ptr, len, len, name).unwrap();
                Ok((ptr, "i8*".to_string()))
            }
            Literal::Bool(b) => Ok((if *b { "1" } else { "0" }.to_string(), "i1".to_string())),
            Literal::Char(c) => Ok((format!("{}", *c as i64), "i32".to_string())),
            Literal::Byte(b) => Ok((format!("{}", b), "i8".to_string())),
            Literal::Null => Ok(("0".to_string(), "i64".to_string())),
        }
    }

    fn gen_lambda(
        &mut self,
        params: &[tinox_parser::Param],
        ret_type: Option<&tinox_parser::Type>,
        body: &tinox_parser::Expr,
        ctx: &mut GenCtx,
    ) -> Result<(String, String), ErrorBag> {
        let lambda_id = self.temp_count;
        self.temp_count += 1;
        let fn_name = format!("__lambda_{}", lambda_id);
        let ret_ty = match ret_type {
            Some(t) => Self::type_to_llvm(t),
            None => "i64".to_string(),
        };
        let mut params_str = String::new();
        let mut fn_type_str = String::new();
        let mut lambda_ctx = GenCtx {
            locals: HashMap::new(),
            local_slots: HashMap::new(),
            range_vars: HashSet::new(),
            params: HashSet::new(),
            struct_fields: Vec::new(),
            current_struct: None,
            local_types: HashMap::new(),
            break_target: None,
            continue_target: None,
            error_catch: None,
            defer_stack: Vec::new(),
            in_defer_exec: false,
            ret_type: ret_ty.clone(),
        };
        for (i, p) in params.iter().enumerate() {
            if i > 0 {
                params_str.push_str(", ");
                fn_type_str.push_str(", ");
            }
            let llvm_ty = Self::type_to_llvm(&p.param_type);
            params_str.push_str(&format!("{} %{}", llvm_ty, p.name));
            fn_type_str.push_str(&llvm_ty);
            lambda_ctx
                .locals
                .insert(p.name.clone(), (llvm_ty.clone(), lambda_ctx.locals.len()));
            lambda_ctx.params.insert(p.name.clone());
        }
        let param_names: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();
        let free_vars = collect_free_vars(body, &param_names);
        let captured: Vec<(String, String)> = ctx
            .locals
            .iter()
            .filter(|(name, _)| free_vars.contains(*name))
            .map(|(n, (t, _))| (n.clone(), t.clone()))
            .collect();
        let env_ptr_name = if captured.is_empty() {
            None
        } else {
            let env_ptr = self.temp();
            writeln!(
                &mut self.ir,
                "{} = call i8* @tinox_alloc(i64 {})",
                env_ptr,
                captured.len() * 8
            )
            .unwrap();
            let env_typed = self.temp();
            writeln!(
                &mut self.ir,
                "{} = bitcast i8* {} to i64*",
                env_typed, env_ptr
            )
            .unwrap();
            for (i, (name, ty)) in captured.iter().enumerate() {
                if let Some((_, _slot)) = ctx.locals.get(name) {
                    let field_ptr = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = getelementptr i64, ptr {}, i64 {}",
                        field_ptr, env_typed, i
                    )
                    .unwrap();
                    let val = self.temp();
                    writeln!(&mut self.ir, "{} = load {}, {}* %{}", val, ty, ty, name).unwrap();
                    writeln!(&mut self.ir, "store {} {}, {}* {}", ty, val, ty, field_ptr).unwrap();
                }
            }
            Some(env_typed)
        };
        if let Some(ref env) = env_ptr_name {
            params_str.push_str(&format!(", i64* {}", env));
            fn_type_str.push_str(", i64*");
            let env_name = env.trim_start_matches('%');
            lambda_ctx
                .locals
                .insert(env_name.to_string(), ("i64*".to_string(), 0));
            lambda_ctx.params.insert(env_name.to_string());
        }
        let saved_ir = std::mem::take(&mut self.ir);
        let saved_lambda_ir = std::mem::take(&mut self.lambda_ir);
        let saved_temp = self.temp_count;
        writeln!(
            &mut self.ir,
            "define {} @{}({}) {{",
            ret_ty, fn_name, params_str
        )
        .unwrap();
        writeln!(&mut self.ir, "entry:").unwrap();
        if let Some(ref env) = env_ptr_name {
            for (i, (name, ty)) in captured.iter().enumerate() {
                writeln!(&mut self.ir, "%{} = alloca {}", name, ty).unwrap();
                let env_field = self.temp();
                writeln!(
                    &mut self.ir,
                    "{} = getelementptr i64, ptr {}, i64 {}",
                    env_field, env, i
                )
                .unwrap();
                let loaded = self.temp();
                writeln!(&mut self.ir, "{} = load i64, i64* {}", loaded, env_field).unwrap();
                writeln!(&mut self.ir, "store i64 {}, i64* %{}", loaded, name).unwrap();
                lambda_ctx
                    .locals
                    .insert(name.clone(), (ty.clone(), lambda_ctx.locals.len()));
            }
        }
        self.gen_stmt_body(
            &Spanned::new(StmtKind::Return(Some(body.clone())), Span::dummy()),
            &mut lambda_ctx,
        )?;
        let has_terminator = self.ir.lines().last().map_or(false, |l| {
            l.trim().starts_with("ret ") || l.trim().starts_with("br ")
        });
        if !has_terminator {
            writeln!(&mut self.ir, "ret {} 0", ret_ty).unwrap();
        }
        writeln!(&mut self.ir, "}}").unwrap();
        writeln!(&mut self.ir).unwrap();
        let lambda_body = std::mem::replace(&mut self.ir, saved_ir);
        let mut new_lambda_ir = saved_lambda_ir;
        new_lambda_ir.push_str(&lambda_body);
        self.lambda_ir = new_lambda_ir;
        self.temp_count = saved_temp;
        let ptr_name = self.temp();
        writeln!(
            &mut self.ir,
            "{} = ptrtoint {} ({})* @{} to i64",
            ptr_name, ret_ty, fn_type_str, fn_name
        )
        .unwrap();
        let closure_ptr_name = if let Some(ref env_ptr) = env_ptr_name {
            let closure_ptr = self.temp();
            let closure_ptr_int = self.temp();
            writeln!(
                &mut self.ir,
                "{} = call i8* @tinox_alloc(i64 16)",
                closure_ptr
            )
            .unwrap();
            writeln!(
                &mut self.ir,
                "{} = bitcast i8* {} to i64*",
                closure_ptr_int, closure_ptr
            )
            .unwrap();
            let fp_field = self.temp();
            writeln!(
                &mut self.ir,
                "{} = getelementptr i64, ptr {}, i64 0",
                fp_field, closure_ptr_int
            )
            .unwrap();
            writeln!(&mut self.ir, "store i64 {}, i64* {}", ptr_name, fp_field).unwrap();
            let env_field = self.temp();
            let _env_ptr_clean = env_ptr.trim_start_matches('%');
            writeln!(
                &mut self.ir,
                "{} = getelementptr i64, ptr {}, i64 1",
                env_field, closure_ptr_int
            )
            .unwrap();
            writeln!(&mut self.ir, "store i64* {}, i64* {}", env_ptr, env_field).unwrap();
            Some(closure_ptr_int)
        } else {
            None
        };
        if let Some(cptr) = closure_ptr_name {
            Ok((cptr, "i64*".to_string()))
        } else {
            Ok((ptr_name, "i64".to_string()))
        }
    }

    fn is_float(ty: &str) -> bool {
        ty == "float" || ty == "double"
    }

    fn llvm_type_str(ty: &str) -> String {
        ty.to_string()
    }

    fn type_to_llvm(ty: &Type) -> String {
        match ty {
            Type::Int8 => "i8".to_string(),
            Type::Int16 => "i16".to_string(),
            Type::Int32 => "i32".to_string(),
            Type::Int64 => "i64".to_string(),
            Type::UInt8 => "i8".to_string(),
            Type::UInt16 => "i16".to_string(),
            Type::UInt32 => "i32".to_string(),
            Type::UInt64 => "i64".to_string(),
            Type::Float32 => "float".to_string(),
            Type::Float64 => "double".to_string(),
            Type::Bool => "i1".to_string(),
            Type::Char => "i32".to_string(),
            Type::String => "i8*".to_string(),
            Type::Nothing => "void".to_string(),
            Type::Named(_) => "i64*".to_string(),
            Type::Generic { name, args } if name == "Array" => {
                args.first().map(|t| format!("{}*", Self::type_to_llvm(t))).unwrap_or_else(|| "i64*".to_string())
            }
            Type::Generic { .. } => "i64*".to_string(),
            Type::Ref(inner) => format!("{}*", Self::type_to_llvm(inner)),
            Type::Mutable(inner) => Self::type_to_llvm(inner),
            Type::Array(inner) => format!("{}*", Self::type_to_llvm(inner)),
            Type::Map(_, _) => "i8*".to_string(),
            Type::Tuple(_) => "i64*".to_string(),
            _ => "i64".to_string(),
        }
    }

    fn type_to_llvm_inst(&self, ty: &Type) -> String {
        if let Type::Named(name) = ty {
            if self.known_enum_types.contains(name) {
                return "i64".to_string();
            }
        }
        Self::type_to_llvm(ty)
    }

    fn temp(&mut self) -> String {
        let t = format!("%tmp.{}", self.temp_count);
        self.temp_count += 1;
        t
    }

    fn new_bb(&mut self, name: &str) -> String {
        format!("{}_{}", name, self.temp_count)
    }

    #[allow(dead_code)]
    fn get_field_offset(
        &mut self,
        _obj: &str,
        field: &str,
        _ctx: &mut GenCtx,
    ) -> Result<u64, ErrorBag> {
        let mut offset = 0u64;
        for f in _ctx.struct_fields.iter() {
            if f == field {
                return Ok(offset);
            }
            offset += 8;
        }
        Ok(0)
    }

    #[allow(dead_code)]
    fn get_struct_name_for_type(&self, _ty: &str) -> String {
        _ty.replace("*", "")
    }

    #[allow(dead_code)]
    fn get_struct_name_for_obj(&self, obj: &Expr, ctx: &GenCtx) -> Option<String> {
        if let ExprKind::Ident(name) = &obj.node {
            ctx.local_types.get(name).cloned()
        } else {
            None
        }
    }

    fn gen_try_stmt(
        &mut self,
        body: &Box<Stmt>,
        catches: &[CatchClause],
        finally: Option<&Stmt>,
        ctx: &mut GenCtx,
    ) -> Result<(), ErrorBag> {
        let error_var = format!("%__error_{}__", self.temp_count);
        let try_bb = self.new_bb("try");
        let catch_bb = self.new_bb("catch");
        let finally_bb = if finally.is_some() {
            Some(self.new_bb("finally"))
        } else {
            None
        };
        let end_bb = self.new_bb("try_end");

        let merge_target = finally_bb.as_deref().unwrap_or(&end_bb).to_string();

        writeln!(&mut self.ir, "{} = alloca i64", error_var).unwrap();
        writeln!(&mut self.ir, "store i64 0, i64* {}", error_var).unwrap();

        // --- try body ---
        writeln!(&mut self.ir, "br label %{}", try_bb).unwrap();
        writeln!(&mut self.ir, "{}:", try_bb).unwrap();
        let old_error_catch = ctx.error_catch.take();
        ctx.error_catch = Some((catch_bb.clone(), error_var.clone()));
        self.gen_stmt_body(body, ctx)?;
        ctx.error_catch = old_error_catch;
        let try_ok_bb = self.new_bb("try_ok");
        writeln!(&mut self.ir, "br label %{}", try_ok_bb).unwrap();
        writeln!(&mut self.ir, "{}:", try_ok_bb).unwrap();
        writeln!(&mut self.ir, "br label %{}", merge_target).unwrap();

        // --- catch blocks (chained) ---
        // Each catch clause gets its own labeled block; they are chained so that
        // control flows through all matching handlers. The dispatch block (catch_bb)
        // jumps into the first clause; each clause ends with an unreachable-guard
        // block that branches to the next clause (or merge_target after the last).
        if catches.is_empty() {
            writeln!(&mut self.ir, "{}:", catch_bb).unwrap();
            let catch_ok_bb = self.new_bb("catch_ok");
            writeln!(&mut self.ir, "{}:", catch_ok_bb).unwrap();
            writeln!(&mut self.ir, "br label %{}", merge_target).unwrap();
        } else {
            // Pre-allocate all per-clause block labels so we can forward-reference them.
            let clause_bbs: Vec<String> = (0..catches.len())
                .map(|i| self.new_bb(&format!("catch_{}", i)))
                .collect();

            // Dispatch: jump to first clause.
            writeln!(&mut self.ir, "{}:", catch_bb).unwrap();
            writeln!(&mut self.ir, "br label %{}", clause_bbs[0]).unwrap();

            for (i, catch) in catches.iter().enumerate() {
                let llvm_ty = Self::type_to_llvm(&catch.ty);
                let param_slot = ctx.locals.len();
                ctx.locals
                    .insert(catch.param.clone(), (llvm_ty.clone(), param_slot));

                writeln!(&mut self.ir, "{}:", clause_bbs[i]).unwrap();
                writeln!(&mut self.ir, "%{} = alloca {}", catch.param, llvm_ty).unwrap();
                let err_val = self.temp();
                writeln!(
                    &mut self.ir,
                    "{} = load i64, i64* {}",
                    err_val, error_var
                )
                .unwrap();
                // Cast the stored i64 back to the catch param's type
                let store_val = if llvm_ty != "i64" {
                    let cast_val = self.temp();
                    if Self::is_float(&llvm_ty) {
                        writeln!(&mut self.ir, "{} = bitcast i64 {} to {}", cast_val, err_val, llvm_ty).unwrap();
                    } else if llvm_ty.ends_with('*') {
                        writeln!(&mut self.ir, "{} = inttoptr i64 {} to {}", cast_val, err_val, llvm_ty).unwrap();
                    } else {
                        writeln!(&mut self.ir, "{} = trunc i64 {} to {}", cast_val, err_val, llvm_ty).unwrap();
                    }
                    cast_val
                } else {
                    err_val
                };
                writeln!(
                    &mut self.ir,
                    "store {} {}, {}* %{}",
                    llvm_ty, store_val, llvm_ty, catch.param
                )
                .unwrap();
                self.gen_stmt_body(&catch.body, ctx)?;

                let next = if i + 1 < clause_bbs.len() {
                    clause_bbs[i + 1].clone()
                } else {
                    merge_target.clone()
                };
                let guard_bb = self.new_bb(&format!("catch_{}_ok", i));
                writeln!(&mut self.ir, "br label %{}", guard_bb).unwrap();
                writeln!(&mut self.ir, "{}:", guard_bb).unwrap();
                writeln!(&mut self.ir, "br label %{}", next).unwrap();
            }
        }

        // --- finally block ---
        if let Some(fb) = &finally_bb {
            writeln!(&mut self.ir, "{}:", fb).unwrap();
            if let Some(finally_stmt) = finally {
                self.gen_stmt_body(finally_stmt, ctx)?;
            }
            let finally_ok_bb = self.new_bb("finally_ok");
            writeln!(&mut self.ir, "br label %{}", finally_ok_bb).unwrap();
            writeln!(&mut self.ir, "{}:", finally_ok_bb).unwrap();
            writeln!(&mut self.ir, "br label %{}", end_bb).unwrap();
        }

        writeln!(&mut self.ir, "{}:", end_bb).unwrap();
        Ok(())
    }

    fn gen_defer_scope(&mut self, ctx: &mut GenCtx) -> Result<(), ErrorBag> {
        if let Some(scope) = ctx.defer_stack.last().cloned() {
            let old_in_defer = ctx.in_defer_exec;
            ctx.in_defer_exec = true;
            for stmt in scope.into_iter().rev() {
                self.gen_stmt_body(&Box::new(stmt), ctx)?;
            }
            ctx.in_defer_exec = old_in_defer;
        }
        Ok(())
    }

    pub fn emit_llvm_ir(&self, path: &Path) -> Result<(), Error> {
        std::fs::write(path, &self.ir)
            .map_err(|e| Error::new(Span::dummy(), format!("Failed to write IR: {}", e)))
    }

    fn run_opt(&self, ir_path: &Path) -> Result<std::path::PathBuf, Error> {
        let bc_path = ir_path.with_extension("opt.bc");
        let output = std::process::Command::new("opt")
            .args(["-O3", "-o"])
            .arg(&bc_path)
            .arg(ir_path)
            .output()
            .map_err(|e| Error::new(Span::dummy(), format!("opt failed: {}", e)))?;

        if !output.status.success() {
            return Err(Error::new(
                Span::dummy(),
                format!("opt failed: {}", String::from_utf8_lossy(&output.stderr)),
            ));
        }
        Ok(bc_path)
    }

    pub fn write_asm(&self, ir_path: &Path, asm_path: &Path) -> Result<(), Error> {
        let bc_path = self.run_opt(ir_path)?;
        let output = std::process::Command::new("llc")
            .args(["-O3", "-march=x86-64", "-filetype=asm", "-o"])
            .arg(asm_path)
            .arg(&bc_path)
            .output()
            .map_err(|e| Error::new(Span::dummy(), format!("llc failed: {}", e)))?;

        if !output.status.success() {
            return Err(Error::new(
                Span::dummy(),
                format!("llc failed: {}", String::from_utf8_lossy(&output.stderr)),
            ));
        }
        Ok(())
    }

    pub fn write_obj(&self, ir_path: &Path, obj_path: &Path) -> Result<(), Error> {
        let bc_path = self.run_opt(ir_path)?;
        let output = std::process::Command::new("llc")
            .args(["-O3", "-march=x86-64", "-filetype=obj", "-o"])
            .arg(obj_path)
            .arg(&bc_path)
            .output()
            .map_err(|e| Error::new(Span::dummy(), format!("llc failed: {}", e)))?;

        if !output.status.success() {
            return Err(Error::new(
                Span::dummy(),
                format!("llc failed: {}", String::from_utf8_lossy(&output.stderr)),
            ));
        }
        Ok(())
    }

    /// Produce a mangled name like `identity__i64__double` for a generic instantiation.
    fn mangle_generic_name(name: &str, type_params: &[String], bindings: &HashMap<String, String>) -> String {
        let suffix: Vec<String> = type_params
            .iter()
            .map(|tp| {
                bindings.get(tp).cloned().unwrap_or_else(|| "i64".to_string())
                    .replace('*', "P")
                    .replace(' ', "_")
            })
            .collect();
        if suffix.is_empty() { name.to_string() } else { format!("{}__{}", name, suffix.join("__")) }
    }

    /// Resolve a parser Type using concrete LLVM type bindings for type parameters.
    fn type_to_llvm_with_bindings(ty: &tinox_parser::Type, bindings: &HashMap<String, String>) -> String {
        match ty {
            tinox_parser::Type::Named(n) => {
                if let Some(llvm) = bindings.get(n) { llvm.clone() }
                else { Self::type_to_llvm(ty) }
            }
            tinox_parser::Type::Generic { name, .. } => {
                if let Some(llvm) = bindings.get(name) { llvm.clone() }
                else { Self::type_to_llvm(ty) }
            }
            _ => Self::type_to_llvm(ty),
        }
    }

    /// Substitute type parameter names in a `Type` with concrete parser `Type`s.
    fn substitute_type(ty: &tinox_parser::Type, subst: &HashMap<String, tinox_parser::Type>) -> tinox_parser::Type {
        match ty {
            tinox_parser::Type::Named(n) => {
                subst.get(n).cloned().unwrap_or_else(|| ty.clone())
            }
            tinox_parser::Type::Generic { name, args } => {
                if let Some(concrete) = subst.get(name) {
                    concrete.clone()
                } else {
                    tinox_parser::Type::Generic {
                        name: name.clone(),
                        args: args.iter().map(|a| Self::substitute_type(a, subst)).collect(),
                    }
                }
            }
            tinox_parser::Type::Array(inner) => tinox_parser::Type::Array(Box::new(Self::substitute_type(inner, subst))),
            tinox_parser::Type::Ref(inner) => tinox_parser::Type::Ref(Box::new(Self::substitute_type(inner, subst))),
            tinox_parser::Type::Mutable(inner) => tinox_parser::Type::Mutable(Box::new(Self::substitute_type(inner, subst))),
            tinox_parser::Type::Fn { params, ret } => tinox_parser::Type::Fn {
                params: params.iter().map(|p| Self::substitute_type(p, subst)).collect(),
                ret: Box::new(Self::substitute_type(ret, subst)),
            },
            other => other.clone(),
        }
    }

    /// Create a monomorphic copy of a generic function with substituted types and a mangled name.
    fn substitute_fn(f: &tinox_parser::Function, mangled_name: &str, bindings: &HashMap<String, String>) -> tinox_parser::Function {
        // Build a Type substitution map: "T" -> Type::Int64 etc.
        let subst: HashMap<String, tinox_parser::Type> = bindings.iter().map(|(tp, llvm_ty)| {
            let concrete_type = Self::llvm_ty_to_parser_type(llvm_ty);
            (tp.clone(), concrete_type)
        }).collect();
        tinox_parser::Function {
            name: mangled_name.to_string(),
            type_params: vec![],
            params: f.params.iter().map(|p| tinox_parser::Param {
                name: p.name.clone(),
                param_type: Self::substitute_type(&p.param_type, &subst),
                span: p.span,
            }).collect(),
            ret_type: Self::substitute_type(&f.ret_type, &subst),
            body: f.body.clone(),
            span: f.span,
            is_async: f.is_async,
            doc: f.doc.clone(),
            annotations: vec![],
        }
    }

    /// Compute the mangled class name for a generic instantiation without emitting code.
    fn effective_class_name(&self, class: &str, type_args: &[tinox_parser::Type]) -> String {
        if type_args.is_empty() {
            return class.to_string();
        }
        if let Some(gc) = self.generic_classes.get(class) {
            let bindings: HashMap<String, String> = gc.type_params.iter()
                .zip(type_args.iter())
                .map(|(tp, ta)| (tp.clone(), Self::type_to_llvm(ta)))
                .collect();
            Self::mangle_generic_name(class, &gc.type_params, &bindings)
        } else {
            class.to_string()
        }
    }

    /// If `class` is a known generic class, monomorphize it with `type_args` and return the
    /// mangled name. Otherwise return the class name unchanged. Emits the specialized methods
    /// into `lambda_ir` the first time a given instantiation is requested.
    fn ensure_generic_class_specialization(
        &mut self,
        class: &str,
        type_args: &[tinox_parser::Type],
    ) -> Result<String, ErrorBag> {
        if type_args.is_empty() || !self.generic_classes.contains_key(class) {
            return Ok(class.to_string());
        }
        let gc = self.generic_classes.get(class).unwrap().clone();
        let bindings: HashMap<String, String> = gc.type_params.iter()
            .zip(type_args.iter())
            .map(|(tp, ta)| (tp.clone(), Self::type_to_llvm(ta)))
            .collect();
        let mangled = Self::mangle_generic_name(class, &gc.type_params, &bindings);
        if !self.generated_specializations.contains(&mangled) {
            self.generated_specializations.insert(mangled.clone());
            let specialized = Self::substitute_class(&gc, &mangled, &bindings);
            // Register struct layout (field names, in order)
            let fields: Vec<String> = specialized.fields.iter().map(|f| f.name.clone()).collect();
            self.struct_layouts.insert(mangled.clone(), fields);
            // Register method signatures for dispatch
            for method in &specialized.methods {
                let fn_name = format!("{}_{}", mangled, method.name);
                let ret_ty = Self::type_to_llvm(&method.ret_type);
                self.method_ret_types.insert(fn_name.clone(), ret_ty);
                self.method_impl.insert(fn_name.clone(), fn_name);
            }
            // Generate method IR into lambda_ir so it doesn't interrupt current function
            let saved_ir = std::mem::take(&mut self.ir);
            let saved_temp = self.temp_count;
            self.temp_count = 0;
            for method in &specialized.methods {
                self.gen_class_method(&mangled, method)?;
            }
            let spec_ir = std::mem::take(&mut self.ir);
            self.ir = saved_ir;
            self.temp_count = saved_temp;
            self.lambda_ir.push_str(&spec_ir);
        }
        Ok(mangled)
    }

    /// Create a monomorphic copy of a generic class with substituted types and a mangled name.
    fn substitute_class(
        c: &tinox_parser::Class,
        mangled_name: &str,
        bindings: &HashMap<String, String>,
    ) -> tinox_parser::Class {
        let subst: HashMap<String, tinox_parser::Type> = bindings.iter()
            .map(|(tp, llvm_ty)| (tp.clone(), Self::llvm_ty_to_parser_type(llvm_ty)))
            .collect();
        tinox_parser::Class {
            name: mangled_name.to_string(),
            type_params: vec![],
            extends: c.extends.clone(),
            implements: c.implements.clone(),
            fields: c.fields.iter().map(|f| tinox_parser::FieldDef {
                name: f.name.clone(),
                field_type: Self::substitute_type(&f.field_type, &subst),
                visibility: f.visibility.clone(),
                mutable: f.mutable,
                span: f.span,
                doc: f.doc.clone(),
                annotations: vec![],
            }).collect(),
            methods: c.methods.iter().map(|m| tinox_parser::Method {
                name: m.name.clone(),
                type_params: m.type_params.clone(),
                params: m.params.iter().map(|p| tinox_parser::Param {
                    name: p.name.clone(),
                    param_type: Self::substitute_type(&p.param_type, &subst),
                    span: p.span,
                }).collect(),
                ret_type: Self::substitute_type(&m.ret_type, &subst),
                body: m.body.clone(),
                static_: m.static_,
                visibility: m.visibility.clone(),
                span: m.span,
                is_async: m.is_async,
                doc: m.doc.clone(),
                annotations: vec![],
            }).collect(),
            span: c.span,
            doc: c.doc.clone(),
            annotations: vec![],
        }
    }

    /// Best-effort mapping from an LLVM type string back to a parser Type (for substitution).
    fn llvm_ty_to_parser_type(llvm_ty: &str) -> tinox_parser::Type {
        match llvm_ty {
            "i64" => tinox_parser::Type::Int64,
            "i32" => tinox_parser::Type::Int32,
            "i16" => tinox_parser::Type::Int16,
            "i8" => tinox_parser::Type::Int8,
            "double" => tinox_parser::Type::Float64,
            "float" => tinox_parser::Type::Float32,
            "i1" => tinox_parser::Type::Bool,
            "i8*" => tinox_parser::Type::String,
            "void" => tinox_parser::Type::Nothing,
            other if other.ends_with('*') => {
                let inner = &other[..other.len() - 1];
                tinox_parser::Type::Ref(Box::new(Self::llvm_ty_to_parser_type(inner)))
            }
            other => tinox_parser::Type::Named(other.to_string()),
        }
    }

    /// Coerce an LLVM value of the given type to i64, emitting cast instructions as needed.
    fn coerce_to_i64(&mut self, val: &str, ty: &str) -> String {
        if ty == "i64" {
            val.to_string()
        } else if ty == "double" {
            let t = self.temp();
            writeln!(&mut self.ir, "  {} = bitcast double {} to i64", t, val).unwrap();
            t
        } else if ty == "i1" {
            let t = self.temp();
            writeln!(&mut self.ir, "  {} = zext i1 {} to i64", t, val).unwrap();
            t
        } else if ty.ends_with('*') {
            let t = self.temp();
            writeln!(&mut self.ir, "  {} = ptrtoint {} {} to i64", t, ty, val).unwrap();
            t
        } else {
            val.to_string()
        }
    }

    /// Emit a spawn wrapper function into lambda_ir.
    /// The wrapper has signature `i8* @name(i8* %raw)` and unpacks n_slots-1 args
    /// from the flat [n_slots x i64] array (slot 0 = fn ptr).
    fn emit_spawn_wrapper(&mut self, name: &str, n_slots: usize, ret_ty: &str, param_tys: &[String]) {
        let mut w = String::new();
        let mut tc = 0usize;
        macro_rules! wt {
            () => {{ tc += 1; format!("%w{}", tc) }};
        }

        writeln!(&mut w, "define i8* @{}(i8* %raw) {{", name).unwrap();
        writeln!(&mut w, "entry:").unwrap();

        let ap = wt!();
        writeln!(&mut w, "  {} = bitcast i8* %raw to [{} x i64]*", ap, n_slots).unwrap();

        // Load fn ptr from slot 0
        let fp_slot = wt!();
        writeln!(&mut w, "  {} = getelementptr [{} x i64], [{} x i64]* {}, i64 0, i64 0", fp_slot, n_slots, n_slots, ap).unwrap();
        let fp_i64 = wt!();
        writeln!(&mut w, "  {} = load i64, i64* {}", fp_i64, fp_slot).unwrap();
        let fn_type_str = format!("{} ({})*", ret_ty, param_tys.join(", "));
        let fp_typed = wt!();
        writeln!(&mut w, "  {} = inttoptr i64 {} to {}", fp_typed, fp_i64, fn_type_str).unwrap();

        // Load and cast each arg
        let mut call_args: Vec<String> = Vec::new();
        for (i, param_ty) in param_tys.iter().enumerate() {
            let slot = wt!();
            writeln!(&mut w, "  {} = getelementptr [{} x i64], [{} x i64]* {}, i64 0, i64 {}", slot, n_slots, n_slots, ap, i + 1).unwrap();
            let raw = wt!();
            writeln!(&mut w, "  {} = load i64, i64* {}", raw, slot).unwrap();
            let typed = if param_ty == "i64" {
                raw
            } else if param_ty == "double" {
                let t = wt!();
                writeln!(&mut w, "  {} = bitcast i64 {} to double", t, raw).unwrap();
                t
            } else if param_ty == "i1" {
                let t = wt!();
                writeln!(&mut w, "  {} = trunc i64 {} to i1", t, raw).unwrap();
                t
            } else if param_ty.ends_with('*') {
                let t = wt!();
                writeln!(&mut w, "  {} = inttoptr i64 {} to {}", t, raw, param_ty).unwrap();
                t
            } else {
                raw
            };
            call_args.push(format!("{} {}", param_ty, typed));
        }

        // Call the function and return result as i8*
        let call_str = call_args.join(", ");
        if ret_ty == "void" {
            writeln!(&mut w, "  call void {}({})", fp_typed, call_str).unwrap();
            writeln!(&mut w, "  ret i8* null").unwrap();
        } else {
            let res = wt!();
            writeln!(&mut w, "  {} = call {} {}({})", res, ret_ty, fp_typed, call_str).unwrap();
            let ret_ptr = wt!();
            if ret_ty == "i64" {
                writeln!(&mut w, "  {} = inttoptr i64 {} to i8*", ret_ptr, res).unwrap();
            } else if ret_ty == "double" {
                let as_i64 = wt!();
                writeln!(&mut w, "  {} = bitcast double {} to i64", as_i64, res).unwrap();
                writeln!(&mut w, "  {} = inttoptr i64 {} to i8*", ret_ptr, as_i64).unwrap();
            } else if ret_ty == "i1" {
                let as_i64 = wt!();
                writeln!(&mut w, "  {} = zext i1 {} to i64", as_i64, res).unwrap();
                writeln!(&mut w, "  {} = inttoptr i64 {} to i8*", ret_ptr, as_i64).unwrap();
            } else if ret_ty.ends_with('*') {
                writeln!(&mut w, "  {} = bitcast {} {} to i8*", ret_ptr, ret_ty, res).unwrap();
            } else {
                writeln!(&mut w, "  {} = inttoptr i64 {} to i8*", ret_ptr, res).unwrap();
            }
            writeln!(&mut w, "  ret i8* {}", ret_ptr).unwrap();
        }

        writeln!(&mut w, "}}").unwrap();
        writeln!(&mut w).unwrap();
        self.lambda_ir.push_str(&w);
    }
}

fn expr_kind_name(kind: &ExprKind) -> &'static str {
    match kind {
        ExprKind::Literal(_) => "Literal",
        ExprKind::ArrayLiteral(_) => "ArrayLiteral",
        ExprKind::MapLiteral(_) => "MapLiteral",
        ExprKind::Ident(_) => "Ident",
        ExprKind::Binary { .. } => "Binary",
        ExprKind::Unary { .. } => "Unary",
        ExprKind::Call { .. } => "Call",
        ExprKind::MethodCall { .. } => "MethodCall",
        ExprKind::Index { .. } => "Index",
        ExprKind::FieldAccess { .. } => "FieldAccess",
        ExprKind::This => "This",
        ExprKind::SuperCall { .. } => "SuperCall",
        ExprKind::New { .. } => "New",
        ExprKind::StructLiteral { .. } => "StructLiteral",
        ExprKind::Block(_) => "Block",
        ExprKind::If { .. } => "If",
        ExprKind::While { .. } => "While",
        ExprKind::For { .. } => "For",
        ExprKind::Loop { .. } => "Loop",
        ExprKind::Match { .. } => "Match",
        ExprKind::Return(_) => "Return",
        ExprKind::Break => "Break",
        ExprKind::Continue => "Continue",
        ExprKind::Throw(_) => "Throw",
        ExprKind::Try { .. } => "Try",
        ExprKind::Assign { .. } => "Assign",
        ExprKind::CompoundAssign { .. } => "CompoundAssign",
        ExprKind::Lambda { .. } => "Lambda",
        ExprKind::Spawn(_) => "Spawn",
        ExprKind::Await(_) => "Await",
        ExprKind::Channel => "Channel",
        ExprKind::Send { .. } => "Send",
        ExprKind::Recv(_) => "Recv",
        ExprKind::Cast { .. } => "Cast",
        ExprKind::Is { .. } => "Is",
        ExprKind::Range { .. } => "Range",
        ExprKind::Tuple(_) => "Tuple",
        ExprKind::TupleIndex { .. } => "TupleIndex",
        ExprKind::EnumValue { .. } => "EnumValue",
    }
}

fn collect_free_vars(expr: &Expr, param_names: &HashSet<String>) -> HashSet<String> {
    let mut vars = HashSet::new();
    collect_free_vars_inner(expr, param_names, &mut vars);
    vars
}

fn collect_free_vars_inner(expr: &Expr, param_names: &HashSet<String>, vars: &mut HashSet<String>) {
    match &expr.node {
        ExprKind::Ident(name) => {
            if !param_names.contains(name) {
                vars.insert(name.clone());
            }
        }
        ExprKind::Binary { op: _, lhs, rhs } => {
            collect_free_vars_inner(lhs, param_names, vars);
            collect_free_vars_inner(rhs, param_names, vars);
        }
        ExprKind::Unary { op: _, operand } => {
            collect_free_vars_inner(operand, param_names, vars);
        }
        ExprKind::Call { func, args } => {
            collect_free_vars_inner(func, param_names, vars);
            for arg in args {
                collect_free_vars_inner(arg, param_names, vars);
            }
        }
        ExprKind::MethodCall {
            obj,
            method: _,
            args,
        } => {
            collect_free_vars_inner(obj, param_names, vars);
            for arg in args {
                collect_free_vars_inner(arg, param_names, vars);
            }
        }
        ExprKind::Index { obj, index } => {
            collect_free_vars_inner(obj, param_names, vars);
            collect_free_vars_inner(index, param_names, vars);
        }
        ExprKind::ArrayLiteral(exprs) => {
            for e in exprs {
                collect_free_vars_inner(e, param_names, vars);
            }
        }
        ExprKind::FieldAccess { obj, field: _ } => {
            collect_free_vars_inner(obj, param_names, vars);
        }
        ExprKind::StructLiteral { fields, .. } => {
            for (_, val) in fields {
                collect_free_vars_inner(val, param_names, vars);
            }
        }
        ExprKind::Tuple(exprs) => {
            for e in exprs {
                collect_free_vars_inner(e, param_names, vars);
            }
        }
        ExprKind::TupleIndex { tuple, .. } => {
            collect_free_vars_inner(tuple, param_names, vars);
        }
        ExprKind::Cast { expr, ty: _ } => {
            collect_free_vars_inner(expr, param_names, vars);
        }
        ExprKind::Block(stmts) => {
            for stmt in stmts {
                match &stmt.node {
                    StmtKind::Expr(e) => collect_free_vars_inner(e, param_names, vars),
                    StmtKind::Return(Some(e)) => collect_free_vars_inner(e, param_names, vars),
                    StmtKind::Let { value: Some(e), .. } => collect_free_vars_inner(e, param_names, vars),
                    StmtKind::Var { value: Some(e), .. } => collect_free_vars_inner(e, param_names, vars),
                    StmtKind::If { cond, .. } => {
                        collect_free_vars_inner(cond, param_names, vars);
                    }
                    _ => {}
                }
            }
        }
        ExprKind::Range {
            start,
            end,
            inclusive: _,
        } => {
            collect_free_vars_inner(start, param_names, vars);
            collect_free_vars_inner(end, param_names, vars);
        }
        ExprKind::Match { expr, cases } => {
            collect_free_vars_inner(expr, param_names, vars);
            for case in cases {
                collect_free_vars_inner(&case.body, param_names, vars);
            }
        }
        ExprKind::Lambda { params, body, .. } => {
            let mut lambda_params = param_names.clone();
            for p in params {
                lambda_params.insert(p.name.clone());
            }
            collect_free_vars_inner(body, &lambda_params, vars);
        }
        ExprKind::This | ExprKind::SuperCall { .. } | ExprKind::New { .. } | ExprKind::Is { .. } => {}
        ExprKind::Literal(_) => {}
        _ => {}
    }
}

pub struct GenCtx {
    locals: HashMap<String, (String, usize)>,
    /// Maps user variable name → unique LLVM alloca slot name (without %)
    local_slots: HashMap<String, String>,
    /// Variables that hold a range value (i64* with start/end, not an array)
    range_vars: HashSet<String>,
    params: HashSet<String>,
    #[allow(dead_code)]
    struct_fields: Vec<String>,
    current_struct: Option<String>,
    local_types: HashMap<String, String>,
    break_target: Option<String>,
    continue_target: Option<String>,
    error_catch: Option<(String, String)>,
    defer_stack: Vec<Vec<Stmt>>,
    in_defer_exec: bool,
    /// LLVM return type of the current function (for casting return values)
    ret_type: String,
}

pub fn gen(source: &SourceFile) -> Result<CodeGen, ErrorBag> {
    let mut codegen = CodeGen::new();
    codegen.gen(source)?;
    Ok(codegen)
}

impl Default for CodeGen {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tinox_lexer::Lexer;
    use tinox_parser::Parser;

    fn compile_to_ir(src: &str) -> String {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().expect("lex failed");
        let mut parser = Parser::new(tokens);
        let ast = parser.parse().expect("parse failed");
        let mut cg = CodeGen::new();
        cg.gen(&ast).expect("codegen failed");
        cg.into_ir()
    }

    #[test]
    fn test_if_expr() {
        let src = "fn main() -> Int64 {\n  let x = if true { 42; } else { 0; };\n  return x;\n}";
        let ir = compile_to_ir(src);
        assert!(ir.contains("if_then"), "should have if_then block");
        assert!(ir.contains("if_merge"), "should have if_merge block");
    }

    #[test]
    fn test_block_expr_returns_last() {
        let src = "fn main() -> Int64 {\n  let x = { let a = 10; a; };\n  return x;\n}";
        let ir = compile_to_ir(src);
        assert!(ir.contains("alloca"), "should have allocas");
    }

    #[test]
    fn test_float_ops() {
        let src = "namespace math { class Ops { fnc add_floats(a: Float64, b: Float64) -> Float64 { return a + b; } } }";
        let ir = compile_to_ir(src);
        assert!(ir.contains("fadd double"), "should use fadd for float addition");
    }

    #[test]
    fn test_try_catch() {
        // throw followed by semicolon — parser requires this
        let src = concat!(
            "fn main() -> Int64 {\n",
            "  try {\n",
            "    println(1);\n",
            "  } catch (e: Int64) {\n",
            "    println(e);\n",
            "  };\n",
            "  return 0;\n",
            "}"
        );
        let ir = compile_to_ir(src);
        assert!(ir.contains("try_"), "should have try block");
        assert!(ir.contains("catch_"), "should have catch block");
        assert!(ir.contains("try_end"), "should have end block");
    }

    #[test]
    fn test_try_finally() {
        let src = concat!(
            "fn main() -> Int64 {\n",
            "  try {\n",
            "    println(1);\n",
            "  } catch (e: Int64) {\n",
            "    println(e);\n",
            "  } finally {\n",
            "    println(0);\n",
            "  };\n",
            "  return 0;\n",
            "}"
        );
        let ir = compile_to_ir(src);
        assert!(ir.contains("finally_"), "should have finally block");
        assert!(ir.contains("try_end"), "should have end block");
    }

    #[test]
    fn test_multiple_catches() {
        let src = concat!(
            "fn main() -> Int64 {\n",
            "  try {\n",
            "    println(1);\n",
            "  } catch (e: Int64) {\n",
            "    println(e);\n",
            "  } catch (f: Int64) {\n",
            "    println(f);\n",
            "  };\n",
            "  return 0;\n",
            "}"
        );
        let ir = compile_to_ir(src);
        assert!(ir.contains("catch_0"), "should have catch_0 block");
        assert!(ir.contains("catch_1"), "should have catch_1 block");
        assert!(ir.contains("catch_0_ok"), "should have catch_0_ok guard");
        assert!(ir.contains("catch_1_ok"), "should have catch_1_ok guard");
    }

    #[test]
    fn test_cast_float_to_int() {
        let src = "namespace test { class C { fnc f(x: Float64) -> Int64 { return cast x as Int64; } } }";
        let ir = compile_to_ir(src);
        assert!(ir.contains("fptosi double"), "should use fptosi for float→int");
    }

    #[test]
    fn test_cast_int_to_float() {
        let src = "namespace test { class C { fnc f(x: Int64) -> Float64 { return cast x as Float64; } } }";
        let ir = compile_to_ir(src);
        assert!(ir.contains("sitofp i64"), "should use sitofp for int→float");
    }

    #[test]
    fn test_cast_double_to_float() {
        let src = "namespace test { class C { fnc f(x: Float64) -> Float32 { return cast x as Float32; } } }";
        let ir = compile_to_ir(src);
        assert!(ir.contains("fptrunc double"), "should use fptrunc for double→float");
    }

    #[test]
    fn test_loop_stmt() {
        let src = "fn main() -> Int64 {\n  loop { break; };\n  return 0;\n}";
        let ir = compile_to_ir(src);
        assert!(ir.contains("loop_body"), "should have loop body block");
        assert!(ir.contains("loop_end"), "should have loop end block");
    }

    #[test]
    fn test_return_as_expr() {
        // return used in expression position (right side of let)
        let src = concat!(
            "namespace test { class C { fnc f() -> Int64 {\n",
            "  let _ = return 42;\n",
            "  return 0;\n",
            "} } }"
        );
        let ir = compile_to_ir(src);
        assert!(ir.contains("ret i64 42"), "should emit ret for return-expr");
        assert!(ir.contains("ret_dead"), "should have dead block after return expr");
    }

    #[test]
    fn test_break_as_expr() {
        let src = concat!(
            "fn main() -> Int64 {\n",
            "  loop {\n",
            "    let _ = break;\n",
            "  };\n",
            "  return 0;\n",
            "}"
        );
        let ir = compile_to_ir(src);
        assert!(ir.contains("break_dead"), "should have dead block after break expr");
    }

    #[test]
    fn test_continue_as_expr() {
        let src = concat!(
            "fn main() -> Int64 {\n",
            "  let i = 0;\n",
            "  loop {\n",
            "    let _ = continue;\n",
            "  };\n",
            "  return 0;\n",
            "}"
        );
        let ir = compile_to_ir(src);
        assert!(ir.contains("cont_dead"), "should have dead block after continue expr");
    }

    #[test]
    fn test_generic_class_monomorphization() {
        let src = concat!(
            "class Box<T> {\n",
            "  value: T;\n",
            "  fn get() -> T {\n",
            "    return this.value;\n",
            "  }\n",
            "}\n",
            "fn main() -> Int64 {\n",
            "  let b = new Box<Int64>(42);\n",
            "  return b.get();\n",
            "}"
        );
        let ir = compile_to_ir(src);
        assert!(ir.contains("Box__i64_get"), "should emit specialized method Box__i64_get");
        assert!(ir.contains("define i64 @Box__i64_get"), "method should return i64");
        assert!(!ir.contains("define i64 @Box_get"), "unspecialized Box_get must not be emitted");
    }

    #[test]
    fn test_generic_class_two_instantiations() {
        let src = concat!(
            "class Pair<T> {\n",
            "  first: T;\n",
            "  fn fst() -> T {\n",
            "    return this.first;\n",
            "  }\n",
            "}\n",
            "fn main() -> Int64 {\n",
            "  let a = new Pair<Int64>(1);\n",
            "  let b = new Pair<Float64>(2);\n",
            "  return a.fst();\n",
            "}"
        );
        let ir = compile_to_ir(src);
        assert!(ir.contains("Pair__i64_fst"), "should have i64 specialization");
        assert!(ir.contains("Pair__double_fst"), "should have double specialization");
    }
}
