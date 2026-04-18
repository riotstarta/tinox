use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::path::Path;
use tinox_common::{Error, ErrorBag, Span, Spanned};
use tinox_parser::{
    BinaryOp, CatchClause, DeclKind, Expr, ExprKind, Literal, Method, Pattern, SourceFile, Stmt,
    StmtKind, Type, UnaryOp,
};

pub struct CodeGen {
    ir: String,
    lambda_ir: String,
    strings: HashMap<String, String>,
    temp_count: usize,
    struct_layouts: HashMap<String, Vec<String>>,
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
        }
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
        writeln!(&mut self.ir, "declare void @tinox_print_newline()").unwrap();
        writeln!(&mut self.ir, "declare i8* @tinox_alloc(i64)").unwrap();
        writeln!(&mut self.ir, "declare void @tinox_panic(i64)").unwrap();
        writeln!(&mut self.ir).unwrap();

        // Build class AST map for inheritance helpers.
        let class_ast_map: HashMap<String, tinox_parser::Class> = source
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

        // First pass: build struct_layouts (with vtable slot at index 0 where needed)
        // and method_impl (for inherited method dispatch).
        for decl in &source.decls {
            if let DeclKind::Class(c) = &decl.node {
                if let Some(parent) = &c.extends {
                    self.class_parents.insert(c.name.clone(), parent.clone());
                }
                let has_vtable = !c.implements.is_empty()
                    || self.classes_with_vtable.contains(&c.name);
                let mut fields: Vec<String> = Vec::new();
                if has_vtable {
                    fields.push("__vtable__".to_string());
                    self.classes_with_vtable.insert(c.name.clone());
                    // Compute vtable size (union of all methods from all interfaces)
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
                // Use inherited field layout (parent fields first, own last).
                fields.extend(Self::collect_inherited_fields(&c.name, &class_ast_map));
                self.struct_layouts.insert(c.name.clone(), fields);

                // Build method_impl: resolve each accessible method to its implementing class.
                // Own methods map to themselves.
                for method in &c.methods {
                    let key = format!("{}_{}", c.name, method.name);
                    self.method_impl.insert(key.clone(), key);
                    self.method_ret_types.insert(
                        format!("{}_{}", c.name, method.name),
                        Self::type_to_llvm(&method.ret_type),
                    );
                }
                // Inherited methods (not overridden) map to the ancestor's implementation.
                let own_method_names: HashSet<String> =
                    c.methods.iter().map(|m| m.name.clone()).collect();
                let mut ancestor = c.extends.clone();
                while let Some(ref aname) = ancestor.clone() {
                    let Some(ac) = class_ast_map.get(aname) else {
                        break;
                    };
                    for method in &ac.methods {
                        if !own_method_names.contains(&method.name) {
                            let child_key = format!("{}_{}", c.name, method.name);
                            if !self.method_impl.contains_key(&child_key) {
                                let owner_key = Self::resolve_method_owner(
                                    aname,
                                    &method.name,
                                    &class_ast_map,
                                );
                                self.method_impl.insert(child_key.clone(), owner_key.clone());
                                let ret_ty = Self::type_to_llvm(&method.ret_type);
                                self.method_ret_types.insert(child_key, ret_ty);
                            }
                        }
                    }
                    ancestor = ac.extends.clone();
                }
            }
        }

        // Second pass: generate code
        for decl in &source.decls {
            match &decl.node {
                DeclKind::Function(f) => {
                    self.gen_fn(f)?;
                }
                DeclKind::Class(c) => {
                    for method in &c.methods {
                        self.gen_class_method(&c.name, method)?;
                    }
                }
                _ => {}
            }
        }

        // Emit vtable globals for classes that implement interfaces
        self.emit_vtable_globals(source);

        for (name, s) in &self.strings {
            writeln!(
                &mut self.ir,
                "@{} = private constant [{} x i8] c\"{}\\00\"",
                name,
                s.len() + 1,
                s
            )
            .unwrap();
        }

        Ok(())
    }

    pub fn into_ir(self) -> String {
        let mut result = self.ir;
        result.push_str(&self.lambda_ir);
        result
    }

    fn gen_fn(&mut self, f: &tinox_parser::Function) -> Result<(), ErrorBag> {
        let ret_type = Self::type_to_llvm(&f.ret_type);
        let mut params_str = String::new();
        let mut ctx = GenCtx {
            locals: HashMap::new(),
            params: HashSet::new(),
            struct_fields: Vec::new(),
            current_struct: None,
            local_types: HashMap::new(),
            break_target: None,
            continue_target: None,
            error_catch: None,
            defer_stack: Vec::new(),
            in_defer_exec: false,
        };

        for (i, p) in f.params.iter().enumerate() {
            if i > 0 {
                params_str.push_str(", ");
            }
            let llvm_ty = Self::type_to_llvm(&p.param_type);
            params_str.push_str(&format!("{} %{}", llvm_ty, p.name));
            ctx.locals.insert(p.name.clone(), (llvm_ty.clone(), i));
            ctx.params.insert(p.name.clone());

            // Track parameter types for struct/class types
            if let Type::Named(class_name) = &p.param_type {
                ctx.local_types.insert(p.name.clone(), class_name.clone());
            }
        }

        let fn_name = if f.name == "main" {
            "tinox_main"
        } else {
            &f.name
        };

        writeln!(
            &mut self.ir,
            "define {} @{}({}) {{",
            ret_type, fn_name, params_str
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
        let ret_type = Self::type_to_llvm(&method.ret_type);
        let fn_name = format!("{}_{}", class_name, method.name);
        self.method_ret_types.insert(fn_name.clone(), ret_type.clone());

        let mut ctx = GenCtx {
            locals: HashMap::new(),
            params: HashSet::new(),
            struct_fields: Vec::new(),
            current_struct: Some(class_name.to_string()),
            local_types: HashMap::new(),
            break_target: None,
            continue_target: None,
            error_catch: None,
            defer_stack: Vec::new(),
            in_defer_exec: false,
        };

        let mut params_str = "i64* %self".to_string();
        ctx.locals
            .insert("self".to_string(), ("i64*".to_string(), 0));
        ctx.params.insert("self".to_string());
        ctx.local_types
            .insert("self".to_string(), class_name.to_string());

        for p in &method.params {
            let llvm_ty = Self::type_to_llvm(&p.param_type);
            params_str.push_str(&format!(", {} %{}", llvm_ty, p.name));
            ctx.locals
                .insert(p.name.clone(), (llvm_ty.clone(), ctx.locals.len()));
            ctx.params.insert(p.name.clone());
            if let Type::Named(cn) = &p.param_type {
                ctx.local_types.insert(p.name.clone(), cn.clone());
            }
        }

        writeln!(
            &mut self.ir,
            "define {} @{}({}) {{",
            ret_type, fn_name, params_str
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
            } else {
                writeln!(&mut self.ir, "ret {} 0", ret_type).unwrap();
            }
        }

        writeln!(&mut self.ir, "}}").unwrap();
        writeln!(&mut self.ir).unwrap();

        Ok(())
    }

    /// Emit a vtable global for each class that implements at least one interface.
    fn emit_vtable_globals(&mut self, source: &SourceFile) {
        let class_names: Vec<(String, Vec<String>)> = source
            .decls
            .iter()
            .filter_map(|d| {
                if let DeclKind::Class(c) = &d.node {
                    if !c.implements.is_empty() {
                        Some((c.name.clone(), c.implements.clone()))
                    } else {
                        None
                    }
                } else {
                    None
                }
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
                let llvm_ty = Self::llvm_type_str(&ty);
                writeln!(&mut self.ir, "ret {} {}", llvm_ty, val).unwrap();
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
                    } else if matches!(&v.node, ExprKind::ArrayLiteral(_)) {
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

                if let Some(val) = value {
                    let (v, val_ty) = self.gen_expr(val, ctx)?;
                    let actual_ty = if matches!(&val.node, ExprKind::Lambda { .. }) {
                        val_ty.clone()
                    } else if is_heap_ptr {
                        llvm_ty.clone()
                    } else {
                        llvm_ty.clone()
                    };
                    let slot = ctx.locals.len();
                    ctx.locals.insert(name.clone(), (actual_ty.clone(), slot));
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
                    } else if matches!(&v.node, ExprKind::ArrayLiteral(_)) {
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
                let (cond_val, _) = self.gen_expr(cond, ctx)?;
                let then_bb = self.new_bb("then");
                let else_bb = self.new_bb("else");
                let merge_bb = self.new_bb("ifcont");
                writeln!(
                    &mut self.ir,
                    "br i1 {}, label %{}, label %{}",
                    cond_val, then_bb, else_bb
                )
                .unwrap();
                writeln!(&mut self.ir, "{}:", then_bb).unwrap();
                self.gen_stmt_body(then_branch, ctx)?;
                writeln!(&mut self.ir, "{}:", else_bb).unwrap();
                if let Some(else_stmt) = else_branch {
                    self.gen_stmt_body(else_stmt, ctx)?;
                }
                if !else_branch.is_some() {
                    writeln!(&mut self.ir, "br label %{}", merge_bb).unwrap();
                    writeln!(&mut self.ir, "{}:", merge_bb).unwrap();
                }
            }
            StmtKind::While { cond, body } => {
                let loop_bb = self.new_bb("loop");
                let body_bb = self.new_bb("loopbody");
                let end_bb = self.new_bb("loopend");

                let old_break = ctx.break_target.take();
                let old_continue = ctx.continue_target.take();
                ctx.break_target = Some(end_bb.clone());
                ctx.continue_target = Some(loop_bb.clone());

                let num_locals = ctx.locals.len();
                let mut local_snapshots: Vec<(String, String)> = Vec::new();
                for (name, (ty, _)) in ctx.locals.iter() {
                    if ty.contains('*') {
                        let temp = self.temp();
                        writeln!(&mut self.ir, "{} = load {}, {}* %{}", temp, ty, ty, name)
                            .unwrap();
                        local_snapshots.push((name.clone(), temp));
                    }
                }

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
                    let (cond_val, _) = self.gen_expr(cond_expr, ctx)?;
                    writeln!(
                        &mut self.ir,
                        "br i1 {}, label %{}, label %{}",
                        cond_val, body_bb, end_bb
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
                if let Some(break_bb) = ctx.break_target.take() {
                    writeln!(&mut self.ir, "br label %{}", break_bb).unwrap();
                }
            }
            StmtKind::Continue => {
                if let Some(continue_bb) = ctx.continue_target.take() {
                    writeln!(&mut self.ir, "br label %{}", continue_bb).unwrap();
                }
            }
            StmtKind::Throw(expr) => {
                let (val, _) = self.gen_expr(expr, ctx)?;
                if let Some((catch_bb, error_var)) = &ctx.error_catch {
                    writeln!(&mut self.ir, "store i64 {}, i64* {}", val, error_var).unwrap();
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
                let (range_ptr, _range_ty) = self.gen_expr(iter, ctx)?;

                let start_gep = self.temp();
                writeln!(
                    &mut self.ir,
                    "{} = getelementptr i64, i64* {}, i64 0",
                    start_gep, range_ptr
                )
                .unwrap();
                let start_val = self.temp();
                writeln!(&mut self.ir, "{} = load i64, i64* {}", start_val, start_gep).unwrap();

                let end_gep = self.temp();
                writeln!(
                    &mut self.ir,
                    "{} = getelementptr i64, i64* {}, i64 1",
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
                writeln!(
                    &mut self.ir,
                    "{} = icmp slt i64 {}, {}",
                    cmp, cur_val, end_val
                )
                .unwrap();
                writeln!(
                    &mut self.ir,
                    "br i1 {}, label %{}, label %{}",
                    cmp, body_bb, end_bb
                )
                .unwrap();

                writeln!(&mut self.ir, "{}:", body_bb).unwrap();
                self.gen_stmt_body(body, ctx)?;

                let loaded_inc = self.temp();
                writeln!(&mut self.ir, "{} = load i64, i64* %{}", loaded_inc, var).unwrap();
                let next_val = self.temp();
                writeln!(&mut self.ir, "{} = add i64 {}, 1", next_val, loaded_inc).unwrap();
                writeln!(&mut self.ir, "store i64 {}, i64* %{}", next_val, var).unwrap();
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
            StmtKind::Assignment { target, value } => {
                if let ExprKind::Ident(name) = &target.node {
                    let name = name.clone();
                    if let Some((ty, _)) = ctx.locals.get(&name) {
                        let ty = ty.clone();
                        let (val, _) = self.gen_expr(value, ctx)?;
                        writeln!(&mut self.ir, "store {} {}, {}* %{}", ty, val, ty, name).unwrap();
                    }
                } else if let ExprKind::FieldAccess { obj, field } = &target.node {
                    let (obj_ptr, _) = self.gen_expr(obj, ctx)?;
                    let struct_name = match &obj.node {
                        ExprKind::Ident(name) => ctx.local_types.get(name).cloned(),
                        ExprKind::This => ctx.current_struct.clone(),
                        _ => None,
                    };
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
                    let field_ptr = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = getelementptr i64, i64* {}, i64 {}",
                        field_ptr, obj_ptr, offset
                    )
                    .unwrap();
                    writeln!(&mut self.ir, "store {} {}, i64* {}", val_ty, val, field_ptr)
                        .unwrap();
                } else if let ExprKind::Index { obj, index } = &target.node {
                    let (idx_val, _) = self.gen_expr(index, ctx)?;
                    let (base_ptr, _) = if let ExprKind::Ident(name) = &obj.node {
                        if ctx.locals.contains_key(name) {
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
                    let ptr_name = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = getelementptr i64, i64* {}, i64 {}",
                        ptr_name, base_ptr, idx_val
                    )
                    .unwrap();
                    writeln!(&mut self.ir, "store i64 {}, i64* {}", val, ptr_name).unwrap();
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
                    let val = self.temp();
                    writeln!(&mut self.ir, "{} = load {}, {}* %{}", val, ty, ty, name).unwrap();
                    Ok((val, ty.clone()))
                } else {
                    Ok((format!("%{}", name), "i64".to_string()))
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let (l, lt) = self.gen_expr(lhs, ctx)?;
                let (r, rt) = self.gen_expr(rhs, ctx)?;
                let result = self.temp();
                let float = Self::is_float(&lt);
                match op {
                    tinox_parser::BinaryOp::Add => {
                        if float {
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
                    }
                    tinox_parser::BinaryOp::Ne => {
                        if float {
                            writeln!(&mut self.ir, "{} = fcmp one {} {}, {}", result, lt, l, r).unwrap()
                        } else {
                            writeln!(&mut self.ir, "{} = icmp ne {} {}, {}", result, lt, l, r).unwrap()
                        }
                    }
                    tinox_parser::BinaryOp::Lt => {
                        if float {
                            writeln!(&mut self.ir, "{} = fcmp olt {} {}, {}", result, lt, l, r).unwrap()
                        } else {
                            writeln!(&mut self.ir, "{} = icmp slt {} {}, {}", result, lt, l, r).unwrap()
                        }
                    }
                    tinox_parser::BinaryOp::Le => {
                        if float {
                            writeln!(&mut self.ir, "{} = fcmp ole {} {}, {}", result, lt, l, r).unwrap()
                        } else {
                            writeln!(&mut self.ir, "{} = icmp sle {} {}, {}", result, lt, l, r).unwrap()
                        }
                    }
                    tinox_parser::BinaryOp::Gt => {
                        if float {
                            writeln!(&mut self.ir, "{} = fcmp ogt {} {}, {}", result, lt, l, r).unwrap()
                        } else {
                            writeln!(&mut self.ir, "{} = icmp sgt {} {}, {}", result, lt, l, r).unwrap()
                        }
                    }
                    tinox_parser::BinaryOp::Ge => {
                        if float {
                            writeln!(&mut self.ir, "{} = fcmp oge {} {}, {}", result, lt, l, r).unwrap()
                        } else {
                            writeln!(&mut self.ir, "{} = icmp sge {} {}, {}", result, lt, l, r).unwrap()
                        }
                    }
                    tinox_parser::BinaryOp::And => {
                        writeln!(&mut self.ir, "{} = and {} {}, {}", result, lt, l, r).unwrap()
                    }
                    tinox_parser::BinaryOp::Or => {
                        writeln!(&mut self.ir, "{} = or {} {}, {}", result, lt, l, r).unwrap()
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
                                let arg = &args[0];
                                if let ExprKind::Literal(Literal::String(s)) = &arg.node {
                                    let str_name = format!("str{}", self.strings.len());
                                    self.strings.insert(str_name.clone(), s.clone());
                                    let len = s.len() + 1;
                                    let ptr_name = self.temp();
                                    writeln!(&mut self.ir, "{} = getelementptr [{} x i8], [{} x i8]* @{}, i64 0, i64 0", ptr_name, len, len, str_name).unwrap();
                                    writeln!(
                                        &mut self.ir,
                                        "call void @tinox_print_string(i8* {})",
                                        ptr_name
                                    )
                                    .unwrap();
                                } else {
                                    let ty = &arg_types[0];
                                    let llvm_fn = match ty.as_str() {
                                        "i64" => "tinox_print_int",
                                        "double" => "tinox_print_float",
                                        _ => "tinox_print_int",
                                    };
                                    writeln!(&mut self.ir, "call void @{}({})", llvm_fn, args_str)
                                        .unwrap();
                                }
                                if name == "println" {
                                    writeln!(&mut self.ir, "call void @tinox_print_newline()")
                                        .unwrap();
                                }
                            }
                            return Ok(("0".to_string(), "void".to_string()));
                        }
                        _ => name.clone(),
                    },
                    _ => "unknown_fn".to_string(),
                };
                let ret_ty = arg_types
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "i64".to_string());
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
                            "{} = getelementptr i64, i64* {}, i64 1",
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
                let (obj_ptr, obj_ty) = self.gen_expr(obj, ctx)?;

                let declared_type = match &obj.node {
                    ExprKind::Ident(name) => ctx.local_types.get(name).cloned(),
                    ExprKind::This => ctx.current_struct.clone(),
                    _ => None,
                };

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
                        "{} = getelementptr i64, i64* {}, i64 0",
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
                        "{} = getelementptr i64, i64* {}, i64 {}",
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
                    writeln!(
                        &mut self.ir,
                        "{} = call {} @{}({})",
                        result, ret_ty, full_method_name, full_args_str
                    )
                    .unwrap();
                    Ok((result, ret_ty))
                }
            }
            ExprKind::Index { obj, index } => {
                let (idx_val, _) = self.gen_expr(index, ctx)?;
                let (base_ptr, _) = if let ExprKind::Ident(name) = &obj.node {
                    if ctx.locals.contains_key(name) {
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
                let ptr_name = self.temp();
                writeln!(
                    &mut self.ir,
                    "{} = getelementptr i64, i64* {}, i64 {}",
                    ptr_name, base_ptr, idx_val
                )
                .unwrap();
                let val = self.temp();
                writeln!(&mut self.ir, "{} = load i64, i64* {}", val, ptr_name).unwrap();
                Ok((val, "i64".to_string()))
            }
            ExprKind::ArrayLiteral(elements) => {
                let ptr = self.temp();
                let size = elements.len() * 8;
                writeln!(
                    &mut self.ir,
                    "{} = call i8* @tinox_alloc(i64 {})",
                    ptr, size
                )
                .unwrap();
                let typed_ptr = self.temp();
                writeln!(&mut self.ir, "{} = bitcast i8* {} to i64*", typed_ptr, ptr).unwrap();
                for (i, elem) in elements.iter().enumerate() {
                    let (val, _) = self.gen_expr(elem, ctx)?;
                    let elem_ptr = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = getelementptr i64, i64* {}, i64 {}",
                        elem_ptr, typed_ptr, i
                    )
                    .unwrap();
                    writeln!(&mut self.ir, "store i64 {}, i64* {}", val, elem_ptr).unwrap();
                }
                Ok((typed_ptr, "i64*".to_string()))
            }
            ExprKind::FieldAccess { obj, field } => {
                let (obj_ptr, _) = self.gen_expr(obj, ctx)?;

                // Find the struct type and field offset
                let struct_name = if let ExprKind::Ident(name) = &obj.node {
                    ctx.local_types.get(name).cloned()
                } else {
                    None
                };

                let offset = if let Some(sname) = struct_name {
                    if let Some(fields) = self.struct_layouts.get(&sname) {
                        fields.iter().position(|f| f == field).unwrap_or(0) as i64
                    } else {
                        0
                    }
                } else {
                    0
                };

                let field_ptr = self.temp();
                writeln!(
                    &mut self.ir,
                    "{} = getelementptr i64, i64* {}, i64 {}",
                    field_ptr, obj_ptr, offset
                )
                .unwrap();

                // Load the value from the field
                let result = self.temp();
                writeln!(&mut self.ir, "{} = load i64, i64* {}", result, field_ptr).unwrap();
                Ok((result, "i64".to_string()))
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
                        "{} = getelementptr i64, i64* {}, i64 0",
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
                    let (val, _) = self.gen_expr(value, ctx)?;
                    // Look up field position in layout (which includes __vtable__ at 0 if vtable class)
                    let field_idx = layout.iter().position(|f| f == fname).unwrap_or(0);
                    let field_ptr = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = getelementptr i64, i64* {}, i64 {}",
                        field_ptr, typed_ptr, field_idx
                    )
                    .unwrap();
                    writeln!(&mut self.ir, "store i64 {}, i64* {}", val, field_ptr).unwrap();
                }
                Ok((typed_ptr, "i64*".to_string()))
            }
            ExprKind::Tuple(exprs) => {
                let ptr = self.temp();
                let size = exprs.len() * 8;
                writeln!(
                    &mut self.ir,
                    "{} = call i8* @tinox_alloc(i64 {})",
                    ptr, size
                )
                .unwrap();
                let typed_ptr = self.temp();
                writeln!(&mut self.ir, "{} = bitcast i8* {} to i64*", typed_ptr, ptr).unwrap();
                for (i, expr) in exprs.iter().enumerate() {
                    let (val, _) = self.gen_expr(expr, ctx)?;
                    let field_ptr = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = getelementptr i64, i64* {}, i64 {}",
                        field_ptr, typed_ptr, i
                    )
                    .unwrap();
                    writeln!(&mut self.ir, "store i64 {}, i64* {}", val, field_ptr).unwrap();
                }
                Ok((typed_ptr, "i64*".to_string()))
            }
            ExprKind::EnumValue {
                enum_name,
                variant,
                args,
            } => {
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
                        "{} = getelementptr i64, i64* {}, i64 0",
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
                            "{} = getelementptr i64, i64* {}, i64 {}",
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
            ExprKind::Cast { expr, ty } => {
                let (val, val_ty) = self.gen_expr(expr, ctx)?;
                let llvm_ty = Self::type_to_llvm(ty);
                if llvm_ty == val_ty {
                    Ok((val, llvm_ty))
                } else {
                    let result = self.temp();
                    if val_ty == "i1" {
                        writeln!(&mut self.ir, "{} = zext i1 {} to {}", result, val, llvm_ty)
                            .unwrap();
                    } else if val_ty.starts_with('i') && llvm_ty.starts_with('i') {
                        let val_bits: u32 = val_ty[1..].parse().unwrap_or(64);
                        let tgt_bits: u32 = llvm_ty[1..].parse().unwrap_or(64);
                        if val_bits < tgt_bits {
                            writeln!(
                                &mut self.ir,
                                "{} = sext {} {} to {}",
                                result, val_ty, val, llvm_ty
                            )
                            .unwrap();
                        } else {
                            writeln!(
                                &mut self.ir,
                                "{} = trunc {} {} to {}",
                                result, val_ty, val, llvm_ty
                            )
                            .unwrap();
                        }
                    } else {
                        writeln!(
                            &mut self.ir,
                            "{} = bitcast {} {} to {}",
                            result, val_ty, val, llvm_ty
                        )
                        .unwrap();
                    }
                    Ok((result, llvm_ty))
                }
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
            ExprKind::New { class, args } => {
                let layout_clone = self.struct_layouts.get(class).cloned();
                let has_vtable = self.classes_with_vtable.contains(class);
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
                    let n_vtable = self.vtable_sizes.get(class).copied().unwrap_or(1);
                    let vtable_gep = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = getelementptr i64, i64* {}, i64 0",
                        vtable_gep, typed_ptr
                    )
                    .unwrap();
                    let vtable_as_i64 = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = ptrtoint [{} x i64]* @{}_vtable to i64",
                        vtable_as_i64, n_vtable, class
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
                            let (val, _) = self.gen_expr(arg, ctx)?;
                            let field_ptr = self.temp();
                            writeln!(
                                &mut self.ir,
                                "{} = getelementptr i64, i64* {}, i64 {}",
                                field_ptr, typed_ptr, layout_idx
                            )
                            .unwrap();
                            writeln!(&mut self.ir, "store i64 {}, i64* {}", val, field_ptr)
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
                    "{} = getelementptr i64, i64* {}, i64 0",
                    start_ptr, typed_ptr
                )
                .unwrap();
                writeln!(&mut self.ir, "store i64 {}, i64* {}", start_val, start_ptr).unwrap();
                let end_ptr = self.temp();
                writeln!(
                    &mut self.ir,
                    "{} = getelementptr i64, i64* {}, i64 1",
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
                                    "{} = getelementptr i64, i64* {}, i64 0",
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
                                            "{} = getelementptr i64, i64* {}, i64 {}",
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
                    "{} = getelementptr i64, i64* {}, i64 0",
                    start_gep, range_ptr
                )
                .unwrap();
                let start_val = self.temp();
                writeln!(&mut self.ir, "{} = load i64, i64* {}", start_val, start_gep).unwrap();

                let end_gep = self.temp();
                writeln!(
                    &mut self.ir,
                    "{} = getelementptr i64, i64* {}, i64 1",
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
                let (base_ptr, var_ty) = if let ExprKind::Ident(name) = &obj.node {
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
                    "{} = getelementptr i64, i64* {}, i64 {}",
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
                Ok((name, "i8*".to_string()))
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
            params: HashSet::new(),
            struct_fields: Vec::new(),
            current_struct: None,
            local_types: HashMap::new(),
            break_target: None,
            continue_target: None,
            error_catch: None,
            defer_stack: Vec::new(),
            in_defer_exec: false,
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
                if let Some((_, slot)) = ctx.locals.get(name) {
                    let field_ptr = self.temp();
                    writeln!(
                        &mut self.ir,
                        "{} = getelementptr i64, i64* {}, i64 {}",
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
                    "{} = getelementptr i64, i64* {}, i64 {}",
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
                "{} = getelementptr i64, i64* {}, i64 0",
                fp_field, closure_ptr_int
            )
            .unwrap();
            writeln!(&mut self.ir, "store i64 {}, i64* {}", ptr_name, fp_field).unwrap();
            let env_field = self.temp();
            let env_ptr_clean = env_ptr.trim_start_matches('%');
            writeln!(
                &mut self.ir,
                "{} = getelementptr i64, i64* {}, i64 1",
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
            Type::Unit => "void".to_string(),
            Type::Named(_) => "i64*".to_string(), // Classes/structs are pointers
            Type::Ref(inner) => format!("{}*", Self::type_to_llvm(inner)),
            Type::Mutable(inner) => Self::type_to_llvm(inner),
            Type::Array(inner) => format!("{}*", Self::type_to_llvm(inner)),
            _ => "i64".to_string(),
        }
    }

    fn temp(&mut self) -> String {
        let t = format!("%t{}", self.temp_count);
        self.temp_count += 1;
        t
    }

    fn new_bb(&mut self, name: &str) -> String {
        format!("{}_{}", name, self.temp_count)
    }

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

    fn get_struct_name_for_type(&self, _ty: &str) -> String {
        // For now, just return the type as-is
        // In a full implementation, we'd look up the actual struct name
        _ty.replace("*", "")
    }

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
        // unreachable block swallows any double-terminator after a throw
        let try_ok_bb = self.new_bb("try_ok");
        writeln!(&mut self.ir, "{}:", try_ok_bb).unwrap();
        writeln!(&mut self.ir, "br label %{}", merge_target).unwrap();

        // --- catch block ---
        writeln!(&mut self.ir, "{}:", catch_bb).unwrap();
        if !catches.is_empty() {
            let catch = &catches[0];
            let param_slot = ctx.locals.len();
            ctx.locals
                .insert(catch.param.clone(), ("i64".to_string(), param_slot));
            writeln!(&mut self.ir, "%{} = alloca i64", catch.param).unwrap();
            let err_val = self.temp();
            writeln!(&mut self.ir, "{} = load i64, i64* {}", err_val, error_var).unwrap();
            writeln!(&mut self.ir, "store i64 {}, i64* %{}", err_val, catch.param).unwrap();
            self.gen_stmt_body(&catch.body, ctx)?;
        }
        // unreachable block after catch body
        let catch_ok_bb = self.new_bb("catch_ok");
        writeln!(&mut self.ir, "{}:", catch_ok_bb).unwrap();
        writeln!(&mut self.ir, "br label %{}", merge_target).unwrap();

        // --- finally block ---
        if let Some(fb) = &finally_bb {
            writeln!(&mut self.ir, "{}:", fb).unwrap();
            if let Some(finally_stmt) = finally {
                self.gen_stmt_body(finally_stmt, ctx)?;
            }
            let finally_ok_bb = self.new_bb("finally_ok");
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

    pub fn write_asm(&self, ir_path: &Path, asm_path: &Path) -> Result<(), Error> {
        let output = std::process::Command::new("llc")
            .args(["-O3", "-march=x86-64", "-filetype=asm", "-o"])
            .arg(asm_path)
            .arg(ir_path)
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
        let output = std::process::Command::new("llc")
            .args(["-O3", "-march=x86-64", "-filetype=obj", "-o"])
            .arg(obj_path)
            .arg(ir_path)
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
}

fn expr_kind_name(kind: &ExprKind) -> &'static str {
    match kind {
        ExprKind::Literal(_) => "Literal",
        ExprKind::ArrayLiteral(_) => "ArrayLiteral",
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
        ExprKind::Cast { expr, ty: _ } => {
            collect_free_vars_inner(expr, param_names, vars);
        }
        ExprKind::Block(stmts) => {
            for stmt in stmts {
                if let StmtKind::Expr(e) = &stmt.node {
                    collect_free_vars_inner(e, param_names, vars);
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
    params: HashSet<String>,
    struct_fields: Vec<String>,
    current_struct: Option<String>,
    local_types: HashMap<String, String>,
    break_target: Option<String>,
    continue_target: Option<String>,
    error_catch: Option<(String, String)>,
    defer_stack: Vec<Vec<Stmt>>,
    in_defer_exec: bool,
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
        let src = "fn add_floats(a: Float64, b: Float64) -> Float64 {\n  return a + b;\n}";
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
    fn test_loop_stmt() {
        let src = "fn main() -> Int64 {\n  loop { break; };\n  return 0;\n}";
        let ir = compile_to_ir(src);
        assert!(ir.contains("loop_body"), "should have loop body block");
        assert!(ir.contains("loop_end"), "should have loop end block");
    }
}
