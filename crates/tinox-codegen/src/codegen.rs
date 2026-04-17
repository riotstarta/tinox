use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::path::Path;
use tinox_common::{Error, ErrorBag, Span, Spanned};
use tinox_parser::{
    BinaryOp, CatchClause, DeclKind, Expr, ExprKind, Literal, Pattern, SourceFile, Stmt, StmtKind,
    Type, UnaryOp,
};

pub struct CodeGen {
    ir: String,
    lambda_ir: String,
    strings: HashMap<String, String>,
    temp_count: usize,
    struct_layouts: HashMap<String, Vec<String>>,
    closure_envs: HashMap<String, String>,
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
        }
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

        for decl in &source.decls {
            match &decl.node {
                DeclKind::Function(f) => {
                    self.gen_fn(f)?;
                }
                DeclKind::Class(c) => {
                    let fields: Vec<String> = c.fields.iter().map(|f| f.name.clone()).collect();
                    self.struct_layouts.insert(c.name.clone(), fields);
                }
                _ => {}
            }
        }

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

        writeln!(&mut self.ir, "}}").unwrap();
        writeln!(&mut self.ir).unwrap();

        Ok(())
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
                    if let Some(sn) = &struct_name {
                        ctx.local_types.insert(name.clone(), sn.clone());
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
                    if let Some(sn) = &struct_name {
                        ctx.local_types.insert(name.clone(), sn.clone());
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
            StmtKind::Assignment { target, value } => {
                if let ExprKind::Ident(name) = &target.node {
                    let name = name.clone();
                    if let Some((ty, _)) = ctx.locals.get(&name) {
                        let ty = ty.clone();
                        let (val, _) = self.gen_expr(value, ctx)?;
                        writeln!(&mut self.ir, "store {} {}, {}* %{}", ty, val, ty, name).unwrap();
                    }
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
                    Ok((format!("%{}", name), "i64".to_string()))
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
                match op {
                    tinox_parser::BinaryOp::Add => {
                        writeln!(&mut self.ir, "{} = add {} {}, {}", result, lt, l, r).unwrap()
                    }
                    tinox_parser::BinaryOp::Sub => {
                        writeln!(&mut self.ir, "{} = sub {} {}, {}", result, lt, l, r).unwrap()
                    }
                    tinox_parser::BinaryOp::Mul => {
                        writeln!(&mut self.ir, "{} = mul {} {}, {}", result, lt, l, r).unwrap()
                    }
                    tinox_parser::BinaryOp::Div => {
                        writeln!(&mut self.ir, "{} = sdiv {} {}, {}", result, lt, l, r).unwrap()
                    }
                    tinox_parser::BinaryOp::Eq => {
                        writeln!(&mut self.ir, "{} = icmp eq {} {}, {}", result, lt, l, r).unwrap()
                    }
                    tinox_parser::BinaryOp::Ne => {
                        writeln!(&mut self.ir, "{} = icmp ne {} {}, {}", result, lt, l, r).unwrap()
                    }
                    tinox_parser::BinaryOp::Lt => {
                        writeln!(&mut self.ir, "{} = icmp slt {} {}, {}", result, lt, l, r).unwrap()
                    }
                    tinox_parser::BinaryOp::Le => {
                        writeln!(&mut self.ir, "{} = icmp sle {} {}, {}", result, lt, l, r).unwrap()
                    }
                    tinox_parser::BinaryOp::Gt => {
                        writeln!(&mut self.ir, "{} = icmp sgt {} {}, {}", result, lt, l, r).unwrap()
                    }
                    tinox_parser::BinaryOp::Ge => {
                        writeln!(&mut self.ir, "{} = icmp sge {} {}, {}", result, lt, l, r).unwrap()
                    }
                    tinox_parser::BinaryOp::Mod => {
                        writeln!(&mut self.ir, "{} = srem {} {}, {}", result, lt, l, r).unwrap()
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
                        writeln!(&mut self.ir, "{} = sub {} 0, {}", result, ty, val).unwrap()
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
                // Get object and determine its class type
                let (obj_ptr, obj_ty) = self.gen_expr(obj, ctx)?;

                // Determine the class name from the object expression
                let class_name = if let ExprKind::Ident(name) = &obj.node {
                    ctx.local_types.get(name).cloned()
                } else {
                    // For other expressions (e.g., struct literals), we can't easily determine type
                    // This is a limitation we'll address with proper type tracking
                    None
                };

                // Construct the full method name: ClassName_methodName
                let full_method_name = if let Some(class) = class_name {
                    format!("{}_{}", class, method)
                } else {
                    // Fallback: just use the method name (for now)
                    method.clone()
                };

                // Build argument list with object as first argument
                let mut full_args_str = format!("{} {}", obj_ty, obj_ptr);
                for arg in args {
                    let (val, ty) = self.gen_expr(arg, ctx)?;
                    full_args_str.push_str(&format!(", {} {}", ty, val));
                }

                // Call the method function
                let result = self.temp();
                writeln!(
                    &mut self.ir,
                    "{} = call i64 @{}({})",
                    result, full_method_name, full_args_str
                )
                .unwrap();
                Ok((result, "i64".to_string()))
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
                for (i, (fname, value)) in fields.iter().enumerate() {
                    let (val, _) = self.gen_expr(value, ctx)?;
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
                for stmt in stmts {
                    self.gen_stmt_body(stmt, ctx)?;
                }
                Ok(("0".to_string(), "i64".to_string()))
            }
            ExprKind::New { class, args } => {
                let layout_clone = self.struct_layouts.get(class).cloned();
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
                if let Some(ref layout) = layout_clone {
                    for (i, _) in layout.iter().enumerate() {
                        if i < args.len() {
                            let (val, _) = self.gen_expr(&args[i], ctx)?;
                            let field_ptr = self.temp();
                            writeln!(
                                &mut self.ir,
                                "{} = getelementptr i64, i64* {}, i64 {}",
                                field_ptr, typed_ptr, i
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
                                "%{} = icmp eq {} {}, {}",
                                cmp, val_ty, val, lit_val
                            )
                            .unwrap();
                            let case_bb = self.new_bb("match_case");
                            let next_bb = self.new_bb("match_next");
                            writeln!(
                                &mut self.ir,
                                "br i1 %{}, label %{}, label %{}",
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
                        _ => {}
                    }
                }
                writeln!(&mut self.ir, "{}:", merge_bb).unwrap();
                Ok(last_result)
            }
            ExprKind::This | ExprKind::Super => Ok(("0".to_string(), "i64".to_string())),
            ExprKind::Is { expr, .. } => {
                let (val, _val_ty) = self.gen_expr(expr, ctx)?;
                let result = self.temp();
                writeln!(&mut self.ir, "{} = icmp ne i64 {}, 0", result, val).unwrap();
                Ok((result, "i1".to_string()))
            }
            _ => Ok(("0".to_string(), "i64".to_string())),
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
                    _ => {
                        let r = self.temp();
                        writeln!(&mut self.ir, "{} = or i64 {}, {}", r, 0, rhs).unwrap();
                        writeln!(&mut self.ir, "{} = add i64 {}, {}", result, loaded, r).unwrap();
                    }
                }
                writeln!(&mut self.ir, "store i64 {}, i64* {}", result, ptr_name).unwrap();
                return Ok((result, "i64".to_string()));
            }
            _ => {}
        }
        Ok(("0".to_string(), "i64".to_string()))
    }

    fn gen_literal(&mut self, lit: &Literal) -> Result<(String, String), ErrorBag> {
        match lit {
            Literal::Integer(n) => Ok((format!("{}", n), "i64".to_string())),
            Literal::Float(_f) => {
                let const_name = format!("f{}", self.temp_count);
                self.temp_count += 1;
                writeln!(
                    &mut self.ir,
                    "{} = fmul double {}, {}",
                    const_name, const_name, const_name
                )
                .unwrap();
                Ok((const_name, "double".to_string()))
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
        format!(".{}_{}", name, self.temp_count)
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

        writeln!(&mut self.ir, "{} = alloca i64", error_var).unwrap();
        writeln!(&mut self.ir, "store i64 0, i64* {}", error_var).unwrap();

        writeln!(&mut self.ir, "br label %{}", try_bb).unwrap();
        writeln!(&mut self.ir, "{}:", try_bb).unwrap();

        let old_error_catch = ctx.error_catch.take();
        ctx.error_catch = Some((catch_bb.clone(), error_var.clone()));

        self.gen_stmt_body(body, ctx)?;

        ctx.error_catch = old_error_catch;

        if let Some(fb) = &finally_bb {
            writeln!(&mut self.ir, "br label %{}", fb).unwrap();
        } else {
            writeln!(&mut self.ir, "br label %{}", end_bb).unwrap();
        }

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

        if let Some(fb) = &finally_bb {
            if let Some(finally_stmt) = finally {
                writeln!(&mut self.ir, "br label %{}", fb).unwrap();
                writeln!(&mut self.ir, "{}:", fb).unwrap();
                self.gen_stmt_body(finally_stmt, ctx)?;
            }
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
        ExprKind::This | ExprKind::Super | ExprKind::New { .. } | ExprKind::Is { .. } => {}
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
