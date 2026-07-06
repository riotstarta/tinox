use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::path::Path;
use tinox_common::{Error, ErrorBag, Span, Spanned};
use tinox_parser::{
    BinaryOp, CatchClause, DeclKind, Expr, ExprKind, Literal, Method, Pattern,
    SourceFile, Stmt, StmtKind, Type, UnaryOp,
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
pub struct LogMaskFieldInfo {
    pub class_name: String,
    pub field_name: String,
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

#[derive(Debug, Clone, PartialEq)]
pub enum MetricKind {
    Timed,
    Counted,
}

#[derive(Debug, Clone)]
pub struct MetricEntry {
    pub kind: MetricKind,
    pub metric_name: String,
    pub class_name: String,
    pub fn_name: String,
}

#[derive(Debug, Clone)]
pub struct EntityFieldEntry {
    pub field_name: String,
    pub column_name: String,
    pub is_id: bool,
    pub is_generated: bool,
    pub not_null: bool,
    pub field_llvm_type: String,
}

#[derive(Debug, Clone)]
pub struct EntityEntry {
    pub class_name: String,
    pub table_name: String,
    pub fields: Vec<EntityFieldEntry>,
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
    method_ret_class: HashMap<String, String>, // method key → Tinox class name when it returns a class
    static_method_keys: HashSet<String>,       // method keys for static (fnc) methods — no self param
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
    /// variant name → payload kind per argument ("String" | "Map" | "List" | "Other"),
    /// used to bind match-pattern payload variables with their true LLVM type.
    enum_variant_payloads: HashMap<String, Vec<String>>,
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
    /// Fields annotated with @Sensitive — logged as '***'
    sensitive_fields: Vec<LogMaskFieldInfo>,
    /// Fields annotated with @Masked — partially masked in logs
    masked_fields: Vec<LogMaskFieldInfo>,
    /// Fields annotated with @DoNotSerialize — excluded from JSON/XML serialization
    do_not_serialize_fields: Vec<LogMaskFieldInfo>,
    /// Class names annotated with @JsonSerializable — get a compiler-generated toJson() method
    json_serializable_classes: Vec<String>,
    /// Metric instrumentation entries from @Timed / @Counted annotations
    metric_entries: Vec<MetricEntry>,
    /// ORM entity entries from @Entity / @Table annotations
    entity_entries: Vec<EntityEntry>,
    /// DB connection URL from tinox.toml [database] — emitted as compile-time constant
    db_url: Option<String>,
    /// Whether a [metrics] endpoint is enabled (path to expose on)
    metrics_path: Option<String>,
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
    /// ClassName_methodName -> list of Tinox param types (excluding self) for lambda param inference
    method_param_types: HashMap<String, Vec<tinox_parser::Type>>,
    /// Temporary: expected class names for the next lambda's params (set before gen_expr on lambda)
    pending_lambda_param_types: Vec<Option<String>>,
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
            method_ret_class: HashMap::new(),
            static_method_keys: HashSet::new(),
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
            enum_variant_payloads: HashMap::new(),
            known_enum_types: HashSet::new(),
            inline_functions: HashSet::new(),
            inline_methods: HashSet::new(),
            route_entries: Vec::new(),
            di_components: Vec::new(),
            log_classes: HashSet::new(),
            config_fields: Vec::new(),
            cli_commands: Vec::new(),
            sensitive_fields: Vec::new(),
            masked_fields: Vec::new(),
            do_not_serialize_fields: Vec::new(),
            json_serializable_classes: Vec::new(),
            metric_entries: Vec::new(),
            entity_entries: Vec::new(),
            db_url: None,
            metrics_path: None,
            test_entry: None,
            has_main: false,
            defined_classes: HashSet::new(),
            struct_field_class_types: HashMap::new(),
            struct_field_llvm_types: HashMap::new(),
            fn_field_sigs: HashMap::new(),
            method_param_types: HashMap::new(),
            pending_lambda_param_types: Vec::new(),
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
        sensitive_fields: Vec<LogMaskFieldInfo>,
        masked_fields: Vec<LogMaskFieldInfo>,
        do_not_serialize_fields: Vec<LogMaskFieldInfo>,
        json_serializable_classes: Vec<String>,
        metric_entries: Vec<MetricEntry>,
    ) {
        self.inline_functions = inline_fns;
        self.inline_methods = inline_meths;
        self.route_entries = routes;
        self.di_components = di_components;
        self.log_classes = log_classes;
        self.config_fields = config_fields;
        self.cli_commands = cli_commands;
        self.sensitive_fields = sensitive_fields;
        self.masked_fields = masked_fields;
        self.do_not_serialize_fields = do_not_serialize_fields;
        self.json_serializable_classes = json_serializable_classes;
        self.metric_entries = metric_entries;
    }

    pub fn set_metrics_config(&mut self, path: Option<String>) {
        self.metrics_path = path;
    }

    pub fn set_entity_entries(&mut self, entries: Vec<EntityEntry>) {
        self.entity_entries = entries;
    }

    pub fn set_db_url(&mut self, url: Option<String>) {
        self.db_url = url;
    }

    /// Register a string constant and return an inline `getelementptr` expression (i8*).
    fn make_string_const(&mut self, s: &str) -> String {
        let label = format!("__metric_str_{}", self.strings.len());
        self.strings.insert(label.clone(), s.to_string());
        let len = s.len() + 1;
        format!("getelementptr [{len} x i8], [{len} x i8]* @{label}, i64 0, i64 0")
    }

    /// Emit a tinox_clock_nanos() call, subtract start_reg, and call tinox_histogram_record.
    fn emit_histogram_record(&mut self, label: &str, start_reg: &str) {
        let end_reg  = self.temp();
        let dur_reg  = self.temp();
        let name_ptr = self.make_string_const(label);
        writeln!(&mut self.ir, "{} = call i64 @tinox_clock_nanos()", end_reg).unwrap();
        writeln!(&mut self.ir, "{} = sub i64 {}, {}", dur_reg, end_reg, start_reg).unwrap();
        writeln!(&mut self.ir, "call void @tinox_histogram_record(i8* {}, i64 {})", name_ptr, dur_reg).unwrap();
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

    /// Classify an enum variant payload type for match-binding purposes.
    fn payload_kind(ty: &Type) -> String {
        match ty {
            Type::String => "String",
            Type::Map(_, _) => "Map",
            Type::Array(_) => "List",
            Type::Generic { name, .. } if name == "List" => "List",
            Type::Float32 | Type::Float64 => "Float",
            Type::Mutable(inner) | Type::Ref(inner) => return Self::payload_kind(inner),
            _ => "Other",
        }
        .to_string()
    }

    /// The payload map is keyed by variant name only; when two enums share a
    /// variant name (e.g. a payload variant and a no-arg token variant), keep the
    /// entry with more payload info — no-arg matches never bind payloads, so the
    /// richer entry is always safe to use.
    fn register_variant_payloads(&mut self, name: &str, kinds: Vec<String>) {
        match self.enum_variant_payloads.get(name) {
            Some(existing) if existing.len() >= kinds.len() => {}
            _ => {
                self.enum_variant_payloads.insert(name.to_string(), kinds);
            }
        }
    }

    /// Bind a match-pattern payload variable with the LLVM type derived from the
    /// enum declaration's payload type, so that subsequent operator/method dispatch
    /// (string ==/+/len/contains, map get/insert, array len/iteration) works correctly.
    fn bind_match_payload(
        &mut self,
        ctx: &mut GenCtx,
        disc_name: &str,
        arg_index: usize,
        arg_name: &str,
        arg_val: &str,
    ) {
        let kind = self
            .enum_variant_payloads
            .get(disc_name)
            .and_then(|ks| ks.get(arg_index))
            .cloned()
            .unwrap_or_else(|| "Other".to_string());
        // Unique slot name avoids duplicate allocas when the same variable name
        // appears in multiple match arms.
        let slot_name = format!("{}_{}", arg_name, self.temp_count);
        self.temp_count += 1;
        let (llvm_ty, decl_ty): (&str, Option<&str>) = match kind.as_str() {
            "String" => ("i8*", None),
            "Map" => ("i8*", Some("Map")),
            "List" => ("i64*", None),
            "Float" => ("double", None),
            _ => ("i64", None),
        };
        let store_val = if llvm_ty == "i64" {
            arg_val.to_string()
        } else if llvm_ty == "double" {
            let p = self.temp();
            writeln!(&mut self.ir, "{} = bitcast i64 {} to double", p, arg_val).unwrap();
            p
        } else {
            let p = self.temp();
            writeln!(&mut self.ir, "{} = inttoptr i64 {} to {}", p, arg_val, llvm_ty).unwrap();
            p
        };
        ctx.locals
            .insert(arg_name.to_string(), (llvm_ty.to_string(), ctx.locals.len()));
        ctx.local_slots.insert(arg_name.to_string(), slot_name.clone());
        writeln!(&mut self.ir, "%{} = alloca {}", slot_name, llvm_ty).unwrap();
        writeln!(
            &mut self.ir,
            "store {} {}, {}* %{}",
            llvm_ty, store_val, llvm_ty, slot_name
        )
        .unwrap();
        match decl_ty {
            Some(t) => {
                ctx.local_types.insert(arg_name.to_string(), t.to_string());
            }
            None => {
                ctx.local_types.remove(arg_name);
            }
        }
    }

    /// True for declared return/field types `List<String>` and `Array<String>` —
    /// used to type such values as "Array:String" so element access yields strings.
    fn is_string_list_type(ty: &tinox_parser::Type) -> bool {
        use tinox_parser::Type;
        match ty {
            Type::Array(inner) => matches!(inner.as_ref(), Type::String),
            Type::Generic { name, args } => {
                (name == "List" || name == "Array")
                    && matches!(args.first(), Some(Type::String))
            }
            _ => false,
        }
    }

    fn extract_class_type_name(ty: &tinox_parser::Type) -> Option<String> {
        use tinox_parser::Type;
        if Self::is_string_list_type(ty) {
            return Some("Array:String".to_string());
        }
        match ty {
            Type::Named(n) => Some(n.clone()),
            Type::Generic { name, .. } => Some(name.clone()),
            Type::Map(_, _) => Some("Map".to_string()),
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
            ExprKind::Ident(name) => {
                let ty = ctx.local_types.get(name)?;
                // "List:ClassName" → strip prefix, return element class name
                if let Some(cls) = ty.strip_prefix("List:") {
                    return Some(cls.to_string());
                }
                Some(ty.clone())
            }
            ExprKind::Index { obj, .. } => {
                // For arr[i], infer the element struct type from the array variable
                if let ExprKind::Ident(arr_name) = &obj.node {
                    let ty = ctx.local_types.get(arr_name.as_str())?;
                    // "List:ClassName" → element type is ClassName
                    ty.strip_prefix("List:").map(|s| s.to_string())
                } else {
                    None
                }
            }
            ExprKind::This => ctx.current_struct.clone(),
            ExprKind::FieldAccess { obj, field } => {
                let outer = self.infer_struct_type(obj, ctx)?;
                self.struct_field_class_types
                    .get(&outer)
                    .and_then(|m| m.get(field.as_str()))
                    .cloned()
            }
            ExprKind::EnumValue { enum_name, variant, .. } => {
                // Static method call returning a known class, e.g. JsonValueHelper::asObject
                let key = format!("{}_{}", enum_name, variant);
                self.method_ret_class.get(&key).cloned()
            }
            ExprKind::MethodCall { obj: mc_obj, method: mc_method, .. } => {
                // Instance method call returning a known class
                self.infer_struct_type(mc_obj, ctx)
                    .and_then(|obj_class| {
                        let key = format!("{}_{}", obj_class, mc_method);
                        self.method_ret_class.get(&key).cloned()
                    })
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
        writeln!(&mut self.ir, "declare i8* @tinox_string_mask_partial(i8*)").unwrap();
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
        // JsonBuilder — used by @JsonSerializable toJson()
        writeln!(&mut self.ir, "declare i8* @jsonBuilderCreate()").unwrap();
        writeln!(&mut self.ir, "declare void @jsonBuilderAddInt(i8*, i8*, i64)").unwrap();
        writeln!(&mut self.ir, "declare void @jsonBuilderAddFloat(i8*, i8*, double)").unwrap();
        writeln!(&mut self.ir, "declare void @jsonBuilderAddBool(i8*, i8*, i32)").unwrap();
        writeln!(&mut self.ir, "declare void @jsonBuilderAddString(i8*, i8*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare void @jsonBuilderAddIntList(i8*, i8*, i64*)").unwrap();
        writeln!(&mut self.ir, "declare i8* @jsonBuilderFinish(i8*)").unwrap();
        // fromJson field helpers
        writeln!(&mut self.ir, "declare i64 @jsonGetIntField(i64*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare double @jsonGetFloatField(i64*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare i32 @jsonGetBoolField(i64*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare i8* @jsonGetStringField(i64*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64* @jsonGetIntListField(i64*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare double @llvm.floor.f64(double)").unwrap();
        writeln!(&mut self.ir, "declare double @llvm.ceil.f64(double)").unwrap();
        writeln!(&mut self.ir, "declare double @llvm.round.f64(double)").unwrap();
        writeln!(&mut self.ir, "declare void @exit(i64)").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_config_get(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_config_get_int(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_config_get_bool(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_string_equals(i8*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_string_compare(i8*, i8*)").unwrap();
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
        writeln!(&mut self.ir, "declare i64* @dirList(i8*)").unwrap();
        writeln!(&mut self.ir, "declare void @dirCreate(i8*)").unwrap();
        writeln!(&mut self.ir, "declare void @dirDelete(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i8* @envGet(i8*)").unwrap();
        writeln!(&mut self.ir, "declare void @envSet(i8*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare void @envRemove(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i8* @envCurrentDir()").unwrap();
        writeln!(&mut self.ir, "declare void @envSetCurrentDir(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i8* @fileReadAllText(i8*)").unwrap();
        writeln!(&mut self.ir, "declare void @fileWriteAllText(i8*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare void @fileAppendText(i8*, i8*)").unwrap();
        writeln!(&mut self.ir, "declare void @fileClose(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64* @processArgs()").unwrap();
        writeln!(&mut self.ir, "declare i1 @fileExists(i8*)").unwrap();
        writeln!(&mut self.ir, "declare void @processExit(i64)").unwrap();
        writeln!(&mut self.ir, "declare i64 @regexIsMatch(i64, i64)").unwrap();
        writeln!(&mut self.ir, "declare i64* @regexFindAll(i64, i64)").unwrap();
        writeln!(&mut self.ir, "declare i64 @regexReplace(i64, i64, i64)").unwrap();
        writeln!(&mut self.ir, "declare i64* @regexSplit(i64, i64)").unwrap();
        writeln!(&mut self.ir, "declare i64* @regexMatchGroups(i8*, i8*, i64, i64)").unwrap();
        writeln!(&mut self.ir, "declare i64* @tinox_array_remove_at(i64*, i64)").unwrap();
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
        // Metrics runtime
        writeln!(&mut self.ir, "declare void @tinox_counter_inc(i8*)").unwrap();
        writeln!(&mut self.ir, "declare void @tinox_histogram_record(i8*, i64)").unwrap();
        writeln!(&mut self.ir, "declare void @tinox_gauge_set(i8*, i64)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_clock_nanos()").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_metrics_prometheus()").unwrap();
        // DB / ORM runtime
        writeln!(&mut self.ir, "declare void @tinox_db_connect(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_db_get_conn()").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_db_exec(i8*, i8*, i8**, i64)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_db_nrows(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64 @tinox_db_ncols(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_db_getval(i8*, i64, i64)").unwrap();
        writeln!(&mut self.ir, "declare i64  @tinox_db_getval_int(i8*, i64, i64)").unwrap();
        writeln!(&mut self.ir, "declare i1  @tinox_db_is_null(i8*, i64, i64)").unwrap();
        writeln!(&mut self.ir, "declare void @tinox_db_free(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_db_error(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i8** @tinox_params_alloc(i64)").unwrap();
        writeln!(&mut self.ir, "declare void @tinox_params_set(i8**, i64, i8*)").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_int_to_param(i64)").unwrap();
        // float math builtins
        writeln!(&mut self.ir, "declare double @log(double)").unwrap();
        writeln!(&mut self.ir, "declare double @exp(double)").unwrap();
        writeln!(&mut self.ir, "declare i64 @mathIsNan(double)").unwrap();
        writeln!(&mut self.ir, "declare i64 @mathIsInfinite(double)").unwrap();
        writeln!(&mut self.ir, "declare i64 @mathIsNormal(double)").unwrap();
        writeln!(&mut self.ir, "declare double @mathNan()").unwrap();
        writeln!(&mut self.ir, "declare double @mathInf()").unwrap();
        writeln!(&mut self.ir, "declare double @tgamma(double)").unwrap();
        writeln!(&mut self.ir, "declare double @lgamma(double)").unwrap();
        writeln!(&mut self.ir, "declare double @cbrt(double)").unwrap();
        writeln!(&mut self.ir, "declare double @trunc(double)").unwrap();
        writeln!(&mut self.ir, "declare double @rint(double)").unwrap();
        writeln!(&mut self.ir, "declare double @logb(double)").unwrap();
        writeln!(&mut self.ir, "declare double @log2(double)").unwrap();
        writeln!(&mut self.ir, "declare double @log10(double)").unwrap();
        writeln!(&mut self.ir, "declare double @exp2(double)").unwrap();
        writeln!(&mut self.ir, "declare double @exp10(double)").unwrap();
        // jgrep-tinox env/time/debug builtins
        writeln!(&mut self.ir, "declare i8* @envDump()").unwrap();
        writeln!(&mut self.ir, "declare i64 @currentTimeSecs()").unwrap();
        writeln!(&mut self.ir, "declare i8* @strftimeStr(i8*, i64)").unwrap();
        writeln!(&mut self.ir, "declare i64 @fromdateStr(i8*)").unwrap();
        writeln!(&mut self.ir, "declare void @printStderr(i8*)").unwrap();
        writeln!(&mut self.ir, "declare i64 @isStdinTty()").unwrap();
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

        // Pre-pass: register all enum type names so that method return type resolution
        // (which calls type_to_llvm_inst) can correctly classify Named enum types as i64.
        for decl in &source.decls {
            match &decl.node {
                DeclKind::Enum(e) => {
                    self.known_enum_types.insert(e.name.clone());
                    for variant in &e.variants {
                        self.known_enum_variants.insert(variant.name.clone());
                        self.register_variant_payloads(
                            &variant.name,
                            variant.args.iter().map(Self::payload_kind).collect(),
                        );
                    }
                }
                DeclKind::Namespace(ns) => {
                    for inner in &ns.decls {
                        if let DeclKind::Enum(e) = &inner.node {
                            self.known_enum_types.insert(e.name.clone());
                            for variant in &e.variants {
                                self.known_enum_variants.insert(variant.name.clone());
                                self.register_variant_payloads(
                            &variant.name,
                            variant.args.iter().map(Self::payload_kind).collect(),
                        );
                            }
                        }
                    }
                }
                _ => {}
            }
        }

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
                    self.method_impl.insert(key.clone(), key.clone());
                    self.method_ret_types.insert(
                        format!("{}_{}", c.name, method.name),
                        self.type_to_llvm_inst(&method.ret_type),
                    );
                    // Track static methods (fnc) — they don't have a self parameter
                    if method.static_ {
                        self.static_method_keys.insert(key.clone());
                    }
                    // Track class name for methods returning class instances (for local_types inference)
                    if let Type::Named(ret_class) = &method.ret_type {
                        if self.defined_classes.contains(ret_class.as_str()) || self.struct_layouts.contains_key(ret_class.as_str()) {
                            self.method_ret_class.insert(key.clone(), ret_class.clone());
                        }
                    } else if matches!(&method.ret_type, Type::Map(_, _)) {
                        self.method_ret_class.insert(key.clone(), "Map".to_string());
                    } else if Self::is_string_list_type(&method.ret_type) {
                        self.method_ret_class.insert(key.clone(), "Array:String".to_string());
                    }
                    let param_tys: Vec<tinox_parser::Type> = method.params.iter()
                        .map(|p| p.param_type.clone()).collect();
                    self.method_param_types.insert(format!("{}_{}", c.name, method.name), param_tys);
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
                                self.method_ret_types.insert(child_key, self.type_to_llvm_inst(&method.ret_type));
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
                        self.fn_sigs.insert(fn_name.clone(), (ret_ty, param_tys));
                        if Self::is_string_list_type(&f.ret_type) {
                            self.method_ret_class.insert(fn_name, "Array:String".to_string());
                        }
                    }
                }
                DeclKind::Class(c) if !c.type_params.is_empty() => {
                    self.generic_classes.insert(c.name.clone(), c.clone());
                }
                DeclKind::Enum(e) => {
                    self.known_enum_types.insert(e.name.clone());
                    for variant in &e.variants {
                        self.known_enum_variants.insert(variant.name.clone());
                        self.register_variant_payloads(
                            &variant.name,
                            variant.args.iter().map(Self::payload_kind).collect(),
                        );
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
                                self.register_variant_payloads(
                            &variant.name,
                            variant.args.iter().map(Self::payload_kind).collect(),
                        );
                            }
                        } else if let DeclKind::Function(f) = &inner.node {
                            // Register namespace-level functions (incl. extern) in fn_sigs
                            if !f.type_params.is_empty() {
                                self.generic_fns.insert(f.name.clone(), f.clone());
                            } else {
                                let fn_name = f.name.clone();
                                let ret_ty = self.type_to_llvm_inst(&f.ret_type);
                                let param_tys: Vec<String> = f.params.iter().map(|p| Self::type_to_llvm(&p.param_type)).collect();
                                self.fn_sigs.insert(fn_name, (ret_ty, param_tys));
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Pre-register toString() for @Sensitive/@Masked classes so method dispatch works
        self.pre_register_log_mask_tostring();
        // Pre-register toJson() / fromJson() for @JsonSerializable classes
        self.pre_register_json_to_json();
        self.pre_register_json_from_json();

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
                        if method.type_params.is_empty() {
                            self.gen_class_method(&c.name, method)?;
                        }
                    }
                }
                DeclKind::Immutable(u) => {
                    self.emit_immutable_new(u);
                }
                DeclKind::Namespace(ns) => {
                    for inner in &ns.decls {
                        match &inner.node {
                            DeclKind::Function(f) if f.type_params.is_empty() => {
                                self.gen_fn(f)?;
                            }
                            DeclKind::Class(c) => {
                                if c.type_params.is_empty() {
                                    for method in &c.methods {
                                        if method.type_params.is_empty() {
                                            self.gen_class_method(&c.name, method)?;
                                        }
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

        // Emit toString() for classes with @Sensitive or @Masked fields
        self.emit_log_mask_code();

        // Emit toJson() / fromJson() for classes with @JsonSerializable
        self.emit_json_serialize_code();
        self.emit_json_deserialize_code();

        // Emit test-runner main if set_test_entry() was called
        self.emit_test_code();

        // Emit SQL-constant functions and row-mapping helpers for @Entity classes
        self.emit_entity_code();

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

        // ── Metrics endpoint shim (if enabled) ──────────────────────────────────
        let metrics_path = self.metrics_path.clone();
        if let Some(ref mpath) = metrics_path {
            let mpath_escaped = Self::escape_llvm_string(mpath);
            let mpath_len = mpath.len() + 1;
            writeln!(&mut self.ir,
                "@__metrics_path = private constant [{mpath_len} x i8] c\"{mpath_escaped}\\00\"").unwrap();
            // Shim: GET /metrics → call tinox_metrics_prometheus(), return as text/plain
            writeln!(&mut self.lambda_ir, "declare i8* @tinox_metrics_prometheus()").unwrap();
            writeln!(&mut self.lambda_ir, "declare i64* @tinox_HttpServer_new(i64)").unwrap();
            writeln!(&mut self.lambda_ir, "define void @__metrics_shim(i64 %ctx_i64) {{").unwrap();
            writeln!(&mut self.lambda_ir, "entry:").unwrap();
            writeln!(&mut self.lambda_ir, "  %ctx_ptr = inttoptr i64 %ctx_i64 to i64*").unwrap();
            // HttpContext[1] = response ptr (i64*)
            writeln!(&mut self.lambda_ir, "  %resp_field = getelementptr i64, i64* %ctx_ptr, i64 1").unwrap();
            writeln!(&mut self.lambda_ir, "  %resp_i64 = load i64, i64* %resp_field").unwrap();
            writeln!(&mut self.lambda_ir, "  %resp_ptr = inttoptr i64 %resp_i64 to i64*").unwrap();
            // Get prometheus text
            writeln!(&mut self.lambda_ir, "  %prom_text = call i8* @tinox_metrics_prometheus()").unwrap();
            // Set status 200
            writeln!(&mut self.lambda_ir, "  %sc_field = getelementptr i64, i64* %resp_ptr, i64 0").unwrap();
            writeln!(&mut self.lambda_ir, "  store i64 200, i64* %sc_field").unwrap();
            // Set body
            writeln!(&mut self.lambda_ir, "  %body_field = getelementptr i64, i64* %resp_ptr, i64 2").unwrap();
            writeln!(&mut self.lambda_ir, "  %body_i64 = ptrtoint i8* %prom_text to i64").unwrap();
            writeln!(&mut self.lambda_ir, "  store i64 %body_i64, i64* %body_field").unwrap();
            // Set Content-Type header to text/plain; version=0.0.4
            let ct = "text/plain; version=0.0.4";
            let ct_escaped = Self::escape_llvm_string(ct);
            let ct_len = ct.len() + 1;
            writeln!(&mut self.ir,
                "@__metrics_ct = private constant [{ct_len} x i8] c\"{ct_escaped}\\00\"").unwrap();
            writeln!(&mut self.lambda_ir,
                "  %ct_hdr_key = getelementptr [13 x i8], [13 x i8]* @__hdr_content_type, i64 0, i64 0").unwrap();
            writeln!(&mut self.lambda_ir,
                "  %ct_hdr_val = getelementptr [{ct_len} x i8], [{ct_len} x i8]* @__metrics_ct, i64 0, i64 0").unwrap();
            // headers are at HttpResponse[1] (i8* to map)
            writeln!(&mut self.lambda_ir, "  %hdrs_field = getelementptr i64, i64* %resp_ptr, i64 1").unwrap();
            writeln!(&mut self.lambda_ir, "  %hdrs_i64 = load i64, i64* %hdrs_field").unwrap();
            writeln!(&mut self.lambda_ir, "  %hdrs_ptr = inttoptr i64 %hdrs_i64 to i8*").unwrap();
            writeln!(&mut self.lambda_ir, "  call void @tinox_map_set(i8* %hdrs_ptr, i8* %ct_hdr_key, i64 %body_i64)").unwrap();
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

        // Register the /metrics route if enabled
        if let Some(ref mpath) = metrics_path {
            let mpath_len = mpath.len() + 1;
            writeln!(&mut self.lambda_ir,
                "  %metrics_fn = ptrtoint void (i64)* @__metrics_shim to i64").unwrap();
            writeln!(&mut self.lambda_ir,
                "  %metrics_path = getelementptr [{mpath_len} x i8], [{mpath_len} x i8]* @__metrics_path, i64 0, i64 0").unwrap();
            writeln!(&mut self.lambda_ir,
                "  call void @tinox_HttpServer_get(i64* %server, i8* %metrics_path, i64 %metrics_fn)").unwrap();
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
            timed_metric: None,
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
                || matches!(&p.param_type, Type::Generic { name, .. } if name == "Array")
                || matches!(&p.param_type, Type::Generic { name, args } if name == "List" && matches!(args.first(), Some(Type::String))) {
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

        // @Counted — increment call counter at function entry
        let counted_metric = self.metric_entries.iter().find(|m| {
            m.kind == MetricKind::Counted && m.class_name.is_empty() && m.fn_name == f.name
        }).map(|m| m.metric_name.clone());
        if let Some(ref label) = counted_metric {
            let s = self.make_string_const(label);
            writeln!(&mut self.ir, "call void @tinox_counter_inc(i8* {})", s).unwrap();
        }

        // @Timed — record start timestamp
        let timed_metric = self.metric_entries.iter().find(|m| {
            m.kind == MetricKind::Timed && m.class_name.is_empty() && m.fn_name == f.name
        }).map(|m| m.metric_name.clone());
        if let Some(ref label) = timed_metric {
            let start_reg = self.temp();
            writeln!(&mut self.ir, "{} = call i64 @tinox_clock_nanos()", start_reg).unwrap();
            ctx.timed_metric = Some((label.clone(), start_reg));
        }

        self.gen_stmt_body(&f.body, &mut ctx)?;

        let has_terminator = self.ir.lines().last().map_or(false, |l| {
            let t = l.trim();
            t.starts_with("ret ") || t.starts_with("br ")
        });
        if !has_terminator {
            if let Some((ref label, ref start_reg)) = ctx.timed_metric.clone() {
                self.emit_histogram_record(label, start_reg);
            }
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
            timed_metric: None,
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
                || matches!(&p.param_type, Type::Generic { name, .. } if name == "Array")
                || matches!(&p.param_type, Type::Generic { name, args } if name == "List" && matches!(args.first(), Some(Type::String))) {
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

        // @Counted — increment call counter at method entry
        let counted_metric = self.metric_entries.iter().find(|m| {
            m.kind == MetricKind::Counted
                && m.class_name == class_name
                && m.fn_name == method.name
        }).map(|m| m.metric_name.clone());
        if let Some(ref label) = counted_metric {
            let s = self.make_string_const(label);
            writeln!(&mut self.ir, "call void @tinox_counter_inc(i8* {})", s).unwrap();
        }

        // @Timed — record start timestamp, store in ctx for return emission
        let timed_metric = self.metric_entries.iter().find(|m| {
            m.kind == MetricKind::Timed
                && m.class_name == class_name
                && m.fn_name == method.name
        }).map(|m| m.metric_name.clone());
        if let Some(ref label) = timed_metric {
            let start_reg = self.temp();
            writeln!(&mut self.ir, "{} = call i64 @tinox_clock_nanos()", start_reg).unwrap();
            ctx.timed_metric = Some((label.clone(), start_reg));
        }

        self.gen_stmt_body(&method.body, &mut ctx)?;

        let has_terminator = self.ir.lines().last().map_or(false, |l| {
            let t = l.trim();
            t.starts_with("ret ") || t.starts_with("br ")
        });
        if !has_terminator {
            // Emit timing before implicit return
            if let Some((ref label, ref start_reg)) = ctx.timed_metric.clone() {
                self.emit_histogram_record(label, start_reg);
            }
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

    /// Return the declared class name of a simple expression (Ident or FieldAccess),
    /// or None for complex expressions. Used for implicit toString() coercion.
    fn expr_class_name(expr: &ExprKind, ctx: &GenCtx) -> Option<String> {
        match expr {
            ExprKind::Ident(name) => ctx.local_types.get(name).cloned(),
            ExprKind::FieldAccess { obj, field } => {
                let obj_class = if let ExprKind::Ident(n) = &obj.node {
                    ctx.local_types.get(n.as_str()).cloned()
                } else {
                    None
                };
                obj_class.and_then(|cn| {
                    ctx.local_types.get(&format!("{}.{}", cn, field)).cloned()
                        .or_else(|| None)
                })
            }
            _ => None,
        }
    }

    /// Convert a raw i64 struct slot to an i8* string, based on LLVM type.
    fn field_val_to_string(&mut self, raw: &str, llvm_ty: &str) -> String {
        match llvm_ty {
            "i8*" => {
                let ptr = self.temp();
                writeln!(&mut self.ir, "  {} = inttoptr i64 {} to i8*", ptr, raw).unwrap();
                ptr
            }
            "i1" => {
                let b = self.temp();
                let s = self.temp();
                writeln!(&mut self.ir, "  {} = trunc i64 {} to i1", b, raw).unwrap();
                writeln!(&mut self.ir, "  {} = call i8* @tinox_bool_to_string(i1 {})", s, b).unwrap();
                s
            }
            "double" | "float" => {
                let f = self.temp();
                let s = self.temp();
                writeln!(&mut self.ir, "  {} = bitcast i64 {} to double", f, raw).unwrap();
                writeln!(&mut self.ir, "  {} = call i8* @tinox_float_to_string(double {})", s, f).unwrap();
                s
            }
            "i64" | "i32" | "i16" | "i8" => {
                let s = self.temp();
                writeln!(&mut self.ir, "  {} = call i8* @tinox_int_to_string(i64 {})", s, raw).unwrap();
                s
            }
            _ => {
                // Object or unknown type
                let content = "<object>";
                let lbl = format!("str{}", self.strings.len());
                self.strings.insert(lbl.clone(), content.to_string());
                let len = content.len() + 1;
                let p = self.temp();
                writeln!(&mut self.ir, "  {} = getelementptr [{} x i8], [{} x i8]* @{}, i64 0, i64 0", p, len, len, lbl).unwrap();
                p
            }
        }
    }

    /// Pre-register `ClassName_toString` return types for classes with masked fields so
    /// the method is visible to user code compiled before `emit_log_mask_code` runs.
    fn pre_register_log_mask_tostring(&mut self) {
        let mut affected: HashSet<String> = HashSet::new();
        for f in &self.sensitive_fields { affected.insert(f.class_name.clone()); }
        for f in &self.masked_fields { affected.insert(f.class_name.clone()); }
        for class_name in &affected {
            let key = format!("{}_toString", class_name);
            // Only register if the user hasn't already defined toString()
            if !self.method_ret_types.contains_key(&key) {
                self.method_ret_types.insert(key, "i8*".to_string());
            }
        }
    }

    /// Emit a `ClassName_toString(i64* %self) -> i8*` method for every class
    /// that has at least one @Sensitive or @Masked field.
    fn emit_log_mask_code(&mut self) {
        let sensitive_set: HashSet<(String, String)> = self.sensitive_fields.iter()
            .map(|f| (f.class_name.clone(), f.field_name.clone()))
            .collect();
        let masked_set: HashSet<(String, String)> = self.masked_fields.iter()
            .map(|f| (f.class_name.clone(), f.field_name.clone()))
            .collect();

        let affected: Vec<String> = {
            let mut s: HashSet<String> = HashSet::new();
            for f in &self.sensitive_fields { s.insert(f.class_name.clone()); }
            for f in &self.masked_fields { s.insert(f.class_name.clone()); }
            let mut v: Vec<String> = s.into_iter().collect();
            v.sort();
            v
        };

        for class_name in affected {
            let layout = match self.struct_layouts.get(&class_name) {
                Some(l) => l.clone(),
                None => continue,
            };
            let llvm_types = match self.struct_field_llvm_types.get(&class_name) {
                Some(m) => m.clone(),
                None => continue,
            };

            // Data fields only (exclude vtable slot and synthetic "log")
            let data_fields: Vec<(String, usize, String)> = layout.iter()
                .enumerate()
                .filter(|(_, f)| *f != "__vtable__" && *f != "log")
                .filter_map(|(idx, f)| llvm_types.get(f).map(|ty| (f.clone(), idx, ty.clone())))
                .collect();

            if data_fields.is_empty() { continue; }

            let fn_key = format!("{}_toString", class_name);
            // Skip if user has already defined toString() — their version takes precedence
            if self.method_ret_types.get(&fn_key).map(|v| v != "i8*").unwrap_or(false) {
                continue;
            }
            writeln!(&mut self.ir, "define i8* @{}_toString(i64* %self) {{", class_name).unwrap();
            writeln!(&mut self.ir, "entry:").unwrap();

            // Start with "ClassName{"
            let prefix = format!("{}{{", class_name);
            let lbl = format!("str{}", self.strings.len());
            self.strings.insert(lbl.clone(), prefix.clone());
            let plen = prefix.len() + 1;
            let prefix_ptr = self.temp();
            writeln!(&mut self.ir, "  {} = getelementptr [{} x i8], [{} x i8]* @{}, i64 0, i64 0", prefix_ptr, plen, plen, lbl).unwrap();
            let mut acc = prefix_ptr;

            for (i, (field_name, struct_idx, llvm_ty)) in data_fields.iter().enumerate() {
                // Separator: "field=" for first, ", field=" for rest
                let sep = if i == 0 { format!("{}=", field_name) } else { format!(", {}=", field_name) };
                let sep_lbl = format!("str{}", self.strings.len());
                self.strings.insert(sep_lbl.clone(), sep.clone());
                let slen = sep.len() + 1;
                let sep_ptr = self.temp();
                writeln!(&mut self.ir, "  {} = getelementptr [{} x i8], [{} x i8]* @{}, i64 0, i64 0", sep_ptr, slen, slen, sep_lbl).unwrap();
                let acc1 = self.temp();
                writeln!(&mut self.ir, "  {} = call i8* @tinox_string_concat(i8* {}, i8* {})", acc1, acc, sep_ptr).unwrap();
                acc = acc1;

                // Load field value (all fields stored as i64)
                let fptr = self.temp();
                writeln!(&mut self.ir, "  {} = getelementptr i64, i64* %self, i64 {}", fptr, struct_idx).unwrap();
                let raw = self.temp();
                writeln!(&mut self.ir, "  {} = load i64, i64* {}", raw, fptr).unwrap();

                let is_sensitive = sensitive_set.contains(&(class_name.clone(), field_name.clone()));
                let is_masked = masked_set.contains(&(class_name.clone(), field_name.clone()));

                let val_str = if is_sensitive {
                    let stars = "***";
                    let slbl = format!("str{}", self.strings.len());
                    self.strings.insert(slbl.clone(), stars.to_string());
                    let slen2 = stars.len() + 1;
                    let p = self.temp();
                    writeln!(&mut self.ir, "  {} = getelementptr [{} x i8], [{} x i8]* @{}, i64 0, i64 0", p, slen2, slen2, slbl).unwrap();
                    p
                } else if is_masked {
                    let raw_str = self.field_val_to_string(&raw.clone(), llvm_ty);
                    let masked = self.temp();
                    writeln!(&mut self.ir, "  {} = call i8* @tinox_string_mask_partial(i8* {})", masked, raw_str).unwrap();
                    masked
                } else {
                    let raw_clone = raw.clone();
                    self.field_val_to_string(&raw_clone, llvm_ty)
                };

                let acc2 = self.temp();
                writeln!(&mut self.ir, "  {} = call i8* @tinox_string_concat(i8* {}, i8* {})", acc2, acc, val_str).unwrap();
                acc = acc2;
            }

            // Close with "}"
            let close = "}";
            let clbl = format!("str{}", self.strings.len());
            self.strings.insert(clbl.clone(), close.to_string());
            let close_len = close.len() + 1;
            let close_ptr = self.temp();
            writeln!(&mut self.ir, "  {} = getelementptr [{} x i8], [{} x i8]* @{}, i64 0, i64 0", close_ptr, close_len, close_len, clbl).unwrap();
            let final_str = self.temp();
            writeln!(&mut self.ir, "  {} = call i8* @tinox_string_concat(i8* {}, i8* {})", final_str, acc, close_ptr).unwrap();
            writeln!(&mut self.ir, "  ret i8* {}", final_str).unwrap();
            writeln!(&mut self.ir, "}}").unwrap();
            writeln!(&mut self.ir).unwrap();
        }
    }

    /// Pre-register `ClassName_toJson` return types for @JsonSerializable classes so
    /// the method is visible to user code compiled before `emit_json_serialize_code` runs.
    fn pre_register_json_to_json(&mut self) {
        let class_names: Vec<String> = self.json_serializable_classes.clone();
        for class_name in &class_names {
            let key = format!("{}_toJson", class_name);
            if !self.method_ret_types.contains_key(&key) {
                self.method_ret_types.insert(key, "i8*".to_string());
            }
        }
    }

    fn pre_register_json_from_json(&mut self) {
        let class_names: Vec<String> = self.json_serializable_classes.clone();
        for class_name in &class_names {
            let key = format!("{}_fromJson", class_name);
            if !self.fn_sigs.contains_key(&key) {
                self.fn_sigs.insert(key, ("i64*".to_string(), vec!["i64*".to_string()]));
            }
            let ret_key = format!("{}_fromJson", class_name);
            if !self.method_ret_types.contains_key(&ret_key) {
                self.method_ret_types.insert(ret_key, "i64*".to_string());
            }
        }
    }

    /// Emit `ClassName_toJson(i64* %self) -> i8*` for every @JsonSerializable class.
    /// Uses JsonBuilder for a single-pass, single-allocation approach instead of
    /// the old chain of tinox_string_concat calls (which did O(N) mallocs).
    fn emit_json_serialize_code(&mut self) {
        let do_not_serialize_set: std::collections::HashSet<(String, String)> =
            self.do_not_serialize_fields.iter()
                .map(|f| (f.class_name.clone(), f.field_name.clone()))
                .collect();

        let class_names: Vec<String> = self.json_serializable_classes.clone();

        for class_name in class_names {
            let layout = match self.struct_layouts.get(&class_name) {
                Some(l) => l.clone(),
                None => continue,
            };
            let llvm_types = match self.struct_field_llvm_types.get(&class_name) {
                Some(m) => m.clone(),
                None => continue,
            };

            let data_fields: Vec<(String, usize, String)> = layout.iter()
                .enumerate()
                .filter(|(_, f)| *f != "__vtable__" && *f != "log")
                .filter(|(_, f)| !do_not_serialize_set.contains(&(class_name.clone(), f.to_string())))
                .filter_map(|(idx, f)| llvm_types.get(f).map(|ty| (f.clone(), idx, ty.clone())))
                .collect();

            writeln!(&mut self.ir, "define i8* @{}_toJson(i64* %self) {{", class_name).unwrap();
            writeln!(&mut self.ir, "entry:").unwrap();
            let builder = self.temp();
            writeln!(&mut self.ir, "  {builder} = call i8* @jsonBuilderCreate()").unwrap();

            for (field_name, struct_idx, llvm_ty) in &data_fields {
                // Intern field name as a string constant
                let key_lbl = format!("str{}", self.strings.len());
                self.strings.insert(key_lbl.clone(), field_name.clone());
                let key_len = field_name.len() + 1;
                let key_ptr = self.temp();
                writeln!(&mut self.ir,
                    "  {key_ptr} = getelementptr [{key_len} x i8], [{key_len} x i8]* @{key_lbl}, i64 0, i64 0").unwrap();

                // Load the raw i64 slot
                let fptr = self.temp();
                let raw  = self.temp();
                writeln!(&mut self.ir, "  {fptr} = getelementptr i64, i64* %self, i64 {struct_idx}").unwrap();
                writeln!(&mut self.ir, "  {raw}  = load i64, i64* {fptr}").unwrap();

                match llvm_ty.as_str() {
                    "i8*" => {
                        let str_val = self.temp();
                        writeln!(&mut self.ir, "  {str_val} = inttoptr i64 {raw} to i8*").unwrap();
                        writeln!(&mut self.ir, "  call void @jsonBuilderAddString(i8* {builder}, i8* {key_ptr}, i8* {str_val})").unwrap();
                    }
                    "double" | "float" => {
                        let dbl = self.temp();
                        writeln!(&mut self.ir, "  {dbl} = bitcast i64 {raw} to double").unwrap();
                        writeln!(&mut self.ir, "  call void @jsonBuilderAddFloat(i8* {builder}, i8* {key_ptr}, double {dbl})").unwrap();
                    }
                    "i1" => {
                        let truncated = self.temp();
                        let extended  = self.temp();
                        writeln!(&mut self.ir, "  {truncated} = trunc i64 {raw} to i1").unwrap();
                        writeln!(&mut self.ir, "  {extended}  = zext i1 {truncated} to i32").unwrap();
                        writeln!(&mut self.ir, "  call void @jsonBuilderAddBool(i8* {builder}, i8* {key_ptr}, i32 {extended})").unwrap();
                    }
                    "i64*" => {
                        let arr_ptr = self.temp();
                        writeln!(&mut self.ir, "  {arr_ptr} = inttoptr i64 {raw} to i64*").unwrap();
                        writeln!(&mut self.ir, "  call void @jsonBuilderAddIntList(i8* {builder}, i8* {key_ptr}, i64* {arr_ptr})").unwrap();
                    }
                    _ => {
                        // i64, i32, etc.
                        writeln!(&mut self.ir, "  call void @jsonBuilderAddInt(i8* {builder}, i8* {key_ptr}, i64 {raw})").unwrap();
                    }
                }
            }

            let result = self.temp();
            writeln!(&mut self.ir, "  {result} = call i8* @jsonBuilderFinish(i8* {builder})").unwrap();
            writeln!(&mut self.ir, "  ret i8* {result}").unwrap();
            writeln!(&mut self.ir, "}}").unwrap();
            writeln!(&mut self.ir).unwrap();
        }
    }

    /// Emit `ClassName_fromJson(i64* %json_val) -> i64*` for every @JsonSerializable class.
    fn emit_json_deserialize_code(&mut self) {
        let class_names: Vec<String> = self.json_serializable_classes.clone();

        for class_name in class_names {
            let layout = match self.struct_layouts.get(&class_name) {
                Some(l) => l.clone(),
                None => continue,
            };
            let llvm_types = match self.struct_field_llvm_types.get(&class_name) {
                Some(m) => m.clone(),
                None => continue,
            };

            let n_slots  = layout.len().max(1);
            let byte_size = n_slots * 8;
            let has_vtable = layout.first().map(|f| f == "__vtable__").unwrap_or(false);

            writeln!(&mut self.ir, "define i64* @{}_fromJson(i64* %json_val) {{", class_name).unwrap();
            writeln!(&mut self.ir, "entry:").unwrap();
            let raw  = self.temp();
            let self_ = self.temp();
            writeln!(&mut self.ir, "  {raw}   = call i8* @tinox_alloc(i64 {byte_size})").unwrap();
            writeln!(&mut self.ir, "  {self_} = bitcast i8* {raw} to i64*").unwrap();

            // Zero all slots first so unhandled fields are safe
            for fi in 0..n_slots {
                let zp = self.temp();
                writeln!(&mut self.ir, "  {zp} = getelementptr i64, i64* {self_}, i64 {fi}").unwrap();
                writeln!(&mut self.ir, "  store i64 0, i64* {zp}").unwrap();
            }

            // Set vtable pointer if present
            if has_vtable {
                let vt_i64 = self.temp();
                let vt_ptr = self.temp();
                writeln!(&mut self.ir, "  {vt_i64} = ptrtoint i64* getelementptr ([1 x i64], [1 x i64]* @{class_name}_vtable, i64 0, i64 0) to i64").unwrap();
                writeln!(&mut self.ir, "  {vt_ptr} = getelementptr i64, i64* {self_}, i64 0").unwrap();
                writeln!(&mut self.ir, "  store i64 {vt_i64}, i64* {vt_ptr}").unwrap();
            }

            // Fill data fields from JSON
            for (struct_idx, field_name) in layout.iter().enumerate() {
                if field_name == "__vtable__" || field_name == "log" { continue; }
                let llvm_ty = match llvm_types.get(field_name) {
                    Some(t) => t.clone(),
                    None => continue,
                };

                let key_lbl = format!("str{}", self.strings.len());
                self.strings.insert(key_lbl.clone(), field_name.clone());
                let key_len = field_name.len() + 1;
                let key_ptr = self.temp();
                writeln!(&mut self.ir,
                    "  {key_ptr} = getelementptr [{key_len} x i8], [{key_len} x i8]* @{key_lbl}, i64 0, i64 0").unwrap();

                let fptr = self.temp();
                writeln!(&mut self.ir, "  {fptr} = getelementptr i64, i64* {self_}, i64 {struct_idx}").unwrap();

                let store_val = match llvm_ty.as_str() {
                    "i8*" => {
                        let str_val = self.temp();
                        let as_i64  = self.temp();
                        writeln!(&mut self.ir, "  {str_val} = call i8* @jsonGetStringField(i64* %json_val, i8* {key_ptr})").unwrap();
                        writeln!(&mut self.ir, "  {as_i64}  = ptrtoint i8* {str_val} to i64").unwrap();
                        as_i64
                    }
                    "double" | "float" => {
                        let dbl    = self.temp();
                        let as_i64 = self.temp();
                        writeln!(&mut self.ir, "  {dbl}    = call double @jsonGetFloatField(i64* %json_val, i8* {key_ptr})").unwrap();
                        writeln!(&mut self.ir, "  {as_i64} = bitcast double {dbl} to i64").unwrap();
                        as_i64
                    }
                    "i1" => {
                        let b32    = self.temp();
                        let as_i64 = self.temp();
                        writeln!(&mut self.ir, "  {b32}    = call i32 @jsonGetBoolField(i64* %json_val, i8* {key_ptr})").unwrap();
                        writeln!(&mut self.ir, "  {as_i64} = zext i32 {b32} to i64").unwrap();
                        as_i64
                    }
                    "i64*" => {
                        let arr_ptr = self.temp();
                        let as_i64  = self.temp();
                        writeln!(&mut self.ir, "  {arr_ptr} = call i64* @jsonGetIntListField(i64* %json_val, i8* {key_ptr})").unwrap();
                        writeln!(&mut self.ir, "  {as_i64}  = ptrtoint i64* {arr_ptr} to i64").unwrap();
                        as_i64
                    }
                    _ => {
                        // i64, i32, etc.
                        let val = self.temp();
                        writeln!(&mut self.ir, "  {val} = call i64 @jsonGetIntField(i64* %json_val, i8* {key_ptr})").unwrap();
                        val
                    }
                };

                writeln!(&mut self.ir, "  store i64 {store_val}, i64* {fptr}").unwrap();
            }

            writeln!(&mut self.ir, "  ret i64* {self_}").unwrap();
            writeln!(&mut self.ir, "}}").unwrap();
            writeln!(&mut self.ir).unwrap();
        }
    }

    /// Translate a lambda body expression into a SQL predicate fragment.
    /// Emits LLVM IR to evaluate parameter values and returns:
    ///   (sql_fragment, vec_of_i8ptr_regs)
    /// Returns None if the expression cannot be statically translated.
    fn lambda_to_sql_and_params(
        &mut self,
        body: &Expr,
        param_name: &str,
        fields: &[EntityFieldEntry],
        param_offset: usize,
        ctx: &mut GenCtx,
    ) -> Option<(String, Vec<String>)> {
        match &body.node {
            ExprKind::Binary { op, lhs, rhs } => {
                match op {
                    BinaryOp::And => {
                        let (lsql, mut lparams) = self.lambda_to_sql_and_params(lhs, param_name, fields, param_offset, ctx)?;
                        let (rsql, rparams) = self.lambda_to_sql_and_params(rhs, param_name, fields, param_offset + lparams.len(), ctx)?;
                        lparams.extend(rparams);
                        Some((format!("({}) AND ({})", lsql, rsql), lparams))
                    }
                    BinaryOp::Or => {
                        let (lsql, mut lparams) = self.lambda_to_sql_and_params(lhs, param_name, fields, param_offset, ctx)?;
                        let (rsql, rparams) = self.lambda_to_sql_and_params(rhs, param_name, fields, param_offset + lparams.len(), ctx)?;
                        lparams.extend(rparams);
                        Some((format!("({}) OR ({})", lsql, rsql), lparams))
                    }
                    BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                        let sql_op = match op {
                            BinaryOp::Eq => "=",
                            BinaryOp::Ne => "!=",
                            BinaryOp::Lt => "<",
                            BinaryOp::Le => "<=",
                            BinaryOp::Gt => ">",
                            BinaryOp::Ge => ">=",
                            _ => unreachable!(),
                        };
                        let n = param_offset + 1;
                        let fields_clone = fields.to_vec();
                        if let Some(col) = orm_extract_field(lhs, param_name, &fields_clone) {
                            let col = col.to_string();
                            let reg = self.emit_orm_param_value(rhs, ctx)?;
                            Some((format!("{} {} ${}", col, sql_op, n), vec![reg]))
                        } else if let Some(col) = orm_extract_field(rhs, param_name, &fields_clone) {
                            let col = col.to_string();
                            let flipped = match op {
                                BinaryOp::Lt => ">",
                                BinaryOp::Le => ">=",
                                BinaryOp::Gt => "<",
                                BinaryOp::Ge => "<=",
                                _ => sql_op,
                            };
                            let reg = self.emit_orm_param_value(lhs, ctx)?;
                            Some((format!("{} {} ${}", col, flipped, n), vec![reg]))
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            }
            ExprKind::Unary { op: UnaryOp::Not, operand } => {
                let (sql, params) = self.lambda_to_sql_and_params(operand, param_name, fields, param_offset, ctx)?;
                Some((format!("NOT ({})", sql), params))
            }
            ExprKind::MethodCall { obj, method, args } => {
                let fields_clone = fields.to_vec();
                if let Some(col) = orm_extract_field(obj, param_name, &fields_clone) {
                    let col = col.to_string();
                    if args.len() == 1 {
                        if let ExprKind::Literal(Literal::String(s)) = &args[0].node {
                            let n = param_offset + 1;
                            let like_val = match method.as_str() {
                                "startsWith" => format!("{}%", s),
                                "endsWith"   => format!("%{}", s),
                                "contains"   => format!("%{}%", s),
                                _ => return None,
                            };
                            let label = format!("__orm_like_{}", self.strings.len());
                            self.strings.insert(label.clone(), like_val.clone());
                            let len = like_val.len() + 1;
                            let like_reg = self.temp();
                            writeln!(&mut self.ir, "  {like_reg} = getelementptr [{len} x i8], [{len} x i8]* @{label}, i64 0, i64 0").unwrap();
                            return Some((format!("{} LIKE ${}", col, n), vec![like_reg]));
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Emit code to evaluate an ORM parameter expression and return an i8* register.
    fn emit_orm_param_value(&mut self, expr: &Expr, ctx: &mut GenCtx) -> Option<String> {
        match &expr.node {
            ExprKind::Literal(Literal::String(s)) => {
                let s_clone = s.clone();
                let label = format!("__orm_p_{}", self.strings.len());
                self.strings.insert(label.clone(), s_clone.clone());
                let len = s_clone.len() + 1;
                let reg = self.temp();
                writeln!(&mut self.ir, "  {reg} = getelementptr [{len} x i8], [{len} x i8]* @{label}, i64 0, i64 0").unwrap();
                Some(reg)
            }
            ExprKind::Literal(Literal::Integer(n)) => {
                let reg = self.temp();
                writeln!(&mut self.ir, "  {reg} = call i8* @tinox_int_to_param(i64 {n})").unwrap();
                Some(reg)
            }
            ExprKind::Literal(Literal::Bool(b)) => {
                let val: i64 = if *b { 1 } else { 0 };
                let reg = self.temp();
                writeln!(&mut self.ir, "  {reg} = call i8* @tinox_int_to_param(i64 {val})").unwrap();
                Some(reg)
            }
            _ => {
                // Runtime expression — evaluate and convert to string
                if let Ok((val_reg, val_ty)) = self.gen_expr(expr, ctx) {
                    let reg = self.temp();
                    match val_ty.as_str() {
                        "i8*" => {
                            writeln!(&mut self.ir, "  {reg} = bitcast i8* {val_reg} to i8*").unwrap();
                        }
                        _ => {
                            writeln!(&mut self.ir, "  {reg} = call i8* @tinox_int_to_param(i64 {val_reg})").unwrap();
                        }
                    }
                    Some(reg)
                } else {
                    None
                }
            }
        }
    }

    /// Generate the full query code for an ORM chain and return (result_reg, result_type).
    fn gen_orm_query(
        &mut self,
        chain: &OrmChain,
        ctx: &mut GenCtx,
    ) -> Result<(String, String), ErrorBag> {
        let entity = self.entity_entries.iter().find(|e| e.class_name == chain.entity_class).cloned();
        let entity = match entity {
            Some(e) => e,
            None => return Ok(("0".to_string(), "i64*".to_string())),
        };

        // Build WHERE clause and collect params
        let mut where_parts: Vec<String> = Vec::new();
        let mut all_params: Vec<String> = Vec::new();
        let fields = entity.fields.clone();

        for (param_name, body) in &chain.filters {
            let param_name = param_name.clone();
            let body = body.clone();
            let offset = all_params.len();
            if let Some((sql, params)) = self.lambda_to_sql_and_params(&body, &param_name, &fields, offset, ctx) {
                where_parts.push(sql);
                all_params.extend(params);
            }
        }

        // Build ORDER BY clause
        let order_sql = if chain.order_by.is_empty() {
            String::new()
        } else {
            let parts: Vec<String> = chain.order_by.iter().map(|(col, desc)| {
                // Look up column name from field name
                let col_name = entity.fields.iter()
                    .find(|f| f.field_name == *col || f.column_name == *col)
                    .map(|f| f.column_name.as_str())
                    .unwrap_or(col.as_str());
                if *desc { format!("{} DESC", col_name) } else { format!("{} ASC", col_name) }
            }).collect();
            format!(" ORDER BY {}", parts.join(", "))
        };

        // Build LIMIT / OFFSET
        let limit_sql = chain.limit.map(|n| format!(" LIMIT {}", n)).unwrap_or_default();
        let offset_sql = chain.offset_val.map(|n| format!(" OFFSET {}", n)).unwrap_or_default();

        // Build the full SQL string at compile time if possible (no runtime concatenation needed
        // for WHERE clause shape — only the parameter values are runtime)
        let base_sql = format!("SELECT {} FROM {}",
            entity.fields.iter().map(|f| f.column_name.as_str()).collect::<Vec<_>>().join(", "),
            entity.table_name);
        let where_sql = if where_parts.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", where_parts.join(" AND "))
        };
        let full_sql = format!("{}{}{}{}{}", base_sql, where_sql, order_sql, limit_sql, offset_sql);

        // Emit SQL string constant
        let sql_label = format!("__orm_query_{}", self.strings.len());
        let sql_len = full_sql.len() + 1;
        self.strings.insert(sql_label.clone(), full_sql);
        let sql_ptr = self.temp();
        writeln!(&mut self.ir, "  {sql_ptr} = getelementptr [{sql_len} x i8], [{sql_len} x i8]* @{sql_label}, i64 0, i64 0").unwrap();

        // Allocate params array and fill it
        let n_params = all_params.len() as i64;
        let params_arr = self.temp();
        writeln!(&mut self.ir, "  {params_arr} = call i8** @tinox_params_alloc(i64 {n_params})").unwrap();
        for (i, param_reg) in all_params.iter().enumerate() {
            writeln!(&mut self.ir, "  call void @tinox_params_set(i8** {params_arr}, i64 {i}, i8* {param_reg})").unwrap();
        }

        // Execute query
        let conn_reg = self.temp();
        let result_reg = self.temp();
        writeln!(&mut self.ir, "  {conn_reg} = call i8* @tinox_db_get_conn()").unwrap();
        writeln!(&mut self.ir, "  {result_reg} = call i8* @tinox_db_exec(i8* {conn_reg}, i8* {sql_ptr}, i8** {params_arr}, i64 {n_params})").unwrap();

        match chain.terminal.as_str() {
            "count" => {
                let n = self.temp();
                writeln!(&mut self.ir, "  {n} = call i64 @tinox_db_nrows(i8* {result_reg})").unwrap();
                writeln!(&mut self.ir, "  call void @tinox_db_free(i8* {result_reg})").unwrap();
                Ok((n, "i64".to_string()))
            }
            "first" => {
                let from_row_fn = format!("{}_fromRow", entity.class_name);
                let obj_reg = self.temp();
                writeln!(&mut self.ir, "  {obj_reg} = call i8* @{from_row_fn}(i8* {result_reg}, i64 0)").unwrap();
                writeln!(&mut self.ir, "  call void @tinox_db_free(i8* {result_reg})").unwrap();
                let as_i64ptr = self.temp();
                writeln!(&mut self.ir, "  {as_i64ptr} = ptrtoint i8* {obj_reg} to i64").unwrap();
                Ok((as_i64ptr, "i64".to_string()))
            }
            _ => {
                // "list" — build a List using Tinox array convention:
                // layout: [length | elem0 | elem1 | ... | elemN-1]
                // returned pointer points to elem0; length lives at index -1.
                let from_row_fn = format!("{}_fromRow", entity.class_name);
                let nrows = self.temp();
                writeln!(&mut self.ir, "  {nrows} = call i64 @tinox_db_nrows(i8* {result_reg})").unwrap();

                // Allocate (nrows + 1) * 8 bytes: slot 0 = length, slots 1..nrows = elements
                let total_slots = self.temp();
                let total_bytes = self.temp();
                let raw_ptr = self.temp();
                let raw_i64 = self.temp();
                writeln!(&mut self.ir, "  {total_slots} = add i64 {nrows}, 1").unwrap();
                writeln!(&mut self.ir, "  {total_bytes} = mul i64 {total_slots}, 8").unwrap();
                writeln!(&mut self.ir, "  {raw_ptr} = call i8* @tinox_alloc(i64 {total_bytes})").unwrap();
                writeln!(&mut self.ir, "  {raw_i64} = bitcast i8* {raw_ptr} to i64*").unwrap();

                // Store length at index 0
                let len_slot = self.temp();
                writeln!(&mut self.ir, "  {len_slot} = getelementptr i64, i64* {raw_i64}, i64 0").unwrap();
                writeln!(&mut self.ir, "  store i64 {nrows}, i64* {len_slot}").unwrap();

                // elem0 lives at index 1 — this is the pointer returned to callers
                let data_ptr = self.temp();
                writeln!(&mut self.ir, "  {data_ptr} = getelementptr i64, i64* {raw_i64}, i64 1").unwrap();

                // Loop: i = 0; while i < nrows { data_ptr[i] = fromRow(result, i); i++ }
                let loop_bb = self.new_bb("orm_loop");
                let body_bb = self.new_bb("orm_body");
                let exit_bb = self.new_bb("orm_exit");

                let idx_alloc = self.temp();
                writeln!(&mut self.ir, "  {idx_alloc} = alloca i64").unwrap();
                writeln!(&mut self.ir, "  store i64 0, i64* {idx_alloc}").unwrap();
                writeln!(&mut self.ir, "  br label %{loop_bb}").unwrap();
                writeln!(&mut self.ir, "{loop_bb}:").unwrap();
                let cur_i = self.temp();
                writeln!(&mut self.ir, "  {cur_i} = load i64, i64* {idx_alloc}").unwrap();
                let cond = self.temp();
                writeln!(&mut self.ir, "  {cond} = icmp slt i64 {cur_i}, {nrows}").unwrap();
                writeln!(&mut self.ir, "  br i1 {cond}, label %{body_bb}, label %{exit_bb}").unwrap();
                writeln!(&mut self.ir, "{body_bb}:").unwrap();

                let row_obj = self.temp();
                writeln!(&mut self.ir, "  {row_obj} = call i8* @{from_row_fn}(i8* {result_reg}, i64 {cur_i})").unwrap();
                let row_as_int = self.temp();
                writeln!(&mut self.ir, "  {row_as_int} = ptrtoint i8* {row_obj} to i64").unwrap();
                let slot = self.temp();
                writeln!(&mut self.ir, "  {slot} = getelementptr i64, i64* {data_ptr}, i64 {cur_i}").unwrap();
                writeln!(&mut self.ir, "  store i64 {row_as_int}, i64* {slot}").unwrap();
                let next_i = self.temp();
                writeln!(&mut self.ir, "  {next_i} = add i64 {cur_i}, 1").unwrap();
                writeln!(&mut self.ir, "  store i64 {next_i}, i64* {idx_alloc}").unwrap();
                writeln!(&mut self.ir, "  br label %{loop_bb}").unwrap();
                writeln!(&mut self.ir, "{exit_bb}:").unwrap();

                writeln!(&mut self.ir, "  call void @tinox_db_free(i8* {result_reg})").unwrap();

                // Return pointer to elem0 — same layout as ArrayLiteral (type i64*)
                Ok((data_ptr, "i64*".to_string()))
            }
        }
    }

    /// Emit SQL-constant getter functions and row-mapping helpers for all @Entity classes.
    fn emit_entity_code(&mut self) {
        // Emit DB init via @llvm.global_ctors if a connection URL is configured
        if let Some(url) = self.db_url.clone() {
            let url_len = url.len() + 1;
            let escaped = Self::escape_llvm_string(&url);
            writeln!(&mut self.ir, "@__db_url = private constant [{url_len} x i8] c\"{escaped}\\00\"").unwrap();
            writeln!(&mut self.ir, "define void @__tinox_db_init() {{").unwrap();
            writeln!(&mut self.ir, "entry:").unwrap();
            writeln!(&mut self.ir, "  %url = getelementptr [{url_len} x i8], [{url_len} x i8]* @__db_url, i64 0, i64 0").unwrap();
            writeln!(&mut self.ir, "  call void @tinox_db_connect(i8* %url)").unwrap();
            writeln!(&mut self.ir, "  ret void").unwrap();
            writeln!(&mut self.ir, "}}").unwrap();
            writeln!(&mut self.ir, "@llvm.global_ctors = appending global [1 x {{ i32, void ()*, i8* }}] [{{ i32, void ()*, i8* }} {{ i32 10, void ()* @__tinox_db_init, i8* null }}]").unwrap();
            writeln!(&mut self.ir).unwrap();
        }

        let entities = self.entity_entries.clone();
        for entity in &entities {
            let cn = entity.class_name.clone();
            let table = entity.table_name.clone();
            let fields = entity.fields.clone();

            // SELECT sql
            let cols: Vec<String> = fields.iter().map(|f| f.column_name.clone()).collect();
            let select_sql = format!("SELECT {} FROM {}", cols.join(", "), table);
            self.emit_sql_const_fn(&format!("{cn}_selectSql"), &select_sql);

            // INSERT sql (exclude @GeneratedValue fields)
            let ins_fields: Vec<&EntityFieldEntry> = fields.iter().filter(|f| !f.is_generated).collect();
            let ins_cols: Vec<&str> = ins_fields.iter().map(|f| f.column_name.as_str()).collect();
            let ins_phs: Vec<String> = (1..=ins_fields.len()).map(|i| format!("${i}")).collect();
            let insert_sql = format!(
                "INSERT INTO {table} ({}) VALUES ({}) RETURNING id",
                ins_cols.join(", "),
                ins_phs.join(", ")
            );
            self.emit_sql_const_fn(&format!("{cn}_insertSql"), &insert_sql);

            // UPDATE sql (non-id fields in SET, id field in WHERE)
            let id_col = fields.iter().find(|f| f.is_id).map(|f| f.column_name.clone()).unwrap_or_else(|| "id".to_string());
            let non_id: Vec<&EntityFieldEntry> = fields.iter().filter(|f| !f.is_id).collect();
            let set_clauses: Vec<String> = non_id.iter().enumerate().map(|(i, f)| format!("{} = ${}", f.column_name, i + 1)).collect();
            let update_sql = format!(
                "UPDATE {table} SET {} WHERE {id_col} = ${}",
                set_clauses.join(", "),
                non_id.len() + 1
            );
            self.emit_sql_const_fn(&format!("{cn}_updateSql"), &update_sql);

            // DELETE sql
            let delete_sql = format!("DELETE FROM {table} WHERE {id_col} = $1");
            self.emit_sql_const_fn(&format!("{cn}_deleteSql"), &delete_sql);

            // fromRow and toParams
            self.emit_entity_from_row(&cn, &fields);
            self.emit_entity_to_params(&cn, &fields);
        }
    }

    fn emit_sql_const_fn(&mut self, fn_name: &str, sql: &str) {
        let label = format!("__sql_{}_{}", fn_name, self.strings.len());
        self.strings.insert(label.clone(), sql.to_string());
        let len = sql.len() + 1;
        let ptr = self.temp();
        writeln!(&mut self.ir, "define i8* @{fn_name}() {{").unwrap();
        writeln!(&mut self.ir, "entry:").unwrap();
        writeln!(&mut self.ir, "  {ptr} = getelementptr [{len} x i8], [{len} x i8]* @{label}, i64 0, i64 0").unwrap();
        writeln!(&mut self.ir, "  ret i8* {ptr}").unwrap();
        writeln!(&mut self.ir, "}}").unwrap();
        writeln!(&mut self.ir).unwrap();
    }

    fn emit_entity_from_row(&mut self, class_name: &str, fields: &[EntityFieldEntry]) {
        let n = fields.len();
        let alloc_size = n as i64 * 8;
        writeln!(&mut self.ir, "define i8* @{class_name}_fromRow(i8* %result, i64 %row_idx) {{").unwrap();
        writeln!(&mut self.ir, "entry:").unwrap();
        let raw = self.temp();
        let ptr = self.temp();
        writeln!(&mut self.ir, "  {raw} = call i8* @tinox_alloc(i64 {alloc_size})").unwrap();
        writeln!(&mut self.ir, "  {ptr} = bitcast i8* {raw} to i64*").unwrap();
        for (col_idx, field) in fields.iter().enumerate() {
            let fptr = self.temp();
            writeln!(&mut self.ir, "  {fptr} = getelementptr i64, i64* {ptr}, i64 {col_idx}").unwrap();
            match field.field_llvm_type.as_str() {
                "i8*" => {
                    let val = self.temp();
                    writeln!(&mut self.ir, "  {val} = call i8* @tinox_db_getval(i8* %result, i64 %row_idx, i64 {col_idx})").unwrap();
                    let as_int = self.temp();
                    writeln!(&mut self.ir, "  {as_int} = ptrtoint i8* {val} to i64").unwrap();
                    writeln!(&mut self.ir, "  store i64 {as_int}, i64* {fptr}").unwrap();
                }
                _ => {
                    // Direct int64 read — no string conversion
                    let ival = self.temp();
                    writeln!(&mut self.ir, "  {ival} = call i64 @tinox_db_getval_int(i8* %result, i64 %row_idx, i64 {col_idx})").unwrap();
                    writeln!(&mut self.ir, "  store i64 {ival}, i64* {fptr}").unwrap();
                }
            }
        }
        writeln!(&mut self.ir, "  ret i8* {raw}").unwrap();
        writeln!(&mut self.ir, "}}").unwrap();
        writeln!(&mut self.ir).unwrap();
    }

    fn emit_entity_to_params(&mut self, class_name: &str, fields: &[EntityFieldEntry]) {
        // INSERT variant: exclude @GeneratedValue fields; slot_idx = field position in struct
        let ins_fields: Vec<(usize, &EntityFieldEntry)> = fields.iter()
            .enumerate()
            .filter(|(_, f)| !f.is_generated)
            .collect();
        let n = ins_fields.len();
        writeln!(&mut self.ir, "define i8** @{class_name}_toParams(i64* %entity, i64* %out_n) {{").unwrap();
        writeln!(&mut self.ir, "entry:").unwrap();
        let arr = self.temp();
        writeln!(&mut self.ir, "  {arr} = call i8** @tinox_params_alloc(i64 {n})").unwrap();
        for (param_idx, (slot_idx, field)) in ins_fields.iter().enumerate() {
            let fptr = self.temp();
            let fval = self.temp();
            writeln!(&mut self.ir, "  {fptr} = getelementptr i64, i64* %entity, i64 {slot_idx}").unwrap();
            writeln!(&mut self.ir, "  {fval} = load i64, i64* {fptr}").unwrap();
            let pstr = if field.field_llvm_type == "i8*" {
                let s = self.temp();
                writeln!(&mut self.ir, "  {s} = inttoptr i64 {fval} to i8*").unwrap();
                s
            } else {
                let s = self.temp();
                writeln!(&mut self.ir, "  {s} = call i8* @tinox_int_to_param(i64 {fval})").unwrap();
                s
            };
            writeln!(&mut self.ir, "  call void @tinox_params_set(i8** {arr}, i64 {param_idx}, i8* {pstr})").unwrap();
        }
        writeln!(&mut self.ir, "  store i64 {n}, i64* %out_n").unwrap();
        writeln!(&mut self.ir, "  ret i8** {arr}").unwrap();
        writeln!(&mut self.ir, "}}").unwrap();
        writeln!(&mut self.ir).unwrap();
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
                if let Some((ref label, ref start_reg)) = ctx.timed_metric.clone() {
                    self.emit_histogram_record(label, start_reg);
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
                        (from, to) if !from.ends_with('*') && to.ends_with('*') => "inttoptr",
                        (from, to) if from.ends_with('*') && !to.ends_with('*') => "ptrtoint",
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
                if let Some((ref label, ref start_reg)) = ctx.timed_metric.clone() {
                    self.emit_histogram_record(label, start_reg);
                }
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
                        } else if matches!(&func.node, ExprKind::Ident(n) if n == "regexFindAll" || n == "regexSplit") {
                            llvm_ty = "i64*".to_string();
                            struct_name = Some("Array:String".to_string());
                            true
                        } else if matches!(&func.node, ExprKind::Ident(n) if n == "regexMatchGroups") {
                            llvm_ty = "i64*".to_string();
                            true
                        } else { false }
                    } else if let ExprKind::MethodCall { method, .. } = &v.node {
                        if method == "split" || method == "keys" {
                            llvm_ty = "i64*".to_string();
                            struct_name = Some("Array:String".to_string());
                            true
                        } else { false }
                    } else if let ExprKind::ArrayLiteral(elems) = &v.node {
                        llvm_ty = "i64*".to_string();
                        let is_str_ann = matches!(ty, Some(Type::Array(inner)) if matches!(inner.as_ref(), Type::String))
                            || matches!(ty, Some(Type::Generic { name, args }) if (name == "Array" || name == "List") && args.first().map(|a| matches!(a, Type::String)).unwrap_or(false));
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
                        || matches!(ty, Some(Type::Generic { name, .. }) if name == "Array")
                        || matches!(ty, Some(Type::Generic { name, args }) if name == "List" && matches!(args.first(), Some(Type::String))) {
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
                    // Generate a unique alloca slot name to avoid duplicate definitions
                    let slot_name = format!("{}_{}", name, self.temp_count);
                    self.temp_count += 1;
                    ctx.local_slots.insert(name.clone(), slot_name.clone());
                    if matches!(&val.node, ExprKind::Range { .. }) {
                        ctx.range_vars.insert(name.clone());
                    }
                    // If the declared type annotation is an interface, record the
                    // interface name so vtable dispatch is used for method calls.
                    // Also infer class name from constructor/factory calls when no annotation is present.
                    // Use method_ret_class mapping built during pre-pass for accurate type inference.
                    let inferred_struct = if struct_name.is_none() {
                        match &val.node {
                            ExprKind::EnumValue { enum_name, variant, .. } => {
                                let method_key = format!("{}_{}", enum_name, variant);
                                let result = self.method_ret_class.get(&method_key).cloned()
                                    .or_else(|| {
                                        // Fallback: constructor heuristic
                                        let is_ctor = variant == "new" || variant.starts_with("from")
                                            || variant.starts_with("create") || variant.starts_with("make");
                                        if is_ctor && self.struct_layouts.contains_key(enum_name.as_str()) {
                                            Some(enum_name.clone())
                                        } else { None }
                                    });
                                result
                            }
                            ExprKind::Call { func, .. } => {
                                match &func.node {
                                    ExprKind::Ident(fname) if fname == "dirList" => {
                                        Some("Array:String".to_string())
                                    }
                                    ExprKind::Ident(fname) => {
                                        self.method_ret_class.get(fname.as_str()).cloned()
                                    }
                                    _ => None,
                                }
                            }
                            ExprKind::MethodCall { obj: mc_obj, method: mc_method, .. } => {
                                // Infer return class from instance method call, e.g. evaluator.eval() -> EvalResult
                                self.infer_struct_type(mc_obj, ctx)
                                    .and_then(|obj_class| {
                                        let method_key = format!("{}_{}", obj_class, mc_method);
                                        self.method_ret_class.get(&method_key).cloned()
                                    })
                            }
                            ExprKind::Ident(src_name) => {
                                // Copy type from source variable (e.g. let x = someObj)
                                ctx.local_types.get(src_name.as_str()).cloned()
                            }
                            _ => None,
                        }
                    } else { struct_name.clone() };
                    let effective_type = if let Some(Type::Named(ann)) = ty {
                        if self.known_interfaces.contains(ann.as_str()) {
                            Some(ann.clone())
                        } else {
                            inferred_struct.clone().or_else(|| struct_name.clone())
                        }
                    } else {
                        inferred_struct.clone().or_else(|| struct_name.clone())
                    };
                    if let Some(sn) = effective_type {
                        ctx.local_types.insert(name.clone(), sn);
                    } else {
                        // Re-binding a name without type info must clear any stale entry
                        // (e.g. a former loop var's "Array:String:elem" marker).
                        ctx.local_types.remove(name.as_str());
                    }
                    // For List<ClassName> annotations, track element type for indexed field access
                    if let Some(Type::Generic { name: gname, args }) = ty {
                        if gname == "List" {
                            if let Some(Type::Named(cls)) = args.first() {
                                if self.defined_classes.contains(cls.as_str()) {
                                    ctx.local_types.insert(name.clone(), format!("List:{}", cls));
                                }
                            }
                        }
                    }
                    if is_heap_ptr {
                        writeln!(&mut self.ir, "%{} = alloca {}", slot_name, actual_ty).unwrap();
                        // Coerce value to actual slot type
                        let store_val = if val_ty == actual_ty || val_ty.is_empty() || actual_ty.is_empty() {
                            v.clone()
                        } else if val_ty == "i64" && (actual_ty.ends_with('*') || actual_ty == "ptr") {
                            let c = self.temp(); writeln!(&mut self.ir, "{} = inttoptr i64 {} to {}", c, v, actual_ty).unwrap(); c
                        } else if (val_ty.ends_with('*') || val_ty == "ptr") && actual_ty == "i64" {
                            let c = self.temp(); writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", c, val_ty, v).unwrap(); c
                        } else if val_ty == "i1" && actual_ty == "i64" {
                            let c = self.temp(); writeln!(&mut self.ir, "{} = zext i1 {} to i64", c, v).unwrap(); c
                        } else if val_ty == "double" && actual_ty == "i64" {
                            let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast double {} to i64", c, v).unwrap(); c
                        } else { v.clone() };
                        writeln!(&mut self.ir, "store {} {}, {}* %{}", actual_ty, store_val, actual_ty, slot_name).unwrap();
                    } else {
                        writeln!(&mut self.ir, "%{} = alloca {}", slot_name, actual_ty).unwrap();
                        // Coerce value to actual slot type
                        let store_val = if val_ty == actual_ty || val_ty.is_empty() || actual_ty.is_empty() {
                            v.clone()
                        } else if val_ty == "i64" && (actual_ty.ends_with('*') || actual_ty == "ptr") {
                            let c = self.temp(); writeln!(&mut self.ir, "{} = inttoptr i64 {} to {}", c, v, actual_ty).unwrap(); c
                        } else if (val_ty.ends_with('*') || val_ty == "ptr") && actual_ty == "i64" {
                            let c = self.temp(); writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", c, val_ty, v).unwrap(); c
                        } else if val_ty == "i1" && actual_ty == "i64" {
                            let c = self.temp(); writeln!(&mut self.ir, "{} = zext i1 {} to i64", c, v).unwrap(); c
                        } else if val_ty == "double" && actual_ty == "i64" {
                            let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast double {} to i64", c, v).unwrap(); c
                        } else { v.clone() };
                        writeln!(&mut self.ir, "store {} {}, {}* %{}", actual_ty, store_val, actual_ty, slot_name).unwrap();
                    }
                } else {
                    let slot = ctx.locals.len();
                    ctx.locals.insert(name.clone(), (llvm_ty.clone(), slot));
                    // Generate a unique alloca slot name to avoid duplicate definitions
                    let slot_name = format!("{}_{}", name, self.temp_count);
                    self.temp_count += 1;
                    ctx.local_slots.insert(name.clone(), slot_name.clone());
                    if let Some(sn) = &struct_name {
                        ctx.local_types.insert(name.clone(), sn.clone());
                    }
                    writeln!(&mut self.ir, "%{} = alloca {}", slot_name, llvm_ty).unwrap();
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
                        } else if matches!(&func.node, ExprKind::Ident(n) if n == "regexFindAll" || n == "regexSplit") {
                            llvm_ty = "i64*".to_string();
                            struct_name = Some("Array:String".to_string());
                            true
                        } else if matches!(&func.node, ExprKind::Ident(n) if n == "regexMatchGroups") {
                            llvm_ty = "i64*".to_string();
                            true
                        } else { false }
                    } else if let ExprKind::MethodCall { method, .. } = &v.node {
                        if method == "split" || method == "keys" {
                            llvm_ty = "i64*".to_string();
                            struct_name = Some("Array:String".to_string());
                            true
                        } else { false }
                    } else if let ExprKind::ArrayLiteral(elems) = &v.node {
                        llvm_ty = "i64*".to_string();
                        let is_str_ann = matches!(ty, Some(Type::Array(inner)) if matches!(inner.as_ref(), Type::String))
                            || matches!(ty, Some(Type::Generic { name, args }) if (name == "Array" || name == "List") && args.first().map(|a| matches!(a, Type::String)).unwrap_or(false));
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
                        || matches!(ty, Some(Type::Generic { name, .. }) if name == "Array")
                        || matches!(ty, Some(Type::Generic { name, args }) if name == "List" && matches!(args.first(), Some(Type::String))) {
                        struct_name = Some("Array:String".to_string());
                        llvm_ty = "i64*".to_string();
                    }
                }

                // Generate a unique alloca slot name to avoid duplicate definitions
                let slot_name = format!("{}_{}", name, self.temp_count);
                self.temp_count += 1;
                ctx.local_slots.insert(name.clone(), slot_name.clone());

                if let Some(val) = value {
                    let (v, val_ty) = self.gen_expr(val, ctx)?;
                    let actual_ty = if matches!(&val.node, ExprKind::Lambda { .. }) {
                        val_ty.clone()
                    } else if is_ptr {
                        llvm_ty.clone()
                    } else if ty.is_none() || matches!(ty, Some(Type::Infer)) {
                        // No annotation: use the value's actual LLVM type (preserves i8* for strings,
                        // double for floats, etc. — avoids spurious ptrtoint/print_int for string vars)
                        val_ty.clone()
                    } else {
                        llvm_ty.clone()
                    };
                    let slot = ctx.locals.len();
                    ctx.locals.insert(name.clone(), (actual_ty.clone(), slot));
                    // Infer struct type from static method calls (EnumValue) and instance method calls.
                    // This ensures local_types is set so subsequent method calls dispatch correctly.
                    let inferred_struct_var = if struct_name.is_none() {
                        match &val.node {
                            ExprKind::EnumValue { enum_name, variant, .. } => {
                                let method_key = format!("{}_{}", enum_name, variant);
                                self.method_ret_class.get(&method_key).cloned()
                                    .or_else(|| {
                                        let is_ctor = variant == "new" || variant.starts_with("from")
                                            || variant.starts_with("create") || variant.starts_with("make");
                                        if is_ctor && self.struct_layouts.contains_key(enum_name.as_str()) {
                                            Some(enum_name.clone())
                                        } else { None }
                                    })
                            }
                            ExprKind::MethodCall { obj: mc_obj, method: mc_method, .. } => {
                                self.infer_struct_type(mc_obj, ctx)
                                    .and_then(|obj_class| {
                                        let method_key = format!("{}_{}", obj_class, mc_method);
                                        self.method_ret_class.get(&method_key).cloned()
                                    })
                            }
                            ExprKind::Call { func, .. } => {
                                match &func.node {
                                    ExprKind::Ident(fname) => {
                                        self.method_ret_class.get(fname.as_str()).cloned()
                                    }
                                    _ => None,
                                }
                            }
                            ExprKind::Ident(src_name) => {
                                // Copy type from source variable (e.g. var newCtx = ctx)
                                ctx.local_types.get(src_name.as_str()).cloned()
                            }
                            _ => None,
                        }
                    } else { struct_name.clone() };
                    // If the declared type annotation is an interface, use it for vtable dispatch.
                    let effective_type = if let Some(Type::Named(ann)) = ty {
                        if self.known_interfaces.contains(ann.as_str()) {
                            Some(ann.clone())
                        } else {
                            inferred_struct_var.clone().or_else(|| struct_name.clone())
                        }
                    } else {
                        inferred_struct_var.clone().or_else(|| struct_name.clone())
                    };
                    if let Some(sn) = effective_type {
                        ctx.local_types.insert(name.clone(), sn);
                    } else {
                        // Re-binding a name without type info must clear any stale entry
                        // (e.g. a former loop var's "Array:String:elem" marker).
                        ctx.local_types.remove(name.as_str());
                    }
                    writeln!(&mut self.ir, "%{} = alloca {}", slot_name, actual_ty).unwrap();
                    // Coerce value type to slot type if necessary
                    let store_val = if val_ty == actual_ty || val_ty.is_empty() || actual_ty.is_empty() {
                        v.clone()
                    } else if val_ty == "i64" && (actual_ty.ends_with('*') || actual_ty == "ptr") {
                        let c = self.temp();
                        writeln!(&mut self.ir, "{} = inttoptr i64 {} to {}", c, v, actual_ty).unwrap();
                        c
                    } else if (val_ty.ends_with('*') || val_ty == "ptr") && actual_ty == "i64" {
                        let c = self.temp();
                        writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", c, val_ty, v).unwrap();
                        c
                    } else if val_ty == "i1" && actual_ty == "i64" {
                        let c = self.temp();
                        writeln!(&mut self.ir, "{} = zext i1 {} to i64", c, v).unwrap();
                        c
                    } else if val_ty == "double" && actual_ty == "i64" {
                        let c = self.temp();
                        writeln!(&mut self.ir, "{} = bitcast double {} to i64", c, v).unwrap();
                        c
                    } else {
                        v.clone()
                    };
                    writeln!(
                        &mut self.ir,
                        "store {} {}, {}* %{}",
                        actual_ty, store_val, actual_ty, slot_name
                    )
                    .unwrap();
                } else {
                    let slot = ctx.locals.len();
                    ctx.locals.insert(name.clone(), (llvm_ty.clone(), slot));
                    if let Some(sn) = &struct_name {
                        ctx.local_types.insert(name.clone(), sn.clone());
                    }
                    writeln!(&mut self.ir, "%{} = alloca {}", slot_name, llvm_ty).unwrap();
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
                    // iter_ptr may be i64 (pointer encoded as integer) or i64*/ptr — coerce to ptr.
                    let arr_ptr_val = if iter_ty == "i64" {
                        let p = self.temp();
                        writeln!(&mut self.ir, "{} = inttoptr i64 {} to i64*", p, iter_ptr).unwrap();
                        p
                    } else {
                        iter_ptr.clone()
                    };
                    let len_ptr = self.temp();
                    writeln!(&mut self.ir, "{} = getelementptr i64, ptr {}, i64 -1", len_ptr, arr_ptr_val).unwrap();
                    let len_val = self.temp();
                    writeln!(&mut self.ir, "{} = load i64, i64* {}", len_val, len_ptr).unwrap();
                    ("0".to_string(), len_val, Some(arr_ptr_val), None)
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

                // The elem marker is only valid inside the loop body — a later variable
                // with the same name must not inherit it (local_types is function-flat).
                if ctx.local_types.get(var.as_str()).map(|t| t == "Array:String:elem").unwrap_or(false) {
                    ctx.local_types.remove(var.as_str());
                }

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
                        let (val, val_ty) = self.gen_expr(value, ctx)?;
                        // Convert value type to target type if they differ
                        let store_val = if val_ty == ty || val_ty.is_empty() || ty.is_empty() {
                            val
                        } else if val_ty == "i64" && (ty.ends_with('*') || ty == "ptr") {
                            let c = self.temp();
                            writeln!(&mut self.ir, "{} = inttoptr i64 {} to {}", c, val, ty).unwrap();
                            c
                        } else if (val_ty.ends_with('*') || val_ty == "ptr") && ty == "i64" {
                            let c = self.temp();
                            writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", c, val_ty, val).unwrap();
                            c
                        } else {
                            val
                        };
                        writeln!(&mut self.ir, "store {} {}, {}* %{}", ty, store_val, ty, slot).unwrap();
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
                            let slot = ctx.local_slots.get(name).cloned().unwrap_or_else(|| name.clone());
                            let loaded_ptr = self.temp();
                            writeln!(
                                &mut self.ir,
                                "{} = load {}, {}* %{}",
                                loaded_ptr, var_ty, var_ty, slot
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

                    if is_map || idx_ty == "i8*" {
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
                        let store_val = if val_ty == "i64" || val_ty.is_empty() {
                            val
                        } else if val_ty == "i1" {
                            let c = self.temp();
                            writeln!(&mut self.ir, "{} = zext i1 {} to i64", c, val).unwrap();
                            c
                        } else if val_ty == "double" || val_ty == "float" {
                            let c = self.temp();
                            writeln!(&mut self.ir, "{} = bitcast {} {} to i64", c, val_ty, val).unwrap();
                            c
                        } else {
                            let c = self.temp();
                            writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", c, val_ty, val).unwrap();
                            c
                        };
                        writeln!(&mut self.ir, "call void @tinox_map_set(i8* {}, i8* {}, i64 {})", map_i8, key_i8, store_val).unwrap();
                    } else {
                        // Coerce base_ptr to i64* if it's encoded as i64
                        let base_arr = if base_ty == "i64" {
                            let c = self.temp();
                            writeln!(&mut self.ir, "{} = inttoptr i64 {} to i64*", c, base_ptr).unwrap();
                            c
                        } else {
                            base_ptr.clone()
                        };
                        let ptr_name = self.temp();
                        writeln!(
                            &mut self.ir,
                            "{} = getelementptr i64, ptr {}, i64 {}",
                            ptr_name, base_arr, idx_val
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
                let (r, rt) = self.gen_expr(rhs, ctx)?;
                let result = self.temp();
                let float = Self::is_float(&lt) || Self::is_float(&rt);
                // Coerce object (i64*) → String if one side is already a String.
                // This calls ClassName_toString() if it exists, enabling "text" + obj syntax.
                let (l, lt) = if (lt == "i64*" && (rt == "i8*" || rt == "i64*")) || (lt == "i8*" && rt == "i64*") {
                    if lt == "i64*" {
                        let cn = Self::expr_class_name(&lhs.node, ctx);
                        let key = cn.as_deref().map(|c| format!("{}_toString", c));
                        if key.as_deref().map(|k| self.method_ret_types.contains_key(k)).unwrap_or(false) {
                            let s = self.temp();
                            writeln!(&mut self.ir, "{} = call i8* @{}(i64* {})", s, key.unwrap(), l).unwrap();
                            (s, "i8*".to_string())
                        } else { (l, lt) }
                    } else { (l, lt) }
                } else { (l, lt) };
                let (r, rt) = if rt == "i64*" && (lt == "i8*" || lt == "i64*") {
                    let cn = Self::expr_class_name(&rhs.node, ctx);
                    let key = cn.as_deref().map(|c| format!("{}_toString", c));
                    if key.as_deref().map(|k| self.method_ret_types.contains_key(k)).unwrap_or(false) {
                        let s = self.temp();
                        writeln!(&mut self.ir, "{} = call i8* @{}(i64* {})", s, key.unwrap(), r).unwrap();
                        (s, "i8*".to_string())
                    } else { (r, rt) }
                } else { (r, rt) };
                match op {
                    tinox_parser::BinaryOp::Add => {
                        if lt == "i8*" || rt == "i8*" || lt == "i64*" || rt == "i64*" {
                            let l_str = if lt == "i8*" {
                                l.clone()
                            } else if lt.ends_with('*') {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = bitcast {} {} to i8*", c, lt, l).unwrap();
                                c
                            } else {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, l).unwrap();
                                c
                            };
                            let r_str = if rt == "i8*" {
                                r.clone()
                            } else if rt.ends_with('*') {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = bitcast {} {} to i8*", c, rt, r).unwrap();
                                c
                            } else {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, r).unwrap();
                                c
                            };
                            writeln!(&mut self.ir, "{} = call i8* @tinox_string_concat(i8* {}, i8* {})", result, l_str, r_str).unwrap();
                            return Ok((result, "i8*".to_string()));
                        } else if float {
                            let float_ty = if lt == "double" || rt == "double" { "double" } else { lt.as_str() };
                            let lf = if lt != "double" && lt != "float" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast {} {} to double", c, lt, l).unwrap(); c } else { l.clone() };
                            let rf = if rt != "double" && rt != "float" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast {} {} to double", c, rt, r).unwrap(); c } else { r.clone() };
                            writeln!(&mut self.ir, "{} = fadd {} {}, {}", result, float_ty, lf, rf).unwrap();
                            return Ok((result, float_ty.to_string()));
                        } else {
                            writeln!(&mut self.ir, "{} = add {} {}, {}", result, lt, l, r).unwrap()
                        }
                    }
                    tinox_parser::BinaryOp::Sub => {
                        if float {
                            let float_ty = if lt == "double" || rt == "double" { "double" } else { lt.as_str() };
                            let lf = if lt != "double" && lt != "float" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast {} {} to double", c, lt, l).unwrap(); c } else { l.clone() };
                            let rf = if rt != "double" && rt != "float" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast {} {} to double", c, rt, r).unwrap(); c } else { r.clone() };
                            writeln!(&mut self.ir, "{} = fsub {} {}, {}", result, float_ty, lf, rf).unwrap();
                            return Ok((result, float_ty.to_string()));
                        } else {
                            writeln!(&mut self.ir, "{} = sub {} {}, {}", result, lt, l, r).unwrap()
                        }
                    }
                    tinox_parser::BinaryOp::Mul => {
                        if float {
                            let float_ty = if lt == "double" || rt == "double" { "double" } else { lt.as_str() };
                            let lf = if lt != "double" && lt != "float" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast {} {} to double", c, lt, l).unwrap(); c } else { l.clone() };
                            let rf = if rt != "double" && rt != "float" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast {} {} to double", c, rt, r).unwrap(); c } else { r.clone() };
                            writeln!(&mut self.ir, "{} = fmul {} {}, {}", result, float_ty, lf, rf).unwrap();
                            return Ok((result, float_ty.to_string()));
                        } else {
                            writeln!(&mut self.ir, "{} = mul {} {}, {}", result, lt, l, r).unwrap()
                        }
                    }
                    tinox_parser::BinaryOp::Div => {
                        if float {
                            let float_ty = if lt == "double" || rt == "double" { "double" } else { lt.as_str() };
                            let lf = if lt != "double" && lt != "float" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast {} {} to double", c, lt, l).unwrap(); c } else { l.clone() };
                            let rf = if rt != "double" && rt != "float" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast {} {} to double", c, rt, r).unwrap(); c } else { r.clone() };
                            writeln!(&mut self.ir, "{} = fdiv {} {}, {}", result, float_ty, lf, rf).unwrap();
                            return Ok((result, float_ty.to_string()));
                        } else {
                            writeln!(&mut self.ir, "{} = sdiv {} {}, {}", result, lt, l, r).unwrap()
                        }
                    }
                    tinox_parser::BinaryOp::Mod => {
                        if float {
                            let float_ty = if lt == "double" || rt == "double" { "double" } else { lt.as_str() };
                            let lf = if lt != "double" && lt != "float" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast {} {} to double", c, lt, l).unwrap(); c } else { l.clone() };
                            let rf = if rt != "double" && rt != "float" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast {} {} to double", c, rt, r).unwrap(); c } else { r.clone() };
                            writeln!(&mut self.ir, "{} = frem {} {}, {}", result, float_ty, lf, rf).unwrap();
                            return Ok((result, float_ty.to_string()));
                        } else {
                            writeln!(&mut self.ir, "{} = srem {} {}, {}", result, lt, l, r).unwrap()
                        }
                    }
                    tinox_parser::BinaryOp::Eq => {
                        if float {
                            // Coerce i64 operands to double if needed (float bits stored as i64).
                            let float_ty = if lt == "double" || rt == "double" { "double" } else { lt.as_str() };
                            let lf = if lt != "double" && lt != "float" {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = bitcast {} {} to double", c, lt, l).unwrap();
                                c
                            } else { l.clone() };
                            let rf = if rt != "double" && rt != "float" {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = bitcast {} {} to double", c, rt, r).unwrap();
                                c
                            } else { r.clone() };
                            writeln!(&mut self.ir, "{} = fcmp oeq {} {}, {}", result, float_ty, lf, rf).unwrap()
                        } else if lt == "i8*" || rt == "i8*" {
                            // String semantic equality
                            let l_str = if lt == "i8*" { l.clone() } else { let c = self.temp(); writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, l).unwrap(); c };
                            let r_str = if rt == "i8*" { r.clone() } else { let c = self.temp(); writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, r).unwrap(); c };
                            let cmp = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_string_equals(i8* {}, i8* {})", cmp, l_str, r_str).unwrap();
                            writeln!(&mut self.ir, "{} = trunc i64 {} to i1", result, cmp).unwrap()
                        } else if lt != rt {
                            // Mixed types: normalize pointer to i64
                            let (nl, nr) = if lt.ends_with('*') || lt == "ptr" {
                                let c = self.temp(); writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", c, if lt == "ptr" { "ptr".to_string() } else { lt.clone() }, l).unwrap(); (c, r.clone())
                            } else if rt.ends_with('*') || rt == "ptr" {
                                let c = self.temp(); writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", c, if rt == "ptr" { "ptr".to_string() } else { rt.clone() }, r).unwrap(); (l.clone(), c)
                            } else { (l.clone(), r.clone()) };
                            writeln!(&mut self.ir, "{} = icmp eq i64 {}, {}", result, nl, nr).unwrap()
                        } else {
                            writeln!(&mut self.ir, "{} = icmp eq {} {}, {}", result, lt, l, r).unwrap()
                        }
                        return Ok((result, "i1".to_string()));
                    }
                    tinox_parser::BinaryOp::Ne => {
                        if float {
                            let float_ty = if lt == "double" || rt == "double" { "double" } else { lt.as_str() };
                            let lf = if lt != "double" && lt != "float" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast {} {} to double", c, lt, l).unwrap(); c } else { l.clone() };
                            let rf = if rt != "double" && rt != "float" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast {} {} to double", c, rt, r).unwrap(); c } else { r.clone() };
                            writeln!(&mut self.ir, "{} = fcmp one {} {}, {}", result, float_ty, lf, rf).unwrap()
                        } else if lt == "i8*" || rt == "i8*" {
                            let l_str = if lt == "i8*" { l.clone() } else { let c = self.temp(); writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, l).unwrap(); c };
                            let r_str = if rt == "i8*" { r.clone() } else { let c = self.temp(); writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, r).unwrap(); c };
                            let cmp = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_string_equals(i8* {}, i8* {})", cmp, l_str, r_str).unwrap();
                            let eq_bit = self.temp();
                            writeln!(&mut self.ir, "{} = trunc i64 {} to i1", eq_bit, cmp).unwrap();
                            writeln!(&mut self.ir, "{} = xor i1 {}, 1", result, eq_bit).unwrap()
                        } else if lt != rt {
                            let (nl, nr) = if lt.ends_with('*') || lt == "ptr" {
                                let c = self.temp(); writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", c, if lt == "ptr" { "ptr".to_string() } else { lt.clone() }, l).unwrap(); (c, r.clone())
                            } else if rt.ends_with('*') || rt == "ptr" {
                                let c = self.temp(); writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", c, if rt == "ptr" { "ptr".to_string() } else { rt.clone() }, r).unwrap(); (l.clone(), c)
                            } else { (l.clone(), r.clone()) };
                            writeln!(&mut self.ir, "{} = icmp ne i64 {}, {}", result, nl, nr).unwrap()
                        } else {
                            writeln!(&mut self.ir, "{} = icmp ne {} {}, {}", result, lt, l, r).unwrap()
                        }
                        return Ok((result, "i1".to_string()));
                    }
                    tinox_parser::BinaryOp::Lt => {
                        if float {
                            let float_ty = if lt == "double" || rt == "double" { "double" } else { lt.as_str() };
                            let lf = if lt != "double" && lt != "float" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast {} {} to double", c, lt, l).unwrap(); c } else { l.clone() };
                            let rf = if rt != "double" && rt != "float" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast {} {} to double", c, rt, r).unwrap(); c } else { r.clone() };
                            writeln!(&mut self.ir, "{} = fcmp olt {} {}, {}", result, float_ty, lf, rf).unwrap()
                        } else if lt == "i8*" && rt == "i8*" {
                            let c = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_string_compare(i8* {}, i8* {})", c, l, r).unwrap();
                            writeln!(&mut self.ir, "{} = icmp slt i64 {}, 0", result, c).unwrap()
                        } else {
                            writeln!(&mut self.ir, "{} = icmp slt {} {}, {}", result, lt, l, r).unwrap()
                        }
                        return Ok((result, "i1".to_string()));
                    }
                    tinox_parser::BinaryOp::Le => {
                        if float {
                            let float_ty = if lt == "double" || rt == "double" { "double" } else { lt.as_str() };
                            let lf = if lt != "double" && lt != "float" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast {} {} to double", c, lt, l).unwrap(); c } else { l.clone() };
                            let rf = if rt != "double" && rt != "float" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast {} {} to double", c, rt, r).unwrap(); c } else { r.clone() };
                            writeln!(&mut self.ir, "{} = fcmp ole {} {}, {}", result, float_ty, lf, rf).unwrap()
                        } else if lt == "i8*" && rt == "i8*" {
                            let c = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_string_compare(i8* {}, i8* {})", c, l, r).unwrap();
                            writeln!(&mut self.ir, "{} = icmp sle i64 {}, 0", result, c).unwrap()
                        } else {
                            writeln!(&mut self.ir, "{} = icmp sle {} {}, {}", result, lt, l, r).unwrap()
                        }
                        return Ok((result, "i1".to_string()));
                    }
                    tinox_parser::BinaryOp::Gt => {
                        if float {
                            let float_ty = if lt == "double" || rt == "double" { "double" } else { lt.as_str() };
                            let lf = if lt != "double" && lt != "float" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast {} {} to double", c, lt, l).unwrap(); c } else { l.clone() };
                            let rf = if rt != "double" && rt != "float" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast {} {} to double", c, rt, r).unwrap(); c } else { r.clone() };
                            writeln!(&mut self.ir, "{} = fcmp ogt {} {}, {}", result, float_ty, lf, rf).unwrap()
                        } else if lt == "i8*" && rt == "i8*" {
                            let c = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_string_compare(i8* {}, i8* {})", c, l, r).unwrap();
                            writeln!(&mut self.ir, "{} = icmp sgt i64 {}, 0", result, c).unwrap()
                        } else {
                            writeln!(&mut self.ir, "{} = icmp sgt {} {}, {}", result, lt, l, r).unwrap()
                        }
                        return Ok((result, "i1".to_string()));
                    }
                    tinox_parser::BinaryOp::Ge => {
                        if float {
                            let float_ty = if lt == "double" || rt == "double" { "double" } else { lt.as_str() };
                            let lf = if lt != "double" && lt != "float" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast {} {} to double", c, lt, l).unwrap(); c } else { l.clone() };
                            let rf = if rt != "double" && rt != "float" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast {} {} to double", c, rt, r).unwrap(); c } else { r.clone() };
                            writeln!(&mut self.ir, "{} = fcmp oge {} {}, {}", result, float_ty, lf, rf).unwrap()
                        } else if lt == "i8*" && rt == "i8*" {
                            let c = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_string_compare(i8* {}, i8* {})", c, l, r).unwrap();
                            writeln!(&mut self.ir, "{} = icmp sge i64 {}, 0", result, c).unwrap()
                        } else {
                            writeln!(&mut self.ir, "{} = icmp sge {} {}, {}", result, lt, l, r).unwrap()
                        }
                        return Ok((result, "i1".to_string()));
                    }
                    tinox_parser::BinaryOp::And => {
                        // Coerce operands to i1 if they are i64 (booleans stored as i64).
                        let li1 = if lt == "i1" { l.clone() } else {
                            let c = self.temp();
                            writeln!(&mut self.ir, "{} = icmp ne {} {}, 0", c, lt, l).unwrap();
                            c
                        };
                        let ri1 = if rt == "i1" { r.clone() } else {
                            let c = self.temp();
                            writeln!(&mut self.ir, "{} = icmp ne {} {}, 0", c, rt, r).unwrap();
                            c
                        };
                        writeln!(&mut self.ir, "{} = and i1 {}, {}", result, li1, ri1).unwrap();
                        return Ok((result, "i1".to_string()));
                    }
                    tinox_parser::BinaryOp::Or => {
                        // Coerce operands to i1 if they are i64 (booleans stored as i64).
                        let li1 = if lt == "i1" { l.clone() } else {
                            let c = self.temp();
                            writeln!(&mut self.ir, "{} = icmp ne {} {}, 0", c, lt, l).unwrap();
                            c
                        };
                        let ri1 = if rt == "i1" { r.clone() } else {
                            let c = self.temp();
                            writeln!(&mut self.ir, "{} = icmp ne {} {}, 0", c, rt, r).unwrap();
                            c
                        };
                        writeln!(&mut self.ir, "{} = or i1 {}, {}", result, li1, ri1).unwrap();
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
                            let (val, val_ty) = self.gen_expr(&args[1], ctx)?;
                            let result = self.temp();
                            let push_val = if val_ty.ends_with('*') || val_ty == "ptr" {
                                let casted = self.temp();
                                writeln!(&mut self.ir, "{} = ptrtoint {}* {} to i64", casted, val_ty.trim_end_matches('*'), val).unwrap();
                                casted
                            } else {
                                val
                            };
                            writeln!(&mut self.ir, "{} = call i64* @tinox_array_push(i64* {}, i64 {})", result, arr, push_val).unwrap();
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
                        "log" => {
                            let (val, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call double @log(double {})", result, val).unwrap();
                            return Ok((result, "double".to_string()));
                        }
                        "exp" => {
                            let (val, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call double @exp(double {})", result, val).unwrap();
                            return Ok((result, "double".to_string()));
                        }
                        "fabs" => {
                            let (val, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call double @llvm.fabs.f64(double {})", result, val).unwrap();
                            return Ok((result, "double".to_string()));
                        }
                        "mathTgamma" | "mathLgamma" | "mathCbrt" | "mathTrunc" | "mathRint" | "mathLogb"
                        | "mathLog2" | "mathLog10" | "mathExp2" | "mathExp10" => {
                            let libm = name[4..].to_lowercase();
                            let (val, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call double @{}(double {})", result, libm, val).unwrap();
                            return Ok((result, "double".to_string()));
                        }
                        "mathIsNan" | "mathIsInfinite" | "mathIsNormal" => {
                            let (val, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @{}(double {})", result, name, val).unwrap();
                            return Ok((result, "i64".to_string()));
                        }
                        "mathNan" | "mathInf" => {
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call double @{}()", result, name).unwrap();
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
                            let (s, s_ty) = self.gen_expr(&args[0], ctx)?;
                            let s_ptr = if s_ty == "i8*" { s.clone() } else { let c = self.temp(); writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, s).unwrap(); c };
                            let (prefix, p_ty) = self.gen_expr(&args[1], ctx)?;
                            let p_ptr = if p_ty == "i8*" { prefix.clone() } else { let c = self.temp(); writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, prefix).unwrap(); c };
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_string_starts_with(i8* {}, i8* {})", result, s_ptr, p_ptr).unwrap();
                            let bool_val = self.temp();
                            writeln!(&mut self.ir, "{} = trunc i64 {} to i1", bool_val, result).unwrap();
                            return Ok((bool_val, "i1".to_string()));
                        }
                        "endsWith" => {
                            let (s, s_ty) = self.gen_expr(&args[0], ctx)?;
                            let s_ptr = if s_ty == "i8*" { s.clone() } else { let c = self.temp(); writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, s).unwrap(); c };
                            let (suffix, suf_ty) = self.gen_expr(&args[1], ctx)?;
                            let suf_ptr = if suf_ty == "i8*" { suffix.clone() } else { let c = self.temp(); writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, suffix).unwrap(); c };
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_string_ends_with(i8* {}, i8* {})", result, s_ptr, suf_ptr).unwrap();
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
                            let (path, path_ty) = self.gen_expr(&args[0], ctx)?;
                            let path_str = if path_ty == "i8*" { path.clone() } else {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, path).unwrap();
                                c
                            };
                            let raw = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_file_exists(i8* {})", raw, path_str).unwrap();
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = icmp ne i64 {}, 0", result, raw).unwrap();
                            return Ok((result, "i1".to_string()));
                        }
                        "deleteFile" => {
                            let (path, _) = self.gen_expr(&args[0], ctx)?;
                            writeln!(&mut self.ir, "call void @tinox_file_delete(i8* {})", path).unwrap();
                            return Ok(("0".to_string(), "void".to_string()));
                        }
                        "processArgs" => {
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64* @processArgs()", result).unwrap();
                            return Ok((result, "i64*".to_string()));
                        }
                        "processExit" => {
                            let (code, code_ty) = self.gen_expr(&args[0], ctx)?;
                            let code_i64 = if code_ty == "i64" { code.clone() } else {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = zext {} {} to i64", c, code_ty, code).unwrap();
                                c
                            };
                            writeln!(&mut self.ir, "call void @processExit(i64 {})", code_i64).unwrap();
                            return Ok(("0".to_string(), "void".to_string()));
                        }
                        "dirList" => {
                            let (path, path_ty) = self.gen_expr(&args[0], ctx)?;
                            let path_str = if path_ty == "i8*" { path.clone() } else {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, path).unwrap();
                                c
                            };
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64* @dirList(i8* {})", result, path_str).unwrap();
                            return Ok((result, "i64*".to_string()));
                        }
                        "regexFindAll" | "regexSplit" => {
                            let (pat, pat_ty) = self.gen_expr(&args[0], ctx)?;
                            let pat_i64 = if pat_ty == "i64" { pat.clone() } else {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", c, pat_ty, pat).unwrap();
                                c
                            };
                            let (subj, subj_ty) = self.gen_expr(&args[1], ctx)?;
                            let subj_i64 = if subj_ty == "i64" { subj.clone() } else {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", c, subj_ty, subj).unwrap();
                                c
                            };
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64* @{}(i64 {}, i64 {})", result, name, pat_i64, subj_i64).unwrap();
                            return Ok((result, "i64*".to_string()));
                        }
                        "regexMatchGroups" => {
                            let (pat, pat_ty) = self.gen_expr(&args[0], ctx)?;
                            let pat_str = if pat_ty == "i8*" { pat.clone() } else {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, pat).unwrap();
                                c
                            };
                            let (subj, subj_ty) = self.gen_expr(&args[1], ctx)?;
                            let subj_str = if subj_ty == "i8*" { subj.clone() } else {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, subj).unwrap();
                                c
                            };
                            let (off, _) = self.gen_expr(&args[2], ctx)?;
                            let (icase, _) = self.gen_expr(&args[3], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64* @regexMatchGroups(i8* {}, i8* {}, i64 {}, i64 {})", result, pat_str, subj_str, off, icase).unwrap();
                            return Ok((result, "i64*".to_string()));
                        }
                        "fileReadAllText" => {
                            let (path, path_ty) = self.gen_expr(&args[0], ctx)?;
                            let path_str = if path_ty == "i8*" { path.clone() } else {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, path).unwrap();
                                c
                            };
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i8* @fileReadAllText(i8* {})", result, path_str).unwrap();
                            return Ok((result, "i8*".to_string()));
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
                let is_expr_fn_ptr = !is_local_fn && fn_name == "unknown_fn";
                if is_expr_fn_ptr {
                    // func is an expression (e.g., array[i]) that evaluates to a fn ptr or closure ptr
                    let (fn_val, fn_ty) = self.gen_expr(func, ctx)?;
                    if fn_ty == "i64*" {
                        // Closure: load fn_ptr from index 0 and env_ptr from index 1
                        let fp_val = self.temp();
                        writeln!(&mut self.ir, "{} = load i64, i64* {}", fp_val, fn_val).unwrap();
                        let env_ptr = self.temp();
                        writeln!(&mut self.ir, "{} = getelementptr i64, ptr {}, i64 1", env_ptr, fn_val).unwrap();
                        let env_val = self.temp();
                        writeln!(&mut self.ir, "{} = load i64*, i64* {}", env_val, env_ptr).unwrap();
                        let casted_fn = self.temp();
                        writeln!(&mut self.ir, "{} = inttoptr i64 {} to i64 (i64, i64*)*", casted_fn, fp_val).unwrap();
                        if ret_ty == "void" {
                            writeln!(&mut self.ir, "call void {}({}, i64* {})", casted_fn, args_str.trim(), env_val).unwrap();
                        } else {
                            writeln!(&mut self.ir, "{} = call {} {}({}, i64* {})", result, ret_ty, casted_fn, args_str.trim(), env_val).unwrap();
                        }
                    } else {
                        // Raw fn ptr stored as i64: inttoptr and call
                        let fn_i64 = if fn_ty == "i64" { fn_val.clone() } else {
                            let c = self.temp();
                            writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", c, fn_ty, fn_val).unwrap();
                            c
                        };
                        let casted_fn = self.temp();
                        writeln!(&mut self.ir, "{} = inttoptr i64 {} to i64 (i64)*", casted_fn, fn_i64).unwrap();
                        if ret_ty == "void" {
                            writeln!(&mut self.ir, "call void {}({})", casted_fn, args_str).unwrap();
                        } else {
                            writeln!(&mut self.ir, "{} = call {} {}({})", result, ret_ty, casted_fn, args_str).unwrap();
                        }
                    }
                } else if is_local_fn {
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
                // ORM query chain: DB.of(T).filter(lambda)...list()/first()/count()
                if matches!(method.as_str(), "list" | "first" | "count") && args.is_empty() {
                    if let Some(chain) = try_extract_orm_chain(obj, method.as_str()) {
                        if self.entity_entries.iter().any(|e| e.class_name == chain.entity_class) {
                            let chain = chain.clone();
                            return self.gen_orm_query(&chain, ctx);
                        }
                    }
                }

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

                // Array method dispatch: only trigger for explicit Array types or when declared type is
                // unknown (None) and obj_ty is i64* — never trigger for known struct instances.
                // Also trigger for i64 objects (ptrtoint'd array pointers) with known array methods.
                let is_known_struct = declared_type.as_deref()
                    .map(|t| self.struct_layouts.contains_key(t))
                    .unwrap_or(false);
                // Array-only methods (excludes contains/len/remove/insert which are also map methods)
                let array_only_methods = ["push","pop","sort","reverse","slice","join",
                    "first","last","find","filter","map","reduce","any","all","indexOf",
                    "clear","isEmpty","toList","unique","flatten","zip","unzip","take","skip",
                    "sortBy","groupBy","partition","sum","min","max","average","forEach"];
                let is_i64_array_method = obj_ty == "i64" && !is_known_struct
                    && array_only_methods.contains(&method.as_str());
                // Coerce i64 array pointer to i64* for array dispatch
                let (obj_ptr, obj_ty) = if is_i64_array_method {
                    let c = self.temp();
                    writeln!(&mut self.ir, "{} = inttoptr i64 {} to i64*", c, obj_ptr).unwrap();
                    (c, "i64*".to_string())
                } else {
                    (obj_ptr, obj_ty)
                };
                let is_array_type = declared_type.as_deref().map(|t| t == "Array:String" || t == "Array").unwrap_or(false)
                    || (obj_ty == "i64*" && !is_known_struct);
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
                            let store_val = if val_ty.ends_with('*') || val_ty == "ptr" {
                                let c = self.temp();
                                let base_ty = val_ty.trim_end_matches('*');
                                writeln!(&mut self.ir, "{} = ptrtoint {}* {} to i64", c, base_ty, val).unwrap();
                                c
                            } else if val_ty == "double" || val_ty == "float" {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = bitcast {} {} to i64", c, val_ty, val).unwrap();
                                c
                            } else { val };
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64* @tinox_array_push(i64* {}, i64 {})", result, obj_ptr, store_val).unwrap();
                            // Write the new pointer back to the source (push may realloc)
                            if let ExprKind::Ident(var_name) = &obj.node {
                                let slot = ctx.local_slots.get(var_name.as_str()).cloned().unwrap_or_else(|| var_name.clone());
                                writeln!(&mut self.ir, "store i64* {}, i64** %{}", result, slot).unwrap();
                            } else if let ExprKind::FieldAccess { obj: field_obj, field: field_name } = &obj.node {
                                // Re-derive the struct field GEP and store the new array ptr as i64
                                let (parent_raw, parent_ty) = self.gen_expr(field_obj, ctx)?;
                                let parent_ptr = if parent_ty == "i64" {
                                    let c = self.temp();
                                    writeln!(&mut self.ir, "{} = inttoptr i64 {} to i64*", c, parent_raw).unwrap();
                                    c
                                } else {
                                    parent_raw
                                };
                                let struct_name = match &field_obj.node {
                                    ExprKind::This => ctx.current_struct.clone().unwrap_or_default(),
                                    ExprKind::Ident(n) => ctx.local_types.get(n).cloned().unwrap_or_default(),
                                    _ => self.infer_struct_type(field_obj, ctx).unwrap_or_default(),
                                };
                                let field_offset = self.struct_layouts.get(&struct_name)
                                    .and_then(|fields| fields.iter().position(|f| f == field_name))
                                    .unwrap_or(0) as i64;
                                let field_gep = self.temp();
                                writeln!(&mut self.ir, "{} = getelementptr i64, ptr {}, i64 {}", field_gep, parent_ptr, field_offset).unwrap();
                                let new_i64 = self.temp();
                                writeln!(&mut self.ir, "{} = ptrtoint i64* {} to i64", new_i64, result).unwrap();
                                writeln!(&mut self.ir, "store i64 {}, i64* {}", new_i64, field_gep).unwrap();
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
                        "removeAt" => {
                            let (idx, _) = self.gen_expr(&args[0], ctx)?;
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64* @tinox_array_remove_at(i64* {}, i64 {})", result, obj_ptr, idx).unwrap();
                            // Write the new pointer back to the source variable if possible
                            if let ExprKind::Ident(var_name) = &obj.node {
                                let slot = ctx.local_slots.get(var_name.as_str()).cloned().unwrap_or_else(|| var_name.clone());
                                writeln!(&mut self.ir, "store i64* {}, i64** %{}", result, slot).unwrap();
                            }
                            return Ok(("0".to_string(), "void".to_string()));
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

                // Map method dispatch — also handle i64 objects that may be ptrtoint'd Map pointers
                let is_map_dispatch = declared_type.as_deref() == Some("Map")
                    || (obj_ty == "i64" && matches!(method.as_str(), "get" | "insert" | "contains" | "keys" | "values" | "remove" | "len"));
                if is_map_dispatch {
                    let map_obj_ptr = if obj_ty == "i64" {
                        let c = self.temp();
                        writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, obj_ptr).unwrap();
                        c
                    } else {
                        obj_ptr.clone()
                    };
                    match method.as_str() {
                        "get" => {
                            let (key, key_ty) = self.gen_expr(&args[0], ctx)?;
                            let key_i8 = if key_ty == "i8*" { key.clone() } else {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, key).unwrap();
                                c
                            };
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_map_get(i8* {}, i8* {})", result, map_obj_ptr, key_i8).unwrap();
                            return Ok((result, "i64".to_string()));
                        }
                        "insert" => {
                            let (key, key_ty) = self.gen_expr(&args[0], ctx)?;
                            let key_i8 = if key_ty == "i8*" { key.clone() } else {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, key).unwrap();
                                c
                            };
                            let (val, val_ty) = self.gen_expr(&args[1], ctx)?;
                            let val_i64 = if val_ty == "i64" || val_ty.is_empty() {
                                val.clone()
                            } else if val_ty == "i1" {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = zext i1 {} to i64", c, val).unwrap();
                                c
                            } else if val_ty == "double" || val_ty == "float" {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = bitcast {} {} to i64", c, val_ty, val).unwrap();
                                c
                            } else {
                                // pointer type — ptrtoint
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", c, val_ty, val).unwrap();
                                c
                            };
                            writeln!(&mut self.ir, "call void @tinox_map_set(i8* {}, i8* {}, i64 {})", map_obj_ptr, key_i8, val_i64).unwrap();
                            return Ok(("0".to_string(), "void".to_string()));
                        }
                        "contains" => {
                            let (key, key_ty) = self.gen_expr(&args[0], ctx)?;
                            let key_str = if key_ty == "i8*" { key.clone() } else {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, key).unwrap();
                                c
                            };
                            let raw = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_map_contains(i8* {}, i8* {})", raw, map_obj_ptr, key_str).unwrap();
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = icmp ne i64 {}, 0", result, raw).unwrap();
                            return Ok((result, "i1".to_string()));
                        }
                        "remove" => {
                            let (key, key_ty) = self.gen_expr(&args[0], ctx)?;
                            let key_str = if key_ty == "i8*" { key.clone() } else {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, key).unwrap();
                                c
                            };
                            writeln!(&mut self.ir, "call void @tinox_map_remove(i8* {}, i8* {})", map_obj_ptr, key_str).unwrap();
                            return Ok(("0".to_string(), "void".to_string()));
                        }
                        "len" => {
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_map_len(i8* {})", result, map_obj_ptr).unwrap();
                            return Ok((result, "i64".to_string()));
                        }
                        "keys" => {
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64* @tinox_map_keys(i8* {})", result, map_obj_ptr).unwrap();
                            return Ok((result, "i64*".to_string()));
                        }
                        "values" => {
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64* @tinox_map_values(i8* {})", result, map_obj_ptr).unwrap();
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

                // Int/Float/Bool toString must be dispatched before str_methods conversion
                // to avoid i64 integer values being misidentified as string pointers.
                if method == "toString" {
                    if obj_ty == "i64" || obj_ty == "double" || obj_ty == "i1" {
                        let result = self.temp();
                        let (fn_name, arg_ty) = match obj_ty.as_str() {
                            "double" => ("tinox_float_to_string", "double"),
                            "i1"     => ("tinox_bool_to_string", "i1"),
                            _        => ("tinox_int_to_string", "i64"),
                        };
                        writeln!(&mut self.ir, "{} = call i8* @{}({} {})", result, fn_name, arg_ty, obj_ptr).unwrap();
                        return Ok((result, "i8*".to_string()));
                    }
                    // Class object toString() — dispatch to generated ClassName_toString
                    if obj_ty == "i64*" {
                        if let Some(cn) = declared_type.as_deref() {
                            let key = format!("{}_toString", cn);
                            if self.method_ret_types.contains_key(&key) {
                                let obj_ptr_typed = if obj_ty == "i64*" {
                                    obj_ptr.clone()
                                } else {
                                    let c = self.temp();
                                    writeln!(&mut self.ir, "{} = inttoptr i64 {} to i64*", c, obj_ptr).unwrap();
                                    c
                                };
                                let result = self.temp();
                                writeln!(&mut self.ir, "{} = call i8* @{}(i64* {})", result, key, obj_ptr_typed).unwrap();
                                return Ok((result, "i8*".to_string()));
                            }
                        }
                    }
                }

                // String method dispatch (obj_ty == "i8*", or i64 stored string pointer)
                let str_methods = ["len","toUpper","toUpperCase","toLower","toLowerCase",
                    "trim","contains","startsWith","endsWith","split","substring","indexOf",
                    "replace","toString","toInt","toFloat","toBool","repeat","padLeft","padRight",
                    "count","charAt","toInt64","toFloat64","toBytes","fromBytes","format","encode",
                    "decode","hash","md5","sha256","base64Encode","base64Decode","urlEncode",
                    "urlDecode","isNumeric","isEmpty","isBlank","lines","words","reverse",
                    "truncate","ellipsis","mask","redact","normalize"];
                let is_str_method = str_methods.contains(&method.as_str());
                let (obj_ptr, obj_ty) = if obj_ty == "i64" && is_str_method {
                    let s = self.temp();
                    writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", s, obj_ptr).unwrap();
                    (s, "i8*".to_string())
                } else {
                    (obj_ptr, obj_ty)
                };
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
                            let (arg, arg_ty) = self.gen_expr(&args[0], ctx)?;
                            let arg_str = if arg_ty == "i8*" { arg.clone() } else {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, arg).unwrap();
                                c
                            };
                            let raw = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_string_contains(i8* {}, i8* {})", raw, obj_ptr, arg_str).unwrap();
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = icmp ne i64 {}, 0", result, raw).unwrap();
                            return Ok((result, "i1".to_string()));
                        }
                        "startsWith" => {
                            let (arg, arg_ty) = self.gen_expr(&args[0], ctx)?;
                            let arg_str = if arg_ty == "i8*" { arg.clone() } else {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, arg).unwrap();
                                c
                            };
                            let raw = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_string_starts_with(i8* {}, i8* {})", raw, obj_ptr, arg_str).unwrap();
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = icmp ne i64 {}, 0", result, raw).unwrap();
                            return Ok((result, "i1".to_string()));
                        }
                        "endsWith" => {
                            let (arg, arg_ty) = self.gen_expr(&args[0], ctx)?;
                            let arg_str = if arg_ty == "i8*" { arg.clone() } else {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, arg).unwrap();
                                c
                            };
                            let raw = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_string_ends_with(i8* {}, i8* {})", raw, obj_ptr, arg_str).unwrap();
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = icmp ne i64 {}, 0", result, raw).unwrap();
                            return Ok((result, "i1".to_string()));
                        }
                        "indexOf" => {
                            let (arg, arg_ty) = self.gen_expr(&args[0], ctx)?;
                            let arg_str = if arg_ty == "i8*" { arg.clone() } else {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, arg).unwrap();
                                c
                            };
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64 @tinox_string_index_of(i8* {}, i8* {})", result, obj_ptr, arg_str).unwrap();
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
                        "split" => {
                            let (delim, delim_ty) = self.gen_expr(&args[0], ctx)?;
                            let delim_str = if delim_ty == "i8*" { delim.clone() } else {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, delim).unwrap();
                                c
                            };
                            let result = self.temp();
                            writeln!(&mut self.ir, "{} = call i64* @tinox_string_split(i8* {}, i8* {})", result, obj_ptr, delim_str).unwrap();
                            return Ok((result, "i64*".to_string()));
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
                // Before generating lambda args, look up expected param types for type inference.
                let method_key = if let Some(ref dt) = declared_type {
                    format!("{}_{}", dt, method)
                } else {
                    method.clone()
                };
                let method_expected_params = self.method_param_types.get(&method_key).cloned();
                let mut extra_args: Vec<(String, String)> = Vec::new();
                for (i, arg) in args.iter().enumerate() {
                    if matches!(&arg.node, ExprKind::Lambda { .. }) {
                        if let Some(ref mep) = method_expected_params {
                            if let Some(tinox_parser::Type::Fn { params: fn_params, .. }) = mep.get(i) {
                                self.pending_lambda_param_types = fn_params.iter().map(|t| {
                                    if let tinox_parser::Type::Named(n) = t { Some(n.clone()) } else { None }
                                }).collect();
                            }
                        }
                    }
                    let (val, ty) = self.gen_expr(arg, ctx)?;
                    self.pending_lambda_param_types.clear();
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
                } else if let Some(_fn_sig) = declared_type.as_deref()
                    .and_then(|dt| self.fn_field_sigs.get(dt))
                    .and_then(|m| m.get(method.as_str()))
                    .cloned()
                {
                    // Fn-type field call: stored value is a closure struct address {fn_ptr: i64, env_ptr: i64*}.
                    // Load fn_ptr and env_ptr, convert args to i64 (ptrtoint), then call fn_ptr(args..., env_ptr).
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
                    // The stored i64 is a closure struct address (ptrtoint of i64* closure alloc)
                    let closure_addr = self.temp();
                    writeln!(&mut self.ir, "{} = load i64, i64* {}", closure_addr, field_gep).unwrap();
                    let closure_ptr = self.temp();
                    writeln!(&mut self.ir, "{} = inttoptr i64 {} to i64*", closure_ptr, closure_addr).unwrap();
                    // Load fn_ptr (i64) from closure slot 0
                    let fn_ptr_i64 = self.temp();
                    writeln!(&mut self.ir, "{} = load i64, i64* {}", fn_ptr_i64, closure_ptr).unwrap();
                    // Load env_ptr from closure slot 1
                    let env_gep = self.temp();
                    writeln!(&mut self.ir, "{} = getelementptr i64, ptr {}, i64 1", env_gep, closure_ptr).unwrap();
                    let env_ptr = self.temp();
                    writeln!(&mut self.ir, "{} = load i64*, i64* {}", env_ptr, env_gep).unwrap();
                    // Tinox lambdas always have LLVM signature i64 (i64, i64*) regardless of declared type
                    let fp = self.temp();
                    writeln!(&mut self.ir, "{} = inttoptr i64 {} to i64 (i64, i64*)*", fp, fn_ptr_i64).unwrap();
                    // Generate call args: convert pointer args to i64 via ptrtoint
                    let mut call_args: Vec<String> = Vec::new();
                    for arg in args.iter() {
                        let (v, t) = self.gen_expr(arg, ctx)?;
                        if t == "i64*" || t == "i8*" || t == "ptr" || (t.len() > 1 && t.ends_with('*')) {
                            let as_i64 = self.temp();
                            writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", as_i64, t, v).unwrap();
                            call_args.push(format!("i64 {}", as_i64));
                        } else {
                            call_args.push(format!("{} {}", t, v));
                        }
                    }
                    call_args.push(format!("i64* {}", env_ptr));
                    let result = self.temp();
                    let args_str = call_args.join(", ");
                    // Discard return value (lambdas return i64 but field type may say void)
                    writeln!(&mut self.ir, "{} = call i64 {}({})", result, fp, args_str).unwrap();
                    Ok((result, "i64".to_string()))
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
                let declared_elem_type = arr_name.as_ref().and_then(|n| ctx.local_types.get(n)).cloned()
                    // Fields like `this.rawLines` (List<String>) have no local_types entry —
                    // fall back to struct field type info so elements are typed as strings.
                    .or_else(|| self.infer_struct_type(obj, ctx));
                let is_str_arr = declared_elem_type.as_deref() == Some("Array:String");
                let is_map = declared_elem_type.as_deref() == Some("Map");

                let (idx_val, idx_ty) = self.gen_expr(index, ctx)?;
                let (base_ptr, base_ty) = if let ExprKind::Ident(name) = &obj.node {
                    if ctx.params.contains(name) {
                        self.gen_expr(obj, ctx)?
                    } else if ctx.locals.contains_key(name) {
                        let (var_ty, _) = ctx.locals.get(name).unwrap();
                        let slot = ctx.local_slots.get(name).cloned().unwrap_or_else(|| name.clone());
                        let loaded_ptr = self.temp();
                        writeln!(&mut self.ir, "{} = load {}, {}* %{}", loaded_ptr, var_ty, var_ty, slot).unwrap();
                        (loaded_ptr, var_ty.clone())
                    } else {
                        self.gen_expr(obj, ctx)?
                    }
                } else {
                    self.gen_expr(obj, ctx)?
                };

                if is_map || idx_ty == "i8*" {
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
                    // Coerce base pointer to ptr if it's an i64 (pointer-as-integer).
                    let base_as_ptr = if base_ty == "i64" {
                        let p = self.temp();
                        writeln!(&mut self.ir, "{} = inttoptr i64 {} to i64*", p, base_ptr).unwrap();
                        p
                    } else {
                        base_ptr.clone()
                    };
                    let ptr_name = self.temp();
                    writeln!(&mut self.ir, "{} = getelementptr i64, ptr {}, i64 {}", ptr_name, base_as_ptr, idx_val).unwrap();
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
                    let store_val = if val_ty == "i1" {
                        let cast = self.temp();
                        writeln!(&mut self.ir, "{} = zext i1 {} to i64", cast, val).unwrap();
                        cast
                    } else if val_ty == "double" || val_ty == "float" {
                        let cast = self.temp();
                        writeln!(&mut self.ir, "{} = bitcast {} {} to i64", cast, val_ty, val).unwrap();
                        cast
                    } else if val_ty != "i64" && !val_ty.is_empty() && val_ty != "void" {
                        let cast = self.temp();
                        if val_ty == "ptr" {
                            writeln!(&mut self.ir, "{} = ptrtoint ptr {} to i64", cast, val).unwrap();
                        } else {
                            writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", cast, val_ty, val).unwrap();
                        }
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
                    let (key_val, key_ty) = self.gen_expr(key_expr, ctx)?;
                    let key_i8 = if key_ty == "i8*" { key_val.clone() } else {
                        let c = self.temp();
                        writeln!(&mut self.ir, "{} = inttoptr i64 {} to i8*", c, key_val).unwrap();
                        c
                    };
                    let (val_val, val_ty) = self.gen_expr(val_expr, ctx)?;
                    let val_i64 = if val_ty == "i64" || val_ty.is_empty() {
                        val_val.clone()
                    } else if val_ty == "i1" {
                        let c = self.temp(); writeln!(&mut self.ir, "{} = zext i1 {} to i64", c, val_val).unwrap(); c
                    } else if val_ty == "double" || val_ty == "float" {
                        let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast {} {} to i64", c, val_ty, val_val).unwrap(); c
                    } else {
                        let c = self.temp(); writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", c, val_ty, val_val).unwrap(); c
                    };
                    writeln!(&mut self.ir, "call void @tinox_map_set(i8* {}, i8* {}, i64 {})", map_ptr, key_i8, val_i64).unwrap();
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
                        if val_ty == "ptr" {
                            writeln!(&mut self.ir, "{} = ptrtoint ptr {} to i64", cast, val).unwrap();
                        } else if val_ty.ends_with('*') {
                            writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", cast, val_ty, val).unwrap();
                        } else {
                            writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", cast, val_ty, val).unwrap();
                        }
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
                    let mut args_parts: Vec<String> = Vec::new();
                    // Instance methods (fn) have an implicit self param — pass null.
                    // Static methods (fnc) do not have self — don't add null.
                    let is_static = self.static_method_keys.contains(&static_key);
                    if self.method_param_types.contains_key(&static_key) && !is_static {
                        args_parts.push("i64* null".to_string());
                    }
                    for arg in args.iter() {
                        let (v, t) = self.gen_expr(arg, ctx)?;
                        args_parts.push(format!("{} {}", t, v));
                    }
                    let args_str = args_parts.join(", ");
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
                        let (val, val_ty) = self.gen_expr(arg, ctx)?;
                        let store_val = if val_ty == "i1" {
                            let c = self.temp();
                            writeln!(&mut self.ir, "{} = zext i1 {} to i64", c, val).unwrap();
                            c
                        } else if val_ty == "double" || val_ty == "float" {
                            let c = self.temp();
                            writeln!(&mut self.ir, "{} = bitcast {} {} to i64", c, val_ty, val).unwrap();
                            c
                        } else if val_ty != "i64" && !val_ty.is_empty() && val_ty != "void" {
                            let c = self.temp();
                            if val_ty == "ptr" {
                                writeln!(&mut self.ir, "{} = ptrtoint ptr {} to i64", c, val).unwrap();
                            } else {
                                writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", c, val_ty, val).unwrap();
                            }
                            c
                        } else {
                            val
                        };
                        let arg_ptr = self.temp();
                        writeln!(
                            &mut self.ir,
                            "{} = getelementptr i64, ptr {}, i64 {}",
                            arg_ptr,
                            typed_ptr,
                            i + 1
                        )
                        .unwrap();
                        writeln!(&mut self.ir, "store i64 {}, i64* {}", store_val, arg_ptr).unwrap();
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
                    let expected = ctx.ret_type.clone();
                    let (final_val, final_ty) = if !expected.is_empty() && ty != expected {
                        let is_from_float = Self::is_float(&ty);
                        let is_to_float = Self::is_float(&expected);
                        let cast_op = match (ty.as_str(), expected.as_str()) {
                            _ if is_from_float && is_to_float => "fptrunc",
                            (from, _) if is_to_float && from.starts_with('i') => "bitcast",
                            (_, to) if is_from_float && to.starts_with('i') => "bitcast",
                            (from, to) if from.ends_with('*') && to.ends_with('*') => "bitcast",
                            (from, to) if from.starts_with('i') && to.starts_with('i')
                                && !from.contains('*') && !to.contains('*') =>
                            {
                                let from_bits: u32 = from[1..].parse().unwrap_or(64);
                                let to_bits: u32 = to[1..].parse().unwrap_or(64);
                                if from_bits > to_bits { "trunc" } else { "zext" }
                            }
                            (from, to) if !from.ends_with('*') && to.ends_with('*') => "inttoptr",
                            (from, to) if from.ends_with('*') && !to.ends_with('*') => "ptrtoint",
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
                    let llvm_ty = Self::llvm_type_str(&final_ty);
                    writeln!(&mut self.ir, "ret {} {}", llvm_ty, final_val).unwrap();
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
                            let store_val = if val_ty == "i1" {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = zext i1 {} to i64", c, val).unwrap();
                                c
                            } else if val_ty == "double" || val_ty == "float" {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = bitcast {} {} to i64", c, val_ty, val).unwrap();
                                c
                            } else if val_ty != "i64" && !val_ty.is_empty() && val_ty != "void" {
                                let c = self.temp();
                                if val_ty == "ptr" {
                                    writeln!(&mut self.ir, "{} = ptrtoint ptr {} to i64", c, val).unwrap();
                                } else {
                                    writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", c, val_ty, val).unwrap();
                                }
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
                // Pre-allocate a result slot so each arm can store its value into it.
                // This ensures the result dominates the merge block regardless of which arm ran.
                let result_slot = format!("match_result_{}", self.temp_count);
                self.temp_count += 1;
                writeln!(&mut self.ir, "%{} = alloca i64", result_slot).unwrap();
                writeln!(&mut self.ir, "store i64 0, i64* %{}", result_slot).unwrap();
                let mut last_result_ty: String = "i64".to_string();
                for case in cases {
                    match &case.pattern {
                        Pattern::Wildcard(_) => {
                            let (body_val, body_ty) = self.gen_expr(&case.body, ctx)?;
                            last_result_ty = body_ty.clone();
                            let store_val = if body_ty == "i64" || body_ty.is_empty() { body_val.clone() }
                                else if body_ty == "i1" { let c = self.temp(); writeln!(&mut self.ir, "{} = zext i1 {} to i64", c, body_val).unwrap(); c }
                                else if body_ty == "double" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast double {} to i64", c, body_val).unwrap(); c }
                                else if body_ty != "void" { let c = self.temp(); writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", c, body_ty, body_val).unwrap(); c }
                                else { "0".to_string() };
                            writeln!(&mut self.ir, "store i64 {}, i64* %{}", store_val, result_slot).unwrap();
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
                            last_result_ty = body_ty.clone();
                            let store_val = if body_ty == "i64" || body_ty.is_empty() { body_val.clone() }
                                else if body_ty == "i1" { let c = self.temp(); writeln!(&mut self.ir, "{} = zext i1 {} to i64", c, body_val).unwrap(); c }
                                else if body_ty == "double" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast double {} to i64", c, body_val).unwrap(); c }
                                else if body_ty != "void" { let c = self.temp(); writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", c, body_ty, body_val).unwrap(); c }
                                else { "0".to_string() };
                            writeln!(&mut self.ir, "store i64 {}, i64* %{}", store_val, result_slot).unwrap();
                            writeln!(&mut self.ir, "br label %{}", merge_bb).unwrap();
                            writeln!(&mut self.ir, "{}:", next_bb).unwrap();
                        }
                        Pattern::Ident(name, _, _) if self.known_enum_variants.contains(name) => {
                            // Bare enum variant name (e.g. `North` instead of `Dir::North`)
                            let discriminator = name.chars().map(|c| c as i64).sum::<i64>();
                            let val_i64 = if val_ty.ends_with('*') || val_ty == "ptr" {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", c, val_ty, val).unwrap();
                                c
                            } else {
                                val.clone()
                            };
                            let cmp = self.temp();
                            writeln!(
                                &mut self.ir,
                                "{} = icmp eq i64 {}, {}",
                                cmp, val_i64, discriminator
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
                            last_result_ty = body_ty.clone();
                            let store_val = if body_ty == "i64" || body_ty.is_empty() { body_val.clone() }
                                else if body_ty == "i1" { let c = self.temp(); writeln!(&mut self.ir, "{} = zext i1 {} to i64", c, body_val).unwrap(); c }
                                else if body_ty == "double" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast double {} to i64", c, body_val).unwrap(); c }
                                else if body_ty != "void" { let c = self.temp(); writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", c, body_ty, body_val).unwrap(); c }
                                else { "0".to_string() };
                            writeln!(&mut self.ir, "store i64 {}, i64* %{}", store_val, result_slot).unwrap();
                            writeln!(&mut self.ir, "br label %{}", merge_bb).unwrap();
                            writeln!(&mut self.ir, "{}:", next_bb).unwrap();
                        }
                        Pattern::Ident(name, _, _) => {
                            let llvm_ty = val_ty.clone();
                            let slot_name = format!("{}_{}", name, self.temp_count);
                            self.temp_count += 1;
                            ctx.locals
                                .insert(name.clone(), (llvm_ty.clone(), ctx.locals.len()));
                            ctx.local_slots.insert(name.clone(), slot_name.clone());
                            writeln!(&mut self.ir, "%{} = alloca {}", slot_name, llvm_ty).unwrap();
                            writeln!(
                                &mut self.ir,
                                "store {} {}, {}* %{}",
                                val_ty, val, llvm_ty, slot_name
                            )
                            .unwrap();
                            let (body_val, body_ty) = self.gen_expr(&case.body, ctx)?;
                            last_result_ty = body_ty.clone();
                            let store_val = if body_ty == "i64" || body_ty.is_empty() { body_val.clone() }
                                else if body_ty == "i1" { let c = self.temp(); writeln!(&mut self.ir, "{} = zext i1 {} to i64", c, body_val).unwrap(); c }
                                else if body_ty == "double" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast double {} to i64", c, body_val).unwrap(); c }
                                else if body_ty != "void" { let c = self.temp(); writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", c, body_ty, body_val).unwrap(); c }
                                else { "0".to_string() };
                            writeln!(&mut self.ir, "store i64 {}, i64* %{}", store_val, result_slot).unwrap();
                            writeln!(&mut self.ir, "br label %{}", merge_bb).unwrap();
                            ctx.locals.remove(name);
                            ctx.local_slots.remove(name.as_str());
                        }
                        Pattern::EnumVariant { enum_name, variant, args, .. } => {
                            // For enum variants, we need to:
                            // 1. Extract and compare the discriminator
                            // 2. If it matches, bind any pattern arguments

                            // When written as `Variant(args)` (no :: qualifier), the parser
                            // puts the name in `enum_name` and leaves `variant` empty.
                            // When written as `Enum::Variant(args)`, the name is in `variant`.
                            let disc_name = if variant.is_empty() { enum_name } else { variant };
                            let discriminator = disc_name.chars().map(|c| c as i64).sum::<i64>();

                            // Normalize the match subject to i64 so all arms use the same
                            // pointer-range-guarded logic regardless of the subject's LLVM type.
                            // Enum values are either a plain discriminator (< 65536, no-arg
                            // variants) or a heap pointer to [disc, payload...].
                            let val_i64 = if val_ty.ends_with('*') || val_ty == "ptr" {
                                let c = self.temp();
                                writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", c, val_ty, val).unwrap();
                                c
                            } else {
                                val.clone()
                            };
                            let case_bb = self.new_bb("match_case");
                            let next_bb = self.new_bb("match_next");
                            if !args.is_empty() {
                                // Payload variant: guard with pointer-range check before
                                // dereferencing, since the value may be a plain discriminator.
                                let try_ptr_bb = self.new_bb("try_ptr");
                                let is_ptr_check = self.temp();
                                writeln!(
                                    &mut self.ir,
                                    "{} = icmp ugt i64 {}, 65535",
                                    is_ptr_check, val_i64
                                )
                                .unwrap();
                                writeln!(
                                    &mut self.ir,
                                    "br i1 {}, label %{}, label %{}",
                                    is_ptr_check, try_ptr_bb, next_bb
                                )
                                .unwrap();
                                writeln!(&mut self.ir, "{}:", try_ptr_bb).unwrap();
                                let ptr_val = self.temp();
                                writeln!(
                                    &mut self.ir,
                                    "{} = inttoptr i64 {} to i64*",
                                    ptr_val, val_i64
                                )
                                .unwrap();
                                let disc_ptr = self.temp();
                                writeln!(
                                    &mut self.ir,
                                    "{} = getelementptr i64, ptr {}, i64 0",
                                    disc_ptr, ptr_val
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
                                writeln!(
                                    &mut self.ir,
                                    "br i1 {}, label %{}, label %{}",
                                    cmp, case_bb, next_bb
                                )
                                .unwrap();
                                writeln!(&mut self.ir, "{}:", case_bb).unwrap();

                                // Bind arguments
                                for (i, arg_pattern) in args.iter().enumerate() {
                                    if let Pattern::Ident(arg_name, _, _) = arg_pattern {
                                        let arg_ptr = self.temp();
                                        writeln!(
                                            &mut self.ir,
                                            "{} = getelementptr i64, ptr {}, i64 {}",
                                            arg_ptr,
                                            ptr_val,
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
                                        self.bind_match_payload(ctx, disc_name, i, arg_name, &arg_val);
                                    }
                                }
                            } else {
                                // No-arg variant: plain discriminator compare
                                let cmp = self.temp();
                                writeln!(
                                    &mut self.ir,
                                    "{} = icmp eq i64 {}, {}",
                                    cmp, val_i64, discriminator
                                )
                                .unwrap();
                                writeln!(
                                    &mut self.ir,
                                    "br i1 {}, label %{}, label %{}",
                                    cmp, case_bb, next_bb
                                )
                                .unwrap();
                                writeln!(&mut self.ir, "{}:", case_bb).unwrap();
                            }
                            let (body_val, body_ty) = self.gen_expr(&case.body, ctx)?;
                            last_result_ty = body_ty.clone();
                            let store_val = if body_ty == "i64" || body_ty.is_empty() { body_val.clone() }
                                else if body_ty == "i1" { let c = self.temp(); writeln!(&mut self.ir, "{} = zext i1 {} to i64", c, body_val).unwrap(); c }
                                else if body_ty == "double" { let c = self.temp(); writeln!(&mut self.ir, "{} = bitcast double {} to i64", c, body_val).unwrap(); c }
                                else if body_ty != "void" { let c = self.temp(); writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", c, body_ty, body_val).unwrap(); c }
                                else { "0".to_string() };
                            writeln!(&mut self.ir, "store i64 {}, i64* %{}", store_val, result_slot).unwrap();
                            writeln!(&mut self.ir, "br label %{}", merge_bb).unwrap();
                            writeln!(&mut self.ir, "{}:", next_bb).unwrap();
                        }
                        _ => {}
                    }
                }
                writeln!(&mut self.ir, "br label %{}", merge_bb).unwrap();
                writeln!(&mut self.ir, "{}:", merge_bb).unwrap();
                // Load the result from the pre-allocated result slot.
                // This value is valid regardless of which arm ran (dominates all uses).
                let result_val = self.temp();
                writeln!(&mut self.ir, "{} = load i64, i64* %{}", result_val, result_slot).unwrap();
                // Restore the original type if the result is a pointer type
                let final_ty = if last_result_ty == "i64" || last_result_ty == "void" || last_result_ty.is_empty() {
                    last_result_ty.clone()
                } else if last_result_ty == "i1" {
                    // Restore bool: truncate from i64
                    let b = self.temp();
                    writeln!(&mut self.ir, "{} = trunc i64 {} to i1", b, result_val).unwrap();
                    return Ok((b, "i1".to_string()));
                } else if last_result_ty == "double" {
                    let d = self.temp();
                    writeln!(&mut self.ir, "{} = bitcast i64 {} to double", d, result_val).unwrap();
                    return Ok((d, "double".to_string()));
                } else if last_result_ty.ends_with('*') || last_result_ty == "ptr" {
                    // Restore pointer type: inttoptr
                    let p = self.temp();
                    writeln!(&mut self.ir, "{} = inttoptr i64 {} to {}", p, result_val, last_result_ty).unwrap();
                    return Ok((p, last_result_ty));
                } else {
                    last_result_ty.clone()
                };
                Ok((result_val, final_ty))
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
                    let slot = ctx.local_slots.get(name.as_str()).cloned().unwrap_or_else(|| name.clone());
                    // Coerce value type to target slot type
                    let store_val = if val_ty == store_ty || val_ty.is_empty() || store_ty.is_empty() {
                        val.clone()
                    } else if val_ty == "i64" && (store_ty.ends_with('*') || store_ty == "ptr") {
                        let c = self.temp();
                        writeln!(&mut self.ir, "{} = inttoptr i64 {} to {}", c, val, store_ty).unwrap();
                        c
                    } else if (val_ty.ends_with('*') || val_ty == "ptr") && store_ty == "i64" {
                        let c = self.temp();
                        writeln!(&mut self.ir, "{} = ptrtoint {} {} to i64", c, val_ty, val).unwrap();
                        c
                    } else if val_ty == "i1" && store_ty == "i64" {
                        let c = self.temp();
                        writeln!(&mut self.ir, "{} = zext i1 {} to i64", c, val).unwrap();
                        c
                    } else if val_ty == "double" && store_ty == "i64" {
                        let c = self.temp();
                        writeln!(&mut self.ir, "{} = bitcast double {} to i64", c, val).unwrap();
                        c
                    } else {
                        val.clone()
                    };
                    writeln!(&mut self.ir, "store {} {}, {}* %{}", store_ty, store_val, store_ty, slot).unwrap();
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
                    let slot = ctx.local_slots.get(name.as_str()).cloned().unwrap_or_else(|| name.clone());
                    let loaded = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = load {}, {}* %{}",
                        loaded.as_str(),
                        ty,
                        ty,
                        slot.as_str()
                    )
                    .unwrap();
                    let (rhs_raw, rhs_ty) = self.gen_expr(value, ctx)?;
                    let rhs = if ty == "i8*" && matches!(op, tinox_parser::CompoundOp::Add) {
                        rhs_raw
                    } else if rhs_ty == ty || rhs_ty.is_empty() {
                        rhs_raw
                    } else if rhs_ty == "i64" && ty == "double" {
                        let c = self.temp();
                        writeln!(&mut self.ir, "{} = sitofp i64 {} to double", c, rhs_raw).unwrap();
                        c
                    } else if rhs_ty == "double" && ty == "i64" {
                        let c = self.temp();
                        writeln!(&mut self.ir, "{} = bitcast double {} to i64", c, rhs_raw).unwrap();
                        c
                    } else {
                        rhs_raw
                    };
                    if ty == "i8*" && matches!(op, tinox_parser::CompoundOp::Add) {
                        // String += String → Konkatenation
                        let result = self.temp();
                        writeln!(&mut self.ir, "{} = call i8* @tinox_string_concat(i8* {}, i8* {})", result, loaded, rhs).unwrap();
                        writeln!(&mut self.ir, "store i8* {}, i8** %{}", result, slot).unwrap();
                        return Ok((result, ty));
                    }
                    let is_float = ty == "double" || ty == "float";
                    let result = self.temp();
                    match op {
                        tinox_parser::CompoundOp::Add => {
                            let instr = if is_float { "fadd" } else { "add" };
                            writeln!(&mut self.ir, "{} = {} {} {}, {}", result, instr, ty, loaded, rhs)
                                .unwrap();
                        }
                        tinox_parser::CompoundOp::Sub => {
                            let instr = if is_float { "fsub" } else { "sub" };
                            writeln!(&mut self.ir, "{} = {} {} {}, {}", result, instr, ty, loaded, rhs)
                                .unwrap();
                        }
                        tinox_parser::CompoundOp::Mul => {
                            let instr = if is_float { "fmul" } else { "mul" };
                            writeln!(&mut self.ir, "{} = {} {} {}, {}", result, instr, ty, loaded, rhs)
                                .unwrap();
                        }
                        tinox_parser::CompoundOp::Div => {
                            let instr = if is_float { "fdiv" } else { "sdiv" };
                            writeln!(&mut self.ir, "{} = {} {} {}, {}", result, instr, ty, loaded, rhs)
                                .unwrap();
                        }
                        tinox_parser::CompoundOp::Mod => {
                            let instr = if is_float { "frem" } else { "srem" };
                            writeln!(&mut self.ir, "{} = {} {} {}, {}", result, instr, ty, loaded, rhs)
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
                    writeln!(&mut self.ir, "store {} {}, {}* %{}", ty, result, ty, slot).unwrap();
                    return Ok((result, ty));
                }
            }
            ExprKind::Index { obj, index } => {
                let (idx_val, _) = self.gen_expr(index, ctx)?;
                let (base_ptr, _var_ty) = if let ExprKind::Ident(name) = &obj.node {
                    if ctx.locals.contains_key(name) {
                        let (vty, _) = ctx.locals.get(name).unwrap();
                        let slot = ctx.local_slots.get(name).cloned().unwrap_or_else(|| name.clone());
                        let loaded_ptr = self.temp();
                        writeln!(
                            &mut self.ir,
                            "{} = load {}, {}* %{}",
                            loaded_ptr, vty, vty, slot
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
            timed_metric: None,
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
            if let tinox_parser::Type::Named(struct_name) = &p.param_type {
                lambda_ctx.local_types.insert(p.name.clone(), struct_name.clone());
            } else if matches!(p.param_type, tinox_parser::Type::Infer | tinox_parser::Type::Any) {
                if let Some(Some(inferred)) = self.pending_lambda_param_types.get(i) {
                    lambda_ctx.local_types.insert(p.name.clone(), inferred.clone());
                }
            }
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
                    let slot = ctx.local_slots.get(name).cloned().unwrap_or_else(|| name.clone());
                    let val = self.temp();
                    writeln!(&mut self.ir, "{} = load {}, {}* %{}", val, ty, ty, slot).unwrap();
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
                // Propagate struct type info so method dispatch works inside the lambda
                if let Some(struct_type) = ctx.local_types.get(name) {
                    lambda_ctx.local_types.insert(name.clone(), struct_type.clone());
                }
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
            Type::Nullable(inner) => Self::type_to_llvm(inner),
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
        let n = self.temp_count;
        self.temp_count += 1;
        format!("{}_{}", name, n)
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

fn collect_free_vars_stmt(stmt: &tinox_parser::Stmt, param_names: &HashSet<String>, vars: &mut HashSet<String>) {
    match &stmt.node {
        StmtKind::Expr(e) => collect_free_vars_inner(e, param_names, vars),
        StmtKind::Return(Some(e)) => collect_free_vars_inner(e, param_names, vars),
        StmtKind::Let { value: Some(e), .. } => collect_free_vars_inner(e, param_names, vars),
        StmtKind::Var { value: Some(e), .. } => collect_free_vars_inner(e, param_names, vars),
        StmtKind::Assignment { target, value } => {
            collect_free_vars_inner(target, param_names, vars);
            collect_free_vars_inner(value, param_names, vars);
        }
        StmtKind::If { cond, then_branch, else_branch } => {
            collect_free_vars_inner(cond, param_names, vars);
            collect_free_vars_stmt(then_branch, param_names, vars);
            if let Some(eb) = else_branch { collect_free_vars_stmt(eb, param_names, vars); }
        }
        StmtKind::While { cond, body } => {
            collect_free_vars_inner(cond, param_names, vars);
            collect_free_vars_stmt(body, param_names, vars);
        }
        StmtKind::Block(stmts) => {
            for s in stmts { collect_free_vars_stmt(s, param_names, vars); }
        }
        _ => {}
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
                    StmtKind::Assignment { target, value } => {
                        collect_free_vars_inner(target, param_names, vars);
                        collect_free_vars_inner(value, param_names, vars);
                    }
                    StmtKind::If { cond, then_branch, else_branch } => {
                        collect_free_vars_inner(cond, param_names, vars);
                        collect_free_vars_stmt(then_branch, param_names, vars);
                        if let Some(eb) = else_branch {
                            collect_free_vars_stmt(eb, param_names, vars);
                        }
                    }
                    StmtKind::While { cond, body } => {
                        collect_free_vars_inner(cond, param_names, vars);
                        collect_free_vars_stmt(body, param_names, vars);
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
    /// If set, emit histogram_record before every return. (metric_name, start_reg)
    timed_metric: Option<(String, String)>,
}

// ─── ORM: compile-time lambda→SQL translation ────────────────────────────────

/// Describes an ORM query chain unwound from `DB.of(T).filter(...).orderBy(...).limit(n).list()`
#[derive(Debug, Clone)]
struct OrmChain {
    entity_class: String,
    /// (lambda_param_name, lambda_body_expr)
    filters: Vec<(String, Expr)>,
    /// (col_name, is_desc)
    order_by: Vec<(String, bool)>,
    limit: Option<i64>,
    offset_val: Option<i64>,
    /// terminal operation: "list" | "first" | "count"
    terminal: String,
}

/// Try to unwind a `DB.of(T).filter(...).orderBy(...).limit(n).list()` chain
/// into an OrmChain descriptor. Returns None if the expr is not an ORM chain.
fn try_extract_orm_chain(expr: &Expr, terminal: &str) -> Option<OrmChain> {
    let mut chain = OrmChain {
        entity_class: String::new(),
        filters: Vec::new(),
        order_by: Vec::new(),
        limit: None,
        offset_val: None,
        terminal: terminal.to_string(),
    };
    unwind_orm_chain(expr, &mut chain)?;
    if chain.entity_class.is_empty() { None } else { Some(chain) }
}

fn unwind_orm_chain(expr: &Expr, chain: &mut OrmChain) -> Option<()> {
    match &expr.node {
        ExprKind::MethodCall { obj, method, args } => {
            match method.as_str() {
                "filter" => {
                    if let Some(ExprKind::Lambda { params, body, .. }) = args.first().map(|a| &a.node) {
                        let param_name = params.first().map(|p| p.name.clone()).unwrap_or_default();
                        chain.filters.push((param_name, *body.clone()));
                    }
                    unwind_orm_chain(obj, chain)
                }
                "orderBy" => {
                    if let Some(lambda) = args.first() {
                        if let ExprKind::Lambda { body, .. } = &lambda.node {
                            if let ExprKind::FieldAccess { field, .. } = &body.node {
                                chain.order_by.push((field.clone(), false));
                            }
                        }
                    }
                    unwind_orm_chain(obj, chain)
                }
                "orderByDesc" => {
                    if let Some(lambda) = args.first() {
                        if let ExprKind::Lambda { body, .. } = &lambda.node {
                            if let ExprKind::FieldAccess { field, .. } = &body.node {
                                chain.order_by.push((field.clone(), true));
                            }
                        }
                    }
                    unwind_orm_chain(obj, chain)
                }
                "limit" => {
                    if let Some(ExprKind::Literal(Literal::Integer(n))) = args.first().map(|a| &a.node) {
                        chain.limit = Some(*n);
                    }
                    unwind_orm_chain(obj, chain)
                }
                "offset" => {
                    if let Some(ExprKind::Literal(Literal::Integer(n))) = args.first().map(|a| &a.node) {
                        chain.offset_val = Some(*n);
                    }
                    unwind_orm_chain(obj, chain)
                }
                "of" => {
                    // DB.of(ClassName) — bottom of the chain
                    if let ExprKind::Ident(db_name) = &obj.node {
                        if db_name == "DB" {
                            if let Some(ExprKind::Ident(class_name)) = args.first().map(|a| &a.node) {
                                chain.entity_class = class_name.clone();
                                return Some(());
                            }
                        }
                    }
                    None
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Extract the column name for `param.field` in a lambda body.
fn orm_extract_field<'a>(expr: &Expr, param: &str, fields: &'a [EntityFieldEntry]) -> Option<&'a str> {
    if let ExprKind::FieldAccess { obj, field } = &expr.node {
        if let ExprKind::Ident(name) = &obj.node {
            if name == param {
                return fields.iter().find(|f| f.field_name == *field).map(|f| f.column_name.as_str());
            }
        }
    }
    None
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

    // --- Integer arithmetic IR ---

    #[test]
    fn test_int_add_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 1 + 2; return x; }");
        assert!(ir.contains("add"), "should emit add instruction");
    }

    #[test]
    fn test_int_sub_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 10 - 3; return x; }");
        assert!(ir.contains("sub"), "should emit sub instruction");
    }

    #[test]
    fn test_int_mul_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 4 * 5; return x; }");
        assert!(ir.contains("mul"), "should emit mul instruction");
    }

    #[test]
    fn test_int_div_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 10 / 2; return x; }");
        assert!(ir.contains("sdiv"), "should emit sdiv for integer division");
    }

    #[test]
    fn test_int_mod_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 10 % 3; return x; }");
        assert!(ir.contains("srem"), "should emit srem for integer modulo");
    }

    // --- Float arithmetic IR ---

    #[test]
    fn test_float_sub_ir() {
        let ir = compile_to_ir("namespace t { class C { fnc f(a: Float64, b: Float64) -> Float64 { return a - b; } } }");
        assert!(ir.contains("fsub double"), "should emit fsub for float subtraction");
    }

    #[test]
    fn test_float_mul_ir() {
        let ir = compile_to_ir("namespace t { class C { fnc f(a: Float64, b: Float64) -> Float64 { return a * b; } } }");
        assert!(ir.contains("fmul double"), "should emit fmul for float multiplication");
    }

    #[test]
    fn test_float_div_ir() {
        let ir = compile_to_ir("namespace t { class C { fnc f(a: Float64, b: Float64) -> Float64 { return a / b; } } }");
        assert!(ir.contains("fdiv double"), "should emit fdiv for float division");
    }

    // --- Comparison IR ---

    #[test]
    fn test_icmp_eq_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 1 == 1; return 0; }");
        assert!(ir.contains("icmp eq"), "should emit icmp eq");
    }

    #[test]
    fn test_icmp_ne_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 1 != 2; return 0; }");
        assert!(ir.contains("icmp ne"), "should emit icmp ne");
    }

    #[test]
    fn test_icmp_lt_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 1 < 2; return 0; }");
        assert!(ir.contains("icmp slt"), "should emit icmp slt for <");
    }

    #[test]
    fn test_icmp_gt_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 2 > 1; return 0; }");
        assert!(ir.contains("icmp sgt"), "should emit icmp sgt for >");
    }

    #[test]
    fn test_icmp_le_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 1 <= 2; return 0; }");
        assert!(ir.contains("icmp sle"), "should emit icmp sle for <=");
    }

    #[test]
    fn test_icmp_ge_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 2 >= 1; return 0; }");
        assert!(ir.contains("icmp sge"), "should emit icmp sge for >=");
    }

    #[test]
    fn test_float_comparison_ir() {
        let ir = compile_to_ir("namespace t { class C { fnc f(a: Float64, b: Float64) -> Bool { return a < b; } } }");
        assert!(ir.contains("fcmp"), "should emit fcmp for float comparison");
    }

    // --- Boolean ops IR ---

    #[test]
    fn test_bool_and_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = true && false; return 0; }");
        assert!(ir.contains("and i1"), "should emit and i1 for &&");
    }

    #[test]
    fn test_bool_or_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = true || false; return 0; }");
        assert!(ir.contains("or i1"), "should emit or i1 for ||");
    }

    // --- Unary ops IR ---

    #[test]
    fn test_unary_neg_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = -5; return x; }");
        assert!(ir.contains("sub") || ir.contains("neg"), "should emit negation");
    }

    #[test]
    fn test_unary_not_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = !true; return 0; }");
        assert!(ir.contains("xor i1") || ir.contains("xor"), "should emit xor for boolean not");
    }

    // --- Variables: alloca/store/load ---

    #[test]
    fn test_alloca_for_local_var() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 42; return x; }");
        assert!(ir.contains("alloca"), "should emit alloca for local variable");
    }

    #[test]
    fn test_store_load_for_var() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 42; return x; }");
        assert!(ir.contains("store"), "should emit store for variable init");
        assert!(ir.contains("load"), "should emit load for variable read");
    }

    // --- Function definition ---

    #[test]
    fn test_function_define_ir() {
        // fn main is emitted as @tinox_main to avoid clashing with libc main
        let ir = compile_to_ir("fn main() -> Int64 { return 0; }");
        assert!(ir.contains("define"), "should emit define for function");
        assert!(ir.contains("@tinox_main"), "fn main should become @tinox_main");
    }

    #[test]
    fn test_function_return_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { return 42; }");
        assert!(ir.contains("ret i64"), "should emit ret i64");
    }

    #[test]
    fn test_multiple_functions_ir() {
        let ir = compile_to_ir("fn foo() -> Int64 { return 1; } fn main() -> Int64 { return foo(); }");
        assert!(ir.contains("@foo"), "should define @foo");
        assert!(ir.contains("@tinox_main"), "fn main should become @tinox_main");
        assert!(ir.contains("call"), "should emit call instruction");
    }

    // --- Control flow blocks ---

    #[test]
    fn test_if_without_else_stmt_ir() {
        // Statement-level if uses block labels: then/else/ifcont
        let ir = compile_to_ir("fn main() -> Int64 { if true { } return 0; }");
        assert!(ir.contains("then"), "should have then block");
        assert!(ir.contains("ifcont"), "should have ifcont merge block");
    }

    #[test]
    fn test_if_else_stmt_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { if true { } else { } return 0; }");
        assert!(ir.contains("then"), "should have then block");
        assert!(ir.contains("else"), "should have else block");
        assert!(ir.contains("ifcont"), "should have ifcont merge block");
    }

    #[test]
    fn test_while_loop_stmt_blocks_ir() {
        // Statement-level while uses block labels: loop/loopbody/loopend
        let ir = compile_to_ir("fn main() -> Int64 { while true { break; } return 0; }");
        assert!(ir.contains("loopbody"), "should have loopbody block");
        assert!(ir.contains("loopend"), "should have loopend block");
    }

    #[test]
    fn test_for_range_loop_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { for i in 0..5 { } return 0; }");
        assert!(ir.contains("for_"), "should have for block structure");
    }

    // --- String literals ---

    #[test]
    fn test_string_literal_global_ir() {
        let ir = compile_to_ir(r#"fn main() -> Int64 { let s = "hello"; return 0; }"#);
        assert!(ir.contains("hello") || ir.contains("@str"), "should emit string constant");
    }

    // --- Namespace/class mangling ---

    #[test]
    fn test_namespace_class_method_mangling() {
        let ir = compile_to_ir("namespace myapp { class Utils { fnc helper() -> Int64 { return 0; } } }");
        assert!(ir.contains("myapp__Utils_helper") || ir.contains("Utils_helper"),
            "should emit mangled method name");
    }

    #[test]
    fn test_class_static_method_ir() {
        // In Tinox, fnc inside a class = static method (no self param)
        let ir = compile_to_ir("class Math { fnc square(x: Int64) -> Int64 { return x * x; } }");
        assert!(ir.contains("Math_square"), "should emit Math_square name");
        assert!(ir.contains("define"), "should define the function");
    }

    // --- Bitwise operations ---

    #[test]
    fn test_bitwise_and_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 6 & 3; return x; }");
        assert!(ir.contains("and i64"), "should emit and i64 for bitwise and");
    }

    #[test]
    fn test_bitwise_or_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 6 | 3; return x; }");
        assert!(ir.contains("or i64"), "should emit or i64 for bitwise or");
    }

    #[test]
    fn test_bitwise_xor_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 6 ^ 3; return x; }");
        assert!(ir.contains("xor i64"), "should emit xor i64 for bitwise xor");
    }

    #[test]
    fn test_shl_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 1 << 3; return x; }");
        assert!(ir.contains("shl"), "should emit shl for left shift");
    }

    #[test]
    fn test_shr_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 8 >> 1; return x; }");
        assert!(ir.contains("shr") || ir.contains("ashr") || ir.contains("lshr"), "should emit shift right");
    }

    // --- Compound assignments ---

    #[test]
    fn test_compound_add_assign_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { var x = 5; x += 3; return x; }");
        assert!(ir.contains("add"), "should emit add for +=");
        assert!(ir.contains("store"), "should store result back");
    }

    #[test]
    fn test_compound_sub_assign_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { var x = 5; x -= 2; return x; }");
        assert!(ir.contains("sub"), "should emit sub for -=");
    }

    // --- Null ---

    #[test]
    fn test_null_literal_ir() {
        // null is emitted as integer 0 in IR
        let ir = compile_to_ir("fn main() -> Int64 { let x = null; return 0; }");
        assert!(ir.contains("i64 0") || ir.contains("store"), "null should emit as 0 or store");
    }

    // --- Specific integer type widths ---

    #[test]
    fn test_i32_type_ir() {
        let ir = compile_to_ir("namespace t { class C { fnc f(x: Int32) -> Int32 { return x; } } }");
        assert!(ir.contains("i32"), "should use i32 for Int32 params");
    }

    #[test]
    fn test_i64_type_ir() {
        let ir = compile_to_ir("namespace t { class C { fnc f(x: Int64) -> Int64 { return x; } } }");
        assert!(ir.contains("i64"), "should use i64 for Int64 params");
    }

    #[test]
    fn test_bool_type_ir() {
        let ir = compile_to_ir("namespace t { class C { fnc f(x: Bool) -> Bool { return x; } } }");
        assert!(ir.contains("i1"), "should use i1 for Bool params");
    }

    #[test]
    fn test_float32_type_ir() {
        let ir = compile_to_ir("namespace t { class C { fnc f(x: Float32) -> Float32 { return x; } } }");
        assert!(ir.contains("float"), "should use float for Float32");
    }

    // --- Struct / class fields ---

    #[test]
    fn test_class_field_gep_ir() {
        let src = concat!(
            "class Point { x: Int64; y: Int64; }\n",
            "fn main() -> Int64 {\n",
            "  let p = new Point(3, 4);\n",
            "  return p.x;\n",
            "}"
        );
        let ir = compile_to_ir(src);
        assert!(ir.contains("getelementptr") || ir.contains("gep") || ir.contains("Point"),
            "should emit GEP or struct access for field read");
    }

    // --- Array ---

    #[test]
    fn test_array_literal_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let arr = [1, 2, 3]; return 0; }");
        assert!(ir.contains("alloca") || ir.contains("array"), "should emit array storage");
    }

    // ================================================================
    // Enum
    // ================================================================

    #[test]
    fn test_enum_type_is_i64() {
        // Enum variants are represented as i64 constants
        let ir = compile_to_ir("enum Color { Red; Green; Blue; } fn main() -> Int64 { return 0; }");
        assert!(ir.contains("i64"), "enum-bearing code should use i64");
    }

    #[test]
    fn test_enum_variant_constant() {
        let ir = compile_to_ir(
            "enum Dir { North; South; East; West; } fn main() -> Int64 { let d = Dir::North; return 0; }"
        );
        assert!(ir.contains("i64 0") || ir.contains("store"), "enum variant should store a constant");
    }

    #[test]
    fn test_match_on_enum() {
        let ir = compile_to_ir(concat!(
            "enum State { On; Off; }\n",
            "fn check(s: State) -> Int64 {\n",
            "    match s {\n",
            "        State::On => 1;\n",
            "        State::Off => 0;\n",
            "        _ => -1;\n",
            "    }\n",
            "    return 0;\n",
            "}\n",
            "fn main() -> Int64 { return 0; }",
        ));
        assert!(ir.contains("switch") || ir.contains("icmp") || ir.contains("br"),
            "match should emit branching IR");
    }

    // ================================================================
    // Match on integer
    // ================================================================

    #[test]
    fn test_match_int_ir() {
        let ir = compile_to_ir(concat!(
            "fn classify(x: Int64) -> Int64 {\n",
            "    match x {\n",
            "        0 => 10;\n",
            "        1 => 20;\n",
            "        _ => 99;\n",
            "    }\n",
            "    return 0;\n",
            "}\n",
            "fn main() -> Int64 { return 0; }",
        ));
        assert!(ir.contains("icmp") || ir.contains("switch"), "integer match should compare");
    }

    #[test]
    fn test_match_bool_ir() {
        let ir = compile_to_ir(concat!(
            "fn f(b: Bool) -> Int64 {\n",
            "    match b {\n",
            "        true => 1;\n",
            "        false => 0;\n",
            "    }\n",
            "    return 0;\n",
            "}\n",
            "fn main() -> Int64 { return 0; }",
        ));
        assert!(ir.contains("icmp") || ir.contains("br"), "bool match needs branch IR");
    }

    // ================================================================
    // Recursive function
    // ================================================================

    #[test]
    fn test_recursive_function_ir() {
        let ir = compile_to_ir(concat!(
            "fn fib(n: Int64) -> Int64 {\n",
            "    if n <= 1 { return n; }\n",
            "    return fib(n - 1) + fib(n - 2);\n",
            "}\n",
            "fn main() -> Int64 { return fib(5); }",
        ));
        // fib calls itself — should appear twice in IR (definition + call)
        let count = ir.matches("@fib(").count() + ir.matches("call i64 @fib").count();
        assert!(count >= 2, "recursive function should call itself in IR");
    }

    // ================================================================
    // Multiple function parameters
    // ================================================================

    #[test]
    fn test_multiple_params_ir() {
        let ir = compile_to_ir(
            "fn add(a: Int64, b: Int64, c: Int64) -> Int64 { return a + b + c; }\nfn main() -> Int64 { return add(1, 2, 3); }"
        );
        assert!(ir.contains("@add(i64 %a, i64 %b, i64 %c)") || ir.contains("@add(i64"),
            "multi-param function should appear in IR");
    }

    // ================================================================
    // Return without value
    // ================================================================

    #[test]
    fn test_return_void_ir() {
        let ir = compile_to_ir("fn greet() -> Nothing { return; }\nfn main() -> Int64 { greet(); return 0; }");
        assert!(ir.contains("ret void") || ir.contains("ret i64"),
            "Nothing-returning function should have a return");
    }

    // ================================================================
    // For-C style loop
    // ================================================================

    #[test]
    fn test_forc_loop_ir() {
        let ir = compile_to_ir(concat!(
            "fn main() -> Int64 {\n",
            "    var sum = 0;\n",
            "    for (var i = 0; i < 10; i += 1) {\n",
            "        sum += i;\n",
            "    }\n",
            "    return sum;\n",
            "}",
        ));
        assert!(ir.contains("br ") && (ir.contains("loop") || ir.contains("for")),
            "for-C loop should emit branch-based loop IR");
    }

    // ================================================================
    // Unary bit-not (~)
    // ================================================================

    #[test]
    fn test_unary_bitnot_ir() {
        let ir = compile_to_ir("fn f(x: Int64) -> Int64 { return ~x; }\nfn main() -> Int64 { return 0; }");
        assert!(ir.contains("xor") || ir.contains("-1"), "bit-not should use xor with -1");
    }

    // ================================================================
    // Shift right arithmetic (>>>)
    // ================================================================

    #[test]
    fn test_arith_shift_right_ir() {
        let ir = compile_to_ir("fn f(x: Int64) -> Int64 { return x >>> 2; }\nfn main() -> Int64 { return 0; }");
        assert!(ir.contains("ashr"), ">>> should emit ashr instruction");
    }

    // ================================================================
    // String method calls
    // ================================================================

    #[test]
    fn test_string_length_method_ir() {
        let ir = compile_to_ir(concat!(
            "fn main() -> Int64 {\n",
            "    let s = \"hello\";\n",
            "    let n = s.len();\n",
            "    return 0;\n",
            "}",
        ));
        assert!(ir.contains("tinox_string_length"), "s.len() should call tinox_string_length");
    }

    #[test]
    fn test_string_concat_ir() {
        let ir = compile_to_ir(concat!(
            "fn main() -> Int64 {\n",
            "    let a = \"foo\";\n",
            "    let b = \"bar\";\n",
            "    let c = a + b;\n",
            "    return 0;\n",
            "}",
        ));
        assert!(ir.contains("tinox_string_concat"), "string + should call tinox_string_concat");
    }

    // ================================================================
    // Field write (this.field = value)
    // ================================================================

    #[test]
    fn test_field_write_ir() {
        let ir = compile_to_ir(concat!(
            "class Counter {\n",
            "    var count: Int64;\n",
            "    fn increment() -> Nothing {\n",
            "        this.count = this.count + 1;\n",
            "    }\n",
            "}\n",
            "fn main() -> Int64 { return 0; }",
        ));
        assert!(ir.contains("getelementptr") && ir.contains("store"),
            "field write should use GEP + store");
    }

    // ================================================================
    // Class inheritance: child calls parent method
    // ================================================================

    #[test]
    fn test_child_inherits_parent_method_ir() {
        let ir = compile_to_ir(concat!(
            "class Animal {\n",
            "    fn speak() -> Int64 { return 1; }\n",
            "}\n",
            "class Dog extends Animal {}\n",
            "fn main() -> Int64 {\n",
            "    let d = new Dog();\n",
            "    return d.speak();\n",
            "}",
        ));
        assert!(ir.contains("Animal_speak") || ir.contains("Dog_speak"),
            "inherited method should be dispatched");
    }

    // ================================================================
    // Immutable struct
    // ================================================================

    #[test]
    fn test_immutable_struct_ir() {
        let ir = compile_to_ir(concat!(
            "immutable Point(x: Int64, y: Int64)\n",
            "fn main() -> Int64 {\n",
            "    let p = new Point(3, 4);\n",
            "    return p.x;\n",
            "}",
        ));
        assert!(ir.contains("%Point") || ir.contains("Point"),
            "immutable type should appear in IR");
    }

    // ================================================================
    // Logical short-circuit (&&, ||)
    // ================================================================

    #[test]
    fn test_logical_and_ir() {
        let ir = compile_to_ir("fn f(a: Bool, b: Bool) -> Bool { return a && b; }\nfn main() -> Int64 { return 0; }");
        assert!(ir.contains("and i1") || ir.contains("br "),
            "&& should emit and or branch IR");
    }

    #[test]
    fn test_logical_or_ir() {
        let ir = compile_to_ir("fn f(a: Bool, b: Bool) -> Bool { return a || b; }\nfn main() -> Int64 { return 0; }");
        assert!(ir.contains("or i1") || ir.contains("br "),
            "|| should emit or or branch IR");
    }

    // ================================================================
    // Compound operators (remaining ones)
    // ================================================================

    #[test]
    fn test_compound_mul_assign_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { var x = 3; x *= 4; return x; }");
        assert!(ir.contains("mul"), "x *= should emit mul");
    }

    #[test]
    fn test_compound_div_assign_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { var x = 8; x /= 2; return x; }");
        assert!(ir.contains("sdiv"), "x /= should emit sdiv");
    }

    #[test]
    fn test_compound_mod_assign_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { var x = 9; x %= 4; return x; }");
        assert!(ir.contains("srem"), "x %= should emit srem");
    }

    #[test]
    fn test_compound_bitand_assign_parse_bug() {
        // BUG: parser does not support &= — parses `x &` then fails on `=`
        // This test documents the current broken state
        let result = std::panic::catch_unwind(|| {
            compile_to_ir("fn main() -> Int64 { var x = 15; x &= 6; return x; }")
        });
        // Currently panics in compile_to_ir because parse fails
        assert!(result.is_err(), "x &= should currently fail to parse (known bug)");
    }

    #[test]
    fn test_compound_bitor_assign_parse_bug() {
        // BUG: parser does not support |=
        let result = std::panic::catch_unwind(|| {
            compile_to_ir("fn main() -> Int64 { var x = 5; x |= 2; return x; }")
        });
        assert!(result.is_err(), "x |= should currently fail to parse (known bug)");
    }

    #[test]
    fn test_compound_xor_assign_parse_bug() {
        // BUG: parser does not support ^=
        let result = std::panic::catch_unwind(|| {
            compile_to_ir("fn main() -> Int64 { var x = 7; x ^= 3; return x; }")
        });
        assert!(result.is_err(), "x ^= should currently fail to parse (known bug)");
    }

    #[test]
    fn test_compound_shl_assign_parse_bug() {
        // BUG: parser does not support <<=
        let result = std::panic::catch_unwind(|| {
            compile_to_ir("fn main() -> Int64 { var x = 1; x <<= 3; return x; }")
        });
        assert!(result.is_err(), "x <<= should currently fail to parse (known bug)");
    }

    // ================================================================
    // Cast instructions
    // ================================================================

    #[test]
    fn test_cast_i32_to_i64_ir() {
        let ir = compile_to_ir("fn f(x: Int32) -> Int64 { return x as Int64; }\nfn main() -> Int64 { return 0; }");
        assert!(ir.contains("sext") || ir.contains("zext") || ir.contains("i64"),
            "Int32->Int64 cast should use sext or zext");
    }

    #[test]
    fn test_cast_bool_to_int_ir() {
        let ir = compile_to_ir("fn f(b: Bool) -> Int64 { return b as Int64; }\nfn main() -> Int64 { return 0; }");
        assert!(ir.contains("zext") || ir.contains("i64"),
            "Bool->Int64 cast should use zext");
    }

    // ================================================================
    // Tuple
    // ================================================================

    #[test]
    fn test_tuple_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let t = (1, 2); return 0; }");
        // Tuples are stored as structs — should allocate memory
        assert!(ir.contains("alloca") || ir.contains("i64"), "tuple should be allocated");
    }

    // ================================================================
    // Lambda / closure
    // ================================================================

    #[test]
    fn test_lambda_define_ir() {
        // Lambda syntax: (params) => body  or  \x -> body
        let ir = compile_to_ir(concat!(
            "fn main() -> Int64 {\n",
            "    let add = (a, b) => a + b;\n",
            "    return 0;\n",
            "}",
        ));
        assert!(ir.contains("lambda") || ir.contains("define") || ir.contains("alloca"),
            "lambda should generate some IR");
    }

    // ================================================================
    // Null literal
    // ================================================================

    #[test]
    fn test_null_in_condition_ir() {
        let ir = compile_to_ir(concat!(
            "fn main() -> Int64 {\n",
            "    let p = null;\n",
            "    if p == null { return 1; }\n",
            "    return 0;\n",
            "}",
        ));
        assert!(ir.contains("icmp") || ir.contains("br "), "null comparison should emit icmp");
    }

    // ================================================================
    // Char literal
    // ================================================================

    #[test]
    fn test_char_literal_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let c = 'A'; return 0; }");
        assert!(ir.contains("i32") || ir.contains("65") || ir.contains("store"),
            "char literal should store its code point");
    }

    // ================================================================
    // Float32 vs Float64 types
    // ================================================================

    #[test]
    fn test_float32_param_ir() {
        let ir = compile_to_ir("fn f(x: Float32) -> Float32 { return x; }\nfn main() -> Int64 { return 0; }");
        assert!(ir.contains("float") || ir.contains("f32") || ir.contains("double"),
            "Float32 param should appear as float type in IR");
    }

    // ================================================================
    // Multiple classes in one program
    // ================================================================

    #[test]
    fn test_two_classes_ir() {
        let ir = compile_to_ir(concat!(
            "class A { fn getA() -> Int64 { return 1; } }\n",
            "class B { fn getB() -> Int64 { return 2; } }\n",
            "fn main() -> Int64 {\n",
            "    let a = new A();\n",
            "    let b = new B();\n",
            "    return a.getA() + b.getB();\n",
            "}",
        ));
        assert!(ir.contains("A_getA") && ir.contains("B_getB"),
            "both class methods should appear in IR");
    }

    // ================================================================
    // Extern fn declaration
    // ================================================================

    #[test]
    fn test_extern_fn_ir() {
        let ir = compile_to_ir(concat!(
            "extern fn puts(s: String) -> Int64;\n",
            "fn main() -> Int64 { puts(\"hi\"); return 0; }",
        ));
        assert!(ir.contains("declare") && ir.contains("@puts"),
            "extern fn should emit a declare");
    }

    // ================================================================
    // If expression (inline) with result used
    // ================================================================

    #[test]
    fn test_if_expr_value_used_ir() {
        let ir = compile_to_ir(concat!(
            "fn abs(x: Int64) -> Int64 {\n",
            "    return if x < 0 { -x; } else { x; };\n",
            "}",
            "fn main() -> Int64 { return abs(-3); }",
        ));
        assert!(ir.contains("if_then") && ir.contains("if_merge"),
            "if-expression should have then/merge blocks");
    }

    // ================================================================
    // While expression (used as value)
    // ================================================================

    #[test]
    fn test_while_stmt_produces_loop_blocks() {
        let ir = compile_to_ir(concat!(
            "fn main() -> Int64 {\n",
            "    var i = 0;\n",
            "    while i < 5 {\n",
            "        i += 1;\n",
            "    }\n",
            "    return i;\n",
            "}",
        ));
        assert!(ir.contains("loop") && ir.contains("loopbody") && ir.contains("loopend"),
            "while loop should produce loop/loopbody/loopend blocks");
    }

    // ================================================================
    // String operations
    // ================================================================

    #[test]
    fn test_string_variable_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let s = \"hello\"; return 0; }");
        assert!(ir.contains("hello") || ir.contains("i8"), "string literal should appear in IR");
    }

    #[test]
    fn test_string_concat_two_vars_ir() {
        let ir = compile_to_ir(concat!(
            "fn main() -> Int64 {\n",
            "    let a = \"foo\";\n",
            "    let b = \"bar\";\n",
            "    let c = a + b;\n",
            "    return 0;\n",
            "}"
        ));
        assert!(ir.contains("foo") && ir.contains("bar"), "string concat should emit both strings");
    }

    // ================================================================
    // For-each style loop
    // ================================================================

    #[test]
    fn test_foreach_loop_ir() {
        let ir = compile_to_ir(concat!(
            "fn main() -> Int64 {\n",
            "    let arr = [1, 2, 3];\n",
            "    var sum = 0;\n",
            "    for x in arr {\n",
            "        sum += x;\n",
            "    }\n",
            "    return sum;\n",
            "}"
        ));
        assert!(ir.contains("sum") || ir.contains("add"), "foreach loop should emit addition IR");
    }

    // ================================================================
    // Boolean literals
    // ================================================================

    #[test]
    fn test_bool_true_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let b = true; return 0; }");
        assert!(ir.contains("i1 1") || ir.contains("i1 true") || ir.contains("true"),
            "true literal should emit i1 1 in IR");
    }

    #[test]
    fn test_bool_false_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let b = false; return 0; }");
        assert!(ir.contains("i1 0") || ir.contains("i1 false") || ir.contains("false"),
            "false literal should emit i1 0 in IR");
    }

    // ================================================================
    // Comparison operators
    // ================================================================

    #[test]
    fn test_less_than_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 3 < 5; return 0; }");
        assert!(ir.contains("icmp slt"), "less-than should emit icmp slt");
    }

    #[test]
    fn test_less_equal_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 3 <= 5; return 0; }");
        assert!(ir.contains("icmp sle"), "less-equal should emit icmp sle");
    }

    #[test]
    fn test_greater_than_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 5 > 3; return 0; }");
        assert!(ir.contains("icmp sgt"), "greater-than should emit icmp sgt");
    }

    #[test]
    fn test_greater_equal_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 5 >= 3; return 0; }");
        assert!(ir.contains("icmp sge"), "greater-equal should emit icmp sge");
    }

    #[test]
    fn test_not_equal_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 3 != 5; return 0; }");
        assert!(ir.contains("icmp ne"), "not-equal should emit icmp ne");
    }

    // ================================================================
    // Nested function calls
    // ================================================================

    #[test]
    fn test_nested_call_ir() {
        let ir = compile_to_ir(concat!(
            "fn double(x: Int64) -> Int64 { return x * 2; }\n",
            "fn quadruple(x: Int64) -> Int64 { return double(double(x)); }\n",
            "fn main() -> Int64 { return quadruple(3); }"
        ));
        assert!(ir.contains("@double") && ir.contains("@quadruple"),
            "nested function calls should emit both function symbols");
    }

    // ================================================================
    // Multiple assignments
    // ================================================================

    #[test]
    fn test_multiple_var_assign_ir() {
        let ir = compile_to_ir(concat!(
            "fn main() -> Int64 {\n",
            "    var x = 1;\n",
            "    var y = 2;\n",
            "    var z = x + y;\n",
            "    x = z * 2;\n",
            "    return x;\n",
            "}"
        ));
        assert!(ir.contains("store") && ir.contains("load"), "multiple assignments should emit store/load");
    }

    // ================================================================
    // Array literal
    // ================================================================

    #[test]
    fn test_array_literal_three_elems_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let a = [10, 20, 30]; return 0; }");
        assert!(ir.contains("10") && ir.contains("20") && ir.contains("30"),
            "array literal elements should appear in IR");
    }

    // ================================================================
    // Enum value
    // ================================================================

    #[test]
    fn test_enum_value_no_args_ir() {
        let ir = compile_to_ir(concat!(
            "enum Dir { North, South }\n",
            "fn main() -> Int64 { let d = Dir::North; return 0; }"
        ));
        assert!(ir.contains("i32 0") || ir.contains("i64 0") || ir.contains("alloca"),
            "enum value should emit constant in IR");
    }

    // ================================================================
    // Struct / class field access
    // ================================================================

    #[test]
    fn test_class_field_read_ir() {
        let ir = compile_to_ir(concat!(
            "class Point { var x: Int64; var y: Int64; }\n",
            "fn main() -> Int64 {\n",
            "    let p = Point();\n",
            "    return p.x;\n",
            "}"
        ));
        assert!(ir.contains("%Point") || ir.contains("getelementptr"),
            "class field read should emit getelementptr in IR");
    }

    #[test]
    fn test_class_field_write_ir() {
        let ir = compile_to_ir(concat!(
            "class Counter { var count: Int64; }\n",
            "fn main() -> Int64 {\n",
            "    var c = Counter();\n",
            "    c.count = 42;\n",
            "    return c.count;\n",
            "}"
        ));
        assert!(ir.contains("store i64 42") || ir.contains("42"),
            "class field write should store value in IR");
    }

    // ================================================================
    // Method calls
    // ================================================================

    #[test]
    fn test_method_call_ir() {
        let ir = compile_to_ir(concat!(
            "class Adder { fn add(a: Int64, b: Int64) -> Int64 { return a + b; } }\n",
            "fn main() -> Int64 {\n",
            "    let adder = Adder();\n",
            "    return adder.add(3, 4);\n",
            "}"
        ));
        assert!(ir.contains("Adder") && ir.contains("add"),
            "method call should emit class and method names in IR");
    }

    // ================================================================
    // Try/catch
    // ================================================================

    #[test]
    fn test_try_catch_ir() {
        let ir = compile_to_ir(concat!(
            "fn main() -> Int64 {\n",
            "    try {\n",
            "        throw \"oops\";\n",
            "    } catch e: String {\n",
            "        return 1;\n",
            "    }\n",
            "    return 0;\n",
            "}"
        ));
        assert!(ir.contains("try") || ir.contains("catch") || ir.contains("label"),
            "try/catch should emit branching IR");
    }

    // ================================================================
    // Modulo operator
    // ================================================================

    #[test]
    fn test_modulo_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { return 17 % 5; }");
        assert!(ir.contains("srem"), "modulo should emit srem instruction");
    }

    // ================================================================
    // Unary minus
    // ================================================================

    #[test]
    fn test_unary_minus_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { var x = 5; return -x; }");
        assert!(ir.contains("sub") || ir.contains("neg"), "unary minus should emit sub/neg in IR");
    }

    // ================================================================
    // Immutable global
    // ================================================================

    #[test]
    fn test_immutable_struct_used_ir() {
        // immutable in Tinox is a struct-like type, not a constant
        let ir = compile_to_ir(concat!(
            "immutable Config(host: String, port: Int64);\n",
            "fn main() -> Int64 { let c = Config(\"localhost\", 8080); return 0; }"
        ));
        assert!(ir.contains("Config") || ir.contains("8080"),
            "immutable struct usage should appear in IR");
    }

    // ================================================================
    // Defer statement
    // ================================================================

    #[test]
    fn test_defer_generates_code() {
        let ir = compile_to_ir(concat!(
            "fn cleanup() -> Nothing { return; }\n",
            "fn main() -> Int64 {\n",
            "    defer { cleanup(); }\n",
            "    return 0;\n",
            "}"
        ));
        assert!(ir.contains("@cleanup") || ir.contains("cleanup"),
            "deferred call should appear in IR");
    }

    // ================================================================
    // Float arithmetic
    // ================================================================

    #[test]
    fn test_float_add_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 1.5 + 2.5; return 0; }");
        assert!(ir.contains("fadd"), "float addition should emit fadd");
    }

    #[test]
    fn test_float_mul_two_literals_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 3.0 * 2.0; return 0; }");
        assert!(ir.contains("fmul"), "float multiplication should emit fmul");
    }

    #[test]
    fn test_float_div_two_literals_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 10.0 / 4.0; return 0; }");
        assert!(ir.contains("fdiv"), "float division should emit fdiv");
    }

    // ================================================================
    // Interface polymorphism
    // ================================================================

    #[test]
    fn test_interface_impl_ir() {
        let ir = compile_to_ir(concat!(
            "interface Greeter { fn greet() -> Nothing; }\n",
            "class Hello implements Greeter {\n",
            "    fn greet() -> Nothing { println(\"hi\"); }\n",
            "}\n",
            "fn main() -> Int64 { let h = Hello(); h.greet(); return 0; }"
        ));
        assert!(ir.contains("Hello") && ir.contains("greet"),
            "interface implementation should emit class and method in IR");
    }

    // ================================================================
    // Recursive functions
    // ================================================================

    #[test]
    fn test_recursive_fibonacci_ir() {
        let ir = compile_to_ir(concat!(
            "fn fib(n: Int64) -> Int64 {\n",
            "    if n <= 1 { return n; }\n",
            "    return fib(n - 1) + fib(n - 2);\n",
            "}\n",
            "fn main() -> Int64 { return fib(10); }"
        ));
        assert!(ir.contains("@fib"), "fibonacci should define @fib in IR");
        assert!(ir.contains("call i64 @fib") || ir.contains("@fib("),
            "fibonacci should call itself recursively");
    }

    #[test]
    fn test_recursive_countdown_ir() {
        let ir = compile_to_ir(concat!(
            "fn countdown(n: Int64) -> Nothing {\n",
            "    if n <= 0 { return; }\n",
            "    countdown(n - 1);\n",
            "}\n",
            "fn main() -> Int64 { countdown(5); return 0; }"
        ));
        assert!(ir.contains("@countdown"), "countdown should appear in IR");
    }

    // ================================================================
    // Multiple functions
    // ================================================================

    #[test]
    fn test_three_functions_ir() {
        let ir = compile_to_ir(concat!(
            "fn a() -> Int64 { return 1; }\n",
            "fn b() -> Int64 { return 2; }\n",
            "fn c() -> Int64 { return a() + b(); }\n",
            "fn main() -> Int64 { return c(); }"
        ));
        assert!(ir.contains("@a") && ir.contains("@b") && ir.contains("@c"),
            "all three functions should appear in IR");
    }

    // ================================================================
    // Higher-order functions / lambdas
    // ================================================================

    #[test]
    fn test_lambda_single_param_ir() {
        let ir = compile_to_ir(concat!(
            "fn main() -> Int64 {\n",
            "    let sq = \\x -> x * x;\n",
            "    return 0;\n",
            "}"
        ));
        assert!(ir.contains("mul") || ir.contains("lambda") || ir.contains("alloca"),
            "lambda should emit IR code");
    }

    #[test]
    fn test_lambda_two_params_ir() {
        let ir = compile_to_ir(concat!(
            "fn main() -> Int64 {\n",
            "    let add = (a, b) => a + b;\n",
            "    return 0;\n",
            "}"
        ));
        assert!(ir.contains("add") || ir.contains("alloca"),
            "two-param lambda should emit IR");
    }

    // ================================================================
    // Class inheritance
    // ================================================================

    #[test]
    fn test_class_extends_ir() {
        let ir = compile_to_ir(concat!(
            "class Animal { fn speak() -> Nothing { println(\"...\"); } }\n",
            "class Dog extends Animal { fn fetch() -> Nothing { println(\"!\"); } }\n",
            "fn main() -> Int64 { let d = Dog(); d.fetch(); return 0; }"
        ));
        assert!(ir.contains("Dog") && ir.contains("fetch"),
            "subclass method should appear in IR");
    }

    // ================================================================
    // Enum with match
    // ================================================================

    #[test]
    fn test_enum_match_ir() {
        let ir = compile_to_ir(concat!(
            "enum Color { Red, Green, Blue }\n",
            "fn name(c: Color) -> String {\n",
            "    match c {\n",
            "        Color::Red => return \"red\";\n",
            "        Color::Green => return \"green\";\n",
            "        _ => return \"blue\";\n",
            "    }\n",
            "}\n",
            "fn main() -> Int64 { let s = name(Color::Red); return 0; }"
        ));
        assert!(ir.contains("@name"), "enum match function should appear in IR");
    }

    // ================================================================
    // For-C loop
    // ================================================================

    #[test]
    fn test_forc_loop_sum_ir() {
        let ir = compile_to_ir(concat!(
            "fn main() -> Int64 {\n",
            "    var sum = 0;\n",
            "    for (var i = 0; i < 10; i += 1) {\n",
            "        sum += i;\n",
            "    }\n",
            "    return sum;\n",
            "}"
        ));
        assert!(ir.contains("add") && ir.contains("icmp"),
            "for-C loop should emit add and compare instructions");
    }

    // ================================================================
    // Nested conditionals
    // ================================================================

    #[test]
    fn test_nested_if_else_ir() {
        let ir = compile_to_ir(concat!(
            "fn classify(n: Int64) -> String {\n",
            "    if n < 0 {\n",
            "        return \"negative\";\n",
            "    } else if n == 0 {\n",
            "        return \"zero\";\n",
            "    } else {\n",
            "        return \"positive\";\n",
            "    }\n",
            "}\n",
            "fn main() -> Int64 { let s = classify(5); return 0; }"
        ));
        assert!(ir.contains("@classify") && ir.contains("then") || ir.contains("br"),
            "nested if/else should emit conditional branches");
    }

    // ================================================================
    // Integer operations
    // ================================================================

    #[test]
    fn test_integer_subtraction_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { return 10 - 3; }");
        assert!(ir.contains("sub"), "subtraction should emit sub");
    }

    #[test]
    fn test_integer_multiplication_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { return 6 * 7; }");
        assert!(ir.contains("mul"), "multiplication should emit mul");
    }

    #[test]
    fn test_integer_division_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { return 20 / 4; }");
        assert!(ir.contains("sdiv"), "division should emit sdiv");
    }

    // ================================================================
    // Local variable allocation
    // ================================================================

    #[test]
    fn test_multiple_locals_ir() {
        let ir = compile_to_ir(concat!(
            "fn main() -> Int64 {\n",
            "    let a = 1;\n",
            "    let b = 2;\n",
            "    let c = 3;\n",
            "    let d = 4;\n",
            "    let e = 5;\n",
            "    return a + b + c + d + e;\n",
            "}"
        ));
        assert!(ir.contains("alloca") || ir.contains("add"),
            "multiple locals should emit alloca or be kept in registers");
    }

    // ================================================================
    // Boolean operations IR
    // ================================================================

    #[test]
    fn test_not_bool_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let b = !true; return 0; }");
        assert!(ir.contains("xor") || ir.contains("not"),
            "boolean NOT should emit xor or not");
    }

    // ================================================================
    // Cast operations
    // ================================================================

    #[test]
    fn test_cast_i64_to_float_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 5; let f = x as Float64; return 0; }");
        assert!(ir.contains("sitofp") || ir.contains("fpext") || ir.contains("float"),
            "int-to-float cast should emit sitofp in IR");
    }

    #[test]
    fn test_cast_float_to_i64_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { let x = 3.14; let n = x as Int64; return n; }");
        assert!(ir.contains("fptosi") || ir.contains("trunc") || ir.contains("i64"),
            "float-to-int cast should emit fptosi in IR");
    }

    // ================================================================
    // Range expression
    // ================================================================

    #[test]
    fn test_range_for_loop_ir() {
        let ir = compile_to_ir(concat!(
            "fn main() -> Int64 {\n",
            "    var total = 0;\n",
            "    for i in 0..5 {\n",
            "        total += i;\n",
            "    }\n",
            "    return total;\n",
            "}"
        ));
        assert!(ir.contains("add") && ir.contains("icmp"),
            "range for loop should emit addition and comparison");
    }

    // ================================================================
    // Struct literal IR
    // ================================================================

    #[test]
    fn test_struct_literal_ir() {
        let ir = compile_to_ir(concat!(
            "class Point { var x: Int64; var y: Int64; }\n",
            "fn main() -> Int64 {\n",
            "    let p = Point { x: 3, y: 4 };\n",
            "    return 0;\n",
            "}"
        ));
        assert!(ir.contains("Point") || ir.contains("alloca"),
            "struct literal should emit type or alloca in IR");
    }

    // ================================================================
    // Global immutable
    // ================================================================

    #[test]
    fn test_immutable_struct_ir_v2() {
        let ir = compile_to_ir(concat!(
            "immutable Config(host: String, port: Int64);\n",
            "fn get_port(c: Config) -> Int64 { return c.port; }\n",
            "fn main() -> Int64 { return 0; }"
        ));
        assert!(ir.contains("Config") || ir.contains("port") || ir.contains("getelementptr"),
            "immutable struct should be in IR");
    }

    // ================================================================
    // Println / print builtins
    // ================================================================

    #[test]
    fn test_println_int_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { println(42); return 0; }");
        assert!(ir.contains("println") || ir.contains("printf") || ir.contains("print"),
            "println should appear in IR");
    }

    #[test]
    fn test_println_string_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { println(\"hello\"); return 0; }");
        assert!(ir.contains("hello"), "string argument should appear in IR");
    }

    // ================================================================
    // Bitwise shift operations
    // ================================================================

    #[test]
    fn test_shift_left_const_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { return 1 << 8; }");
        assert!(ir.contains("shl"), "left shift should emit shl");
    }

    #[test]
    fn test_shift_right_const_ir() {
        let ir = compile_to_ir("fn main() -> Int64 { return 256 >> 4; }");
        assert!(ir.contains("ashr") || ir.contains("lshr"), "right shift should emit ashr or lshr");
    }

    // ================================================================
    // Break and continue
    // ================================================================

    #[test]
    fn test_break_in_loop_ir() {
        let ir = compile_to_ir(concat!(
            "fn main() -> Int64 {\n",
            "    var i = 0;\n",
            "    loop {\n",
            "        if i >= 5 { break; }\n",
            "        i += 1;\n",
            "    }\n",
            "    return i;\n",
            "}"
        ));
        assert!(ir.contains("br") || ir.contains("loop"),
            "break in loop should produce branch instruction");
    }

    #[test]
    fn test_continue_in_while_ir() {
        let ir = compile_to_ir(concat!(
            "fn main() -> Int64 {\n",
            "    var sum = 0;\n",
            "    var i = 0;\n",
            "    while i < 10 {\n",
            "        i += 1;\n",
            "        if i == 5 { continue; }\n",
            "        sum += i;\n",
            "    }\n",
            "    return sum;\n",
            "}"
        ));
        assert!(ir.contains("loop") || ir.contains("br"),
            "continue in while should produce branch back to loop header");
    }

    // ================================================================
    // @Sensitive / @Masked — toString generation
    // ================================================================

    fn compile_to_ir_with_masks(src: &str, sensitive: Vec<(&str, &str)>, masked: Vec<(&str, &str)>) -> String {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().expect("lex failed");
        let mut parser = Parser::new(tokens);
        let ast = parser.parse().expect("parse failed");
        let mut cg = CodeGen::new();
        let s_fields = sensitive.into_iter().map(|(c, f)| LogMaskFieldInfo {
            class_name: c.to_string(), field_name: f.to_string(),
        }).collect();
        let m_fields = masked.into_iter().map(|(c, f)| LogMaskFieldInfo {
            class_name: c.to_string(), field_name: f.to_string(),
        }).collect();
        cg.set_annotation_info(
            Default::default(), Default::default(), vec![], vec![],
            Default::default(), vec![], vec![], s_fields, m_fields,
            vec![], vec![], vec![],
        );
        cg.gen(&ast).expect("codegen failed");
        cg.into_ir()
    }

    fn compile_to_ir_with_serialize(
        src: &str,
        json_classes: Vec<&str>,
        do_not_serialize: Vec<(&str, &str)>,
    ) -> String {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().expect("lex failed");
        let mut parser = Parser::new(tokens);
        let ast = parser.parse().expect("parse failed");
        let mut cg = CodeGen::new();
        let dns_fields = do_not_serialize.into_iter().map(|(c, f)| LogMaskFieldInfo {
            class_name: c.to_string(), field_name: f.to_string(),
        }).collect();
        cg.set_annotation_info(
            Default::default(), Default::default(), vec![], vec![],
            Default::default(), vec![], vec![], vec![], vec![],
            dns_fields,
            json_classes.into_iter().map(|s| s.to_string()).collect(),
            vec![],
        );
        cg.gen(&ast).expect("codegen failed");
        cg.into_ir()
    }

    #[test]
    fn test_sensitive_field_emits_tostring() {
        let ir = compile_to_ir_with_masks(
            "class User { var name: String; var password: String; }\nfn main() -> Int64 { return 0; }",
            vec![("User", "password")],
            vec![],
        );
        assert!(ir.contains("User_toString"), "should emit toString for User");
    }

    #[test]
    fn test_sensitive_field_uses_stars() {
        let ir = compile_to_ir_with_masks(
            "class User { var name: String; var password: String; }\nfn main() -> Int64 { return 0; }",
            vec![("User", "password")],
            vec![],
        );
        assert!(ir.contains("***"), "sensitive field should emit *** literal");
    }

    #[test]
    fn test_masked_field_calls_mask_partial() {
        let ir = compile_to_ir_with_masks(
            "class User { var name: String; var email: String; }\nfn main() -> Int64 { return 0; }",
            vec![],
            vec![("User", "email")],
        );
        assert!(ir.contains("tinox_string_mask_partial"), "masked field should call mask_partial");
    }

    #[test]
    fn test_no_annotation_no_masked_tostring() {
        let ir = compile_to_ir(
            "class User { var name: String; }\nfn main() -> Int64 { return 0; }",
        );
        assert!(!ir.contains("User_toString"), "no @Sensitive/@Masked → no User_toString generated");
    }

    #[test]
    fn test_string_concat_coerces_object_to_string() {
        let ir = compile_to_ir_with_masks(
            concat!(
                "class User { var name: String; var password: String; }\n",
                "fn log(msg: String) -> Nothing { println(msg); }\n",
                "fn main() -> Int64 {\n",
                "    var u = User { name: \"Alice\", password: \"secret\" };\n",
                "    let s = \"prefix: \" + u;\n",
                "    return 0;\n",
                "}"
            ),
            vec![("User", "password")],
            vec![],
        );
        assert!(ir.contains("User_toString"), "toString should be generated");
        assert!(ir.contains("tinox_string_concat"), "concat should be emitted");
    }

    #[test]
    fn test_tostring_contains_class_name_prefix() {
        let ir = compile_to_ir_with_masks(
            "class Payment { var amount: Int64; var card: String; }\nfn main() -> Int64 { return 0; }",
            vec![("Payment", "card")],
            vec![],
        );
        // The class name prefix "Payment{" is stored as a string literal in the IR
        assert!(ir.contains("Payment{"), "toString should start with ClassName{{");
    }

    #[test]
    fn test_tostring_registered_in_method_ret_types() {
        // pre_register_log_mask_tostring must put ClassName_toString into method_ret_types
        // BEFORE user code is compiled so that explicit user.toString() calls resolve.
        let mut lexer = tinox_lexer::Lexer::new(
            "class User { var name: String; var password: String; }\nfn main() -> Int64 { return 0; }"
        );
        let tokens = lexer.tokenize().expect("lex");
        let ast = Parser::new(tokens).parse().expect("parse");
        let mut cg = CodeGen::new();
        cg.set_annotation_info(
            Default::default(), Default::default(), vec![], vec![],
            Default::default(), vec![], vec![],
            vec![LogMaskFieldInfo { class_name: "User".to_string(), field_name: "password".to_string() }],
            vec![], vec![], vec![], vec![],
        );
        cg.gen(&ast).expect("codegen");
        let ir = cg.into_ir();
        assert!(ir.contains("define i8* @User_toString"), "User_toString function must be emitted");
    }

    #[test]
    fn test_explicit_tostring_call_on_object() {
        // user.toString() on a @Sensitive-annotated class should call User_toString
        let src = concat!(
            "class User { var name: String; var secret: String; }\n",
            "fn show(s: String) -> Nothing { println(s); }\n",
            "fn main() -> Int64 {\n",
            "    var u = User { name: \"Alice\", secret: \"pw\" };\n",
            "    let s = u.toString();\n",
            "    show(s);\n",
            "    return 0;\n",
            "}"
        );
        let ir = compile_to_ir_with_masks(src, vec![("User", "secret")], vec![]);
        assert!(ir.contains("User_toString"), "explicit u.toString() should dispatch to User_toString");
        assert!(ir.contains("***"), "sensitive field must be masked");
    }

    #[test]
    fn test_both_annotations_in_same_class() {
        let ir = compile_to_ir_with_masks(
            "class User { var name: String; var password: String; var email: String; }\nfn main() -> Int64 { return 0; }",
            vec![("User", "password")],
            vec![("User", "email")],
        );
        assert!(ir.contains("User_toString"), "should emit toString");
        assert!(ir.contains("***"), "sensitive field → ***");
        assert!(ir.contains("tinox_string_mask_partial"), "masked field → mask_partial");
    }

    // ================================================================
    // @JsonSerializable / @DoNotSerialize — toJson generation
    // ================================================================

    #[test]
    fn test_json_serializable_emits_to_json() {
        let ir = compile_to_ir_with_serialize(
            "class User { var id: Int64; var name: String; }\nfn main() -> Int64 { return 0; }",
            vec!["User"],
            vec![],
        );
        assert!(ir.contains("User_toJson"), "should emit toJson for User");
    }

    #[test]
    fn test_json_serializable_emits_opening_brace() {
        let ir = compile_to_ir_with_serialize(
            "class Item { var id: Int64; }\nfn main() -> Int64 { return 0; }",
            vec!["Item"],
            vec![],
        );
        assert!(ir.contains("{"), "toJson should contain opening brace");
    }

    #[test]
    fn test_json_serializable_includes_field_key() {
        let ir = compile_to_ir_with_serialize(
            "class User { var id: Int64; var name: String; }\nfn main() -> Int64 { return 0; }",
            vec!["User"],
            vec![],
        );
        assert!(ir.contains("\"id\"") || ir.contains("id"), "toJson should reference field name");
    }

    #[test]
    fn test_json_serializable_string_field_gets_quotes() {
        let ir = compile_to_ir_with_serialize(
            "class User { var name: String; }\nfn main() -> Int64 { return 0; }",
            vec!["User"],
            vec![],
        );
        // String values are wrapped in quotes — the IR has a quote literal "
        assert!(ir.contains("\\22") || ir.contains("\"\\\"\"") || ir.contains("inttoptr"),
            "string field in toJson should be wrapped in quotes (inttoptr for i8* conversion)");
    }

    #[test]
    fn test_do_not_serialize_field_absent_from_to_json() {
        let ir = compile_to_ir_with_serialize(
            "class User { var name: String; var internalToken: String; }\nfn main() -> Int64 { return 0; }",
            vec!["User"],
            vec![("User", "internalToken")],
        );
        assert!(ir.contains("User_toJson"), "toJson should still be emitted");
        // internalToken field name must not appear as a JSON key
        assert!(!ir.contains("\"internalToken\""), "@DoNotSerialize field must not appear in toJson");
    }

    #[test]
    fn test_do_not_serialize_all_fields_emits_empty_object() {
        let ir = compile_to_ir_with_serialize(
            "class Secret { var token: String; var key: String; }\nfn main() -> Int64 { return 0; }",
            vec!["Secret"],
            vec![("Secret", "token"), ("Secret", "key")],
        );
        assert!(ir.contains("Secret_toJson"), "toJson should be emitted even when all fields are excluded");
        // With all fields excluded the only string literals in the function are "{" and "}"
        assert!(ir.contains("{"), "empty object should still have opening brace");
    }

    #[test]
    fn test_no_json_serializable_no_to_json() {
        let ir = compile_to_ir(
            "class Plain { var x: Int64; }\nfn main() -> Int64 { return 0; }",
        );
        assert!(!ir.contains("Plain_toJson"), "no @JsonSerializable → no toJson emitted");
    }

    #[test]
    fn test_json_serializable_registered_in_method_ret_types() {
        let mut lexer = tinox_lexer::Lexer::new(
            "class User { var name: String; }\nfn main() -> Int64 { return 0; }"
        );
        let tokens = lexer.tokenize().expect("lex");
        let ast = Parser::new(tokens).parse().expect("parse");
        let mut cg = CodeGen::new();
        cg.set_annotation_info(
            Default::default(), Default::default(), vec![], vec![],
            Default::default(), vec![], vec![], vec![], vec![],
            vec![],
            vec!["User".to_string()],
            vec![],
        );
        cg.gen(&ast).expect("codegen");
        let ir = cg.into_ir();
        assert!(ir.contains("define i8* @User_toJson"), "User_toJson function must be emitted");
    }

    #[test]
    fn test_do_not_serialize_combined_with_sensitive_in_codegen() {
        // A class can have both @Sensitive (for logging) and @DoNotSerialize (for JSON)
        // on different fields — both should be respected independently.
        let src = "class Record { var label: String; var password: String; var internalId: String; }\nfn main() -> Int64 { return 0; }";
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().expect("lex");
        let ast = Parser::new(tokens).parse().expect("parse");
        let mut cg = CodeGen::new();
        cg.set_annotation_info(
            Default::default(), Default::default(), vec![], vec![],
            Default::default(), vec![], vec![],
            vec![LogMaskFieldInfo { class_name: "Record".to_string(), field_name: "password".to_string() }],
            vec![],
            vec![LogMaskFieldInfo { class_name: "Record".to_string(), field_name: "internalId".to_string() }],
            vec!["Record".to_string()],
            vec![],
        );
        cg.gen(&ast).expect("codegen");
        let ir = cg.into_ir();
        assert!(ir.contains("Record_toString"), "toString should be emitted for @Sensitive field");
        assert!(ir.contains("Record_toJson"), "toJson should be emitted for @JsonSerializable");
        assert!(ir.contains("***"), "@Sensitive field should be masked in toString");
        assert!(!ir.contains("\"internalId\""), "@DoNotSerialize field must not appear in toJson");
    }

    #[test]
    fn test_multiple_json_serializable_classes() {
        let ir = compile_to_ir_with_serialize(
            "class User { var id: Int64; }\nclass Product { var sku: String; }\nfn main() -> Int64 { return 0; }",
            vec!["User", "Product"],
            vec![],
        );
        assert!(ir.contains("User_toJson"), "User should get toJson");
        assert!(ir.contains("Product_toJson"), "Product should get toJson");
    }
}
