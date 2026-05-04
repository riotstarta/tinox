use crate::ast::*;

pub struct Formatter {
    indent: usize,
}

impl Formatter {
    pub fn new() -> Self {
        Self { indent: 0 }
    }

    pub fn format(&mut self, source: &SourceFile) -> String {
        let mut out = String::new();
        for (i, decl) in source.decls.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&self.fmt_decl(&decl.node));
            out.push('\n');
        }
        out
    }

    fn ind(&self) -> String {
        "    ".repeat(self.indent)
    }

    fn fmt_doc(&self, doc: &Option<String>) -> String {
        match doc {
            Some(d) => {
                let lines: Vec<&str> = d.lines().collect();
                if lines.len() <= 1 {
                    let content = d.trim();
                    let content = content.strip_suffix("*/").unwrap_or(content).trim();
                    format!("{}/** {} */\n", self.ind(), content)
                } else {
                    let mut out = String::new();
                    out.push_str(&format!("{}/**\n", self.ind()));
                    for line in &lines {
                        let trimmed = line.trim();
                        // Skip the closing */
                        if trimmed == "*/" || trimmed.is_empty() {
                            continue;
                        }
                        let mut cleaned = trimmed.trim_start_matches('*');
                        cleaned = cleaned.strip_suffix("*/").unwrap_or(cleaned);
                        let cleaned = cleaned.trim();
                        if !cleaned.is_empty() {
                            out.push_str(&format!("{} * {}\n", self.ind(), cleaned));
                        }
                    }
                    out.push_str(&format!("{} */\n", self.ind()));
                    out
                }
            }
            None => String::new(),
        }
    }

    fn fmt_decl(&mut self, decl: &DeclKind) -> String {
        match decl {
            DeclKind::Function(f) => self.fmt_fn(f),
            DeclKind::Class(c) => self.fmt_class(c),
            DeclKind::Interface(i) => self.fmt_interface(i),
            DeclKind::Enum(e) => self.fmt_enum(e),
            DeclKind::Trait(t) => self.fmt_trait(t),
            DeclKind::Import(i) => self.fmt_import(i),
            DeclKind::Module(name) => format!("module {};", name),
            DeclKind::Namespace(ns) => self.fmt_namespace(ns),
            DeclKind::Immutable(u) => {
                let fields: Vec<String> = u.fields.iter()
                    .map(|f| format!("{}: {}", f.name, self.fmt_type(&f.param_type)))
                    .collect();
                format!("{}immutable {}({})", self.fmt_doc(&u.doc), u.name, fields.join(", "))
            }
        }
    }

fn fmt_annotations(&self, annotations: &[Annotation]) -> String {
        let mut out = String::new();
        for ann in annotations {
            if ann.args.is_empty() {
                out.push_str(&format!("{}@{}\n", self.ind(), ann.name));
            } else {
                let args: Vec<String> = ann.args.iter().map(fmt_literal).collect();
                out.push_str(&format!("{}@{}({})\n", self.ind(), ann.name, args.join(", ")));
            }
        }
        out
    }

    fn fmt_fn(&mut self, f: &Function) -> String {
        let ann = self.fmt_annotations(&f.annotations);
        let doc = self.fmt_doc(&f.doc);
        let async_ = if f.is_async { "async " } else { "" };
        let type_params = fmt_type_params(&f.type_params);
        let params = self.fmt_params(&f.params);
        let ret = self.fmt_type(&f.ret_type);
        let sig = format!("{}fn {}{}({}) -> {}", async_, f.name, type_params, params, ret);
        format!("{}{}{}\n{}", ann, doc, sig, self.fmt_block_stmt(&f.body))
    }

    fn fmt_method(&mut self, m: &Method) -> String {
        let ann = self.fmt_annotations(&m.annotations);
        let doc = self.fmt_doc(&m.doc);
        let async_ = if m.is_async { "async " } else { "" };
        let vis = fmt_visibility(&m.visibility);
        let fn_kw = if m.static_ { "fnc" } else { "fn" };
        let params = self.fmt_params(&m.params);
        let ret = self.fmt_type(&m.ret_type);
        let sig = format!("{}{}{}{} {}({}) -> {}", self.ind(), vis, async_, fn_kw, m.name, params, ret);
        format!("{}{}{}\n{}", ann, doc, sig, self.fmt_block_stmt(&m.body))
    }

    fn fmt_namespace(&mut self, ns: &Namespace) -> String {
        let mut body = String::new();
        self.indent += 1;
        for (i, decl) in ns.decls.iter().enumerate() {
            if i > 0 {
                body.push('\n');
            }
            body.push_str(&format!("{}{}\n", self.ind(), self.fmt_decl(&decl.node)));
        }
        self.indent -= 1;
        format!("namespace {}\n{}{{\n{}{}}}", ns.name.join("."), self.ind(), body, self.ind())
    }

    fn fmt_class(&mut self, c: &Class) -> String {
        let ann = self.fmt_annotations(&c.annotations);
        let doc = self.fmt_doc(&c.doc);
        let type_params = fmt_type_params(&c.type_params);
        let mut header = format!("class {}{}", c.name, type_params);
        if let Some(ext) = &c.extends {
            header.push_str(&format!(" extends {}", ext));
        }
        if !c.implements.is_empty() {
            header.push_str(&format!(" implements {}", c.implements.join(", ")));
        }
        let mut body = String::new();
        self.indent += 1;
        for field in &c.fields {
            let field_ann = self.fmt_annotations(&field.annotations);
            let field_doc = self.fmt_doc(&field.doc);
            body.push_str(&format!("{}{}{}{}{}{}{};\n",
                field_ann,
                field_doc,
                self.ind(),
                fmt_visibility(&field.visibility),
                if field.mutable { "var " } else { "" },
                field.name,
                format!(": {}", self.fmt_type(&field.field_type))
            ));
        }
        if !c.fields.is_empty() && !c.methods.is_empty() {
            body.push('\n');
        }
        for (i, method) in c.methods.iter().enumerate() {
            if i > 0 {
                body.push('\n');
            }
            body.push_str(&self.fmt_method(method));
            body.push('\n');
        }
        self.indent -= 1;
        format!("{}{}{}{}\n{}{{\n{}{}}}", ann, doc, self.ind(), header, self.ind(), body, self.ind())
    }

    fn fmt_interface(&mut self, i: &Interface) -> String {
        let ann = self.fmt_annotations(&i.annotations);
        let doc = self.fmt_doc(&i.doc);
        let mut header = format!("interface {}", i.name);
        if !i.extends.is_empty() {
            header.push_str(&format!(" extends {}", i.extends.join(", ")));
        }
        let mut body = String::new();
        self.indent += 1;
        for f in &i.methods {
            let method_doc = self.fmt_doc(&f.doc);
            let type_params = fmt_type_params(&f.type_params);
            let params = self.fmt_params(&f.params);
            let ret = self.fmt_type(&f.ret_type);
            body.push_str(&format!("{}{}fn {}{}({}) -> {};\n",
                self.ind(), method_doc, f.name, type_params, params, ret));
        }
        self.indent -= 1;
        format!("{}{}{}{}\n{}{{\n{}{}}}", ann, doc, self.ind(), header, self.ind(), body, self.ind())
    }

    fn fmt_enum(&mut self, e: &Enum) -> String {
        let ann = self.fmt_annotations(&e.annotations);
        let doc = self.fmt_doc(&e.doc);
        let mut body = String::new();
        self.indent += 1;
        for v in &e.variants {
            let variant_doc = self.fmt_doc(&v.doc);
            if v.args.is_empty() {
                body.push_str(&format!("{}{}{};\n", self.ind(), variant_doc, v.name));
            } else {
                let args: Vec<String> = v.args.iter().map(|t| self.fmt_type(t)).collect();
                body.push_str(&format!("{}{}{}({});\n", self.ind(), variant_doc, v.name, args.join(", ")));
            }
        }
        self.indent -= 1;
        format!("{}{}{}enum {}\n{}{{\n{}{}}}", ann, doc, self.ind(), e.name, self.ind(), body, self.ind())
    }

    fn fmt_trait(&mut self, t: &Trait) -> String {
        let ann = self.fmt_annotations(&t.annotations);
        let doc = self.fmt_doc(&t.doc);
        let mut body = String::new();
        self.indent += 1;
        for f in &t.methods {
            let method_doc = self.fmt_doc(&f.doc);
            let type_params = fmt_type_params(&f.type_params);
            let params = self.fmt_params(&f.params);
            let ret = self.fmt_type(&f.ret_type);
            body.push_str(&format!("{}{}fn {}{}({}) -> {};\n",
                self.ind(), method_doc, f.name, type_params, params, ret));
        }
        self.indent -= 1;
        format!("{}{}{}trait {}\n{}{{\n{}{}}}", ann, doc, self.ind(), t.name, self.ind(), body, self.ind())
    }

    fn fmt_import(&self, i: &Import) -> String {
        let path = i.path.join("::");
        match &i.alias {
            Some(a) => format!("import {} as {};", path, a),
            None => format!("import {};", path),
        }
    }

    fn ind_inner(&self) -> String {
        "    ".repeat(self.indent + 1)
    }

    fn fmt_block_stmt(&mut self, stmt: &Stmt) -> String {
        match &stmt.node {
            StmtKind::Block(stmts) => self.fmt_block(stmts),
            other => {
                let s = self.fmt_stmt_kind(other);
                format!("{}{{\n{}{}\n{}}}", self.ind(), self.ind_inner(), s, self.ind())
            }
        }
    }

    fn fmt_block(&mut self, stmts: &[Stmt]) -> String {
        let mut body = String::new();
        self.indent += 1;
        for stmt in stmts {
            let s = self.fmt_stmt_kind(&stmt.node);
            if !s.is_empty() {
                body.push_str(&format!("{}{}\n", self.ind(), s));
            }
        }
        self.indent -= 1;
        format!("{}{{\n{}{}}}", self.ind(), body, self.ind())
    }

    fn fmt_stmt_kind(&mut self, stmt: &StmtKind) -> String {
        match stmt {
            StmtKind::Expr(e) => format!("{};", self.fmt_expr(&e.node)),
            StmtKind::Let { name, ty, value } => {
                let ty_str = ty.as_ref().map(|t| format!(": {}", self.fmt_type(t))).unwrap_or_default();
                let val_str = value.as_ref().map(|v| format!(" = {}", self.fmt_expr(&v.node))).unwrap_or_default();
                format!("let {}{}{};", name, ty_str, val_str)
            }
            StmtKind::Var { name, ty, value, .. } => {
                let ty_str = ty.as_ref().map(|t| format!(": {}", self.fmt_type(t))).unwrap_or_default();
                let val_str = value.as_ref().map(|v| format!(" = {}", self.fmt_expr(&v.node))).unwrap_or_default();
                format!("var {}{}{};", name, ty_str, val_str)
            }
            StmtKind::Assignment { target, value } => {
                format!("{} = {};", self.fmt_expr(&target.node), self.fmt_expr(&value.node))
            }
            StmtKind::If { cond, then_branch, else_branch } => {
                let cond_str = self.fmt_expr(&cond.node);
                let then_str = self.fmt_block_stmt(then_branch);
                let mut s = format!("if {}\n{}", cond_str, then_str);
                if let Some(else_b) = else_branch {
                    match &else_b.node {
                        StmtKind::If { .. } => {
                            let else_str = self.fmt_stmt_kind(&else_b.node);
                            s.push_str(&format!("\nelse {}", else_str));
                        }
                        _ => {
                            let else_str = self.fmt_block_stmt(else_b);
                            s.push_str(&format!("\n{}else\n{}", self.ind(), else_str));
                        }
                    }
                }
                s
            }
            StmtKind::While { cond, body } => {
                let cond_str = self.fmt_expr(&cond.node);
                let body_str = self.fmt_block_stmt(body);
                format!("while {}\n{}", cond_str, body_str)
            }
            StmtKind::For { var, iter, body } => {
                let iter_str = self.fmt_expr(&iter.node);
                let body_str = self.fmt_block_stmt(body);
                format!("for {} in {}\n{}", var, iter_str, body_str)
            }
            StmtKind::ForC { init, cond, update, body } => {
                let init_str = init.as_ref().map(|s| {
                    let k = self.fmt_stmt_kind(&s.node);
                    k.trim_end_matches(';').to_string()
                }).unwrap_or_default();
                let cond_str = cond.as_ref().map(|e| self.fmt_expr(&e.node)).unwrap_or_default();
                let upd_str = update.as_ref().map(|e| self.fmt_expr(&e.node)).unwrap_or_default();
                let body_str = self.fmt_block_stmt(body);
                format!("for {}; {}; {}\n{}", init_str, cond_str, upd_str, body_str)
            }
            StmtKind::Loop { body } => {
                let body_str = self.fmt_block_stmt(body);
                format!("loop\n{}", body_str)
            }
            StmtKind::Return(Some(e)) => format!("return {};", self.fmt_expr(&e.node)),
            StmtKind::Return(None) => "return;".to_string(),
            StmtKind::Break => "break;".to_string(),
            StmtKind::Continue => "continue;".to_string(),
            StmtKind::Throw(e) => format!("throw {};", self.fmt_expr(&e.node)),
            StmtKind::Try { body, catches, finally } => {
                let body_str = self.fmt_block_stmt(body);
                let mut s = format!("try\n{}", body_str);
                for catch in catches {
                    let ty = self.fmt_type(&catch.ty);
                    let catch_body = self.fmt_block_stmt(&catch.body);
                    s.push_str(&format!("\n{}catch {}: {}\n{}", self.ind(), catch.param, ty, catch_body));
                }
                if let Some(fin) = finally {
                    let fin_str = self.fmt_block_stmt(fin);
                    s.push_str(&format!("\n{}finally\n{}", self.ind(), fin_str));
                }
                s
            }
            StmtKind::Defer(inner) => {
                let inner_str = self.fmt_stmt_kind(&inner.node);
                format!("defer {{ {} }}", inner_str)
            }
            StmtKind::Block(stmts) => self.fmt_block(stmts),
            StmtKind::Select { arms, default } => {
                let mut body = String::new();
                self.indent += 1;
                for arm in arms {
                    let ch = self.fmt_expr(&arm.channel.node);
                    let arm_body = self.fmt_block_stmt(&arm.body);
                    body.push_str(&format!("{}recv {} -> {}\n{}\n",
                        self.ind(), ch, arm.var, arm_body));
                }
                if let Some(def) = default {
                    let def_body = self.fmt_block_stmt(def);
                    body.push_str(&format!("{}default\n{}\n", self.ind(), def_body));
                }
                self.indent -= 1;
                format!("select\n{}{{\n{}{}}}", self.ind(), body, self.ind())
            }
            StmtKind::Empty => String::new(),
        }
    }

    fn fmt_expr(&mut self, expr: &ExprKind) -> String {
        match expr {
            ExprKind::Literal(lit) => fmt_literal(lit),
            ExprKind::Ident(name) => name.clone(),
            ExprKind::This => "this".to_string(),
            ExprKind::Channel => "channel".to_string(),
            ExprKind::Break => "break".to_string(),
            ExprKind::Continue => "continue".to_string(),
            ExprKind::ArrayLiteral(elems) => {
                let items: Vec<String> = elems.iter().map(|e| self.fmt_expr(&e.node)).collect();
                format!("[{}]", items.join(", "))
            }
            ExprKind::MapLiteral(pairs) => {
                let items: Vec<String> = pairs.iter()
                    .map(|(k, v)| format!("{} => {}", self.fmt_expr(&k.node), self.fmt_expr(&v.node)))
                    .collect();
                format!("@{{{}}}", items.join(", "))
            }
            ExprKind::Tuple(elems) => {
                let items: Vec<String> = elems.iter().map(|e| self.fmt_expr(&e.node)).collect();
                format!("({})", items.join(", "))
            }
            ExprKind::Binary { op, lhs, rhs } => {
                format!("{} {} {}", self.fmt_expr(&lhs.node), fmt_binop(op), self.fmt_expr(&rhs.node))
            }
            ExprKind::Unary { op, operand } => {
                format!("{}{}", fmt_unop(op), self.fmt_expr(&operand.node))
            }
            ExprKind::CompoundAssign { op, target, value } => {
                format!("{} {}= {}", self.fmt_expr(&target.node), fmt_compound_op(op), self.fmt_expr(&value.node))
            }
            ExprKind::Call { func, args } => {
                let args_str: Vec<String> = args.iter().map(|a| self.fmt_expr(&a.node)).collect();
                format!("{}({})", self.fmt_expr(&func.node), args_str.join(", "))
            }
            ExprKind::MethodCall { obj, method, args } => {
                let args_str: Vec<String> = args.iter().map(|a| self.fmt_expr(&a.node)).collect();
                format!("{}.{}({})", self.fmt_expr(&obj.node), method, args_str.join(", "))
            }
            ExprKind::Index { obj, index } => {
                format!("{}[{}]", self.fmt_expr(&obj.node), self.fmt_expr(&index.node))
            }
            ExprKind::FieldAccess { obj, field } => {
                format!("{}.{}", self.fmt_expr(&obj.node), field)
            }
            ExprKind::TupleIndex { tuple, index } => {
                format!("{}.{}", self.fmt_expr(&tuple.node), index)
            }
            ExprKind::New { class, type_args, args } => {
                let ta = if type_args.is_empty() {
                    String::new()
                } else {
                    let ts: Vec<String> = type_args.iter().map(|t| self.fmt_type(t)).collect();
                    format!("<{}>", ts.join(", "))
                };
                let args_str: Vec<String> = args.iter().map(|a| self.fmt_expr(&a.node)).collect();
                format!("new {}{}({})", class, ta, args_str.join(", "))
            }
            ExprKind::StructLiteral { name, fields } => {
                let fs: Vec<String> = fields.iter()
                    .map(|(k, v)| format!("{}: {}", k, self.fmt_expr(&v.node)))
                    .collect();
                format!("{} {{ {} }}", name, fs.join(", "))
            }
            ExprKind::EnumValue { enum_name, variant, args } => {
                if args.is_empty() {
                    format!("{}::{}", enum_name, variant)
                } else {
                    let as_: Vec<String> = args.iter().map(|a| self.fmt_expr(&a.node)).collect();
                    format!("{}::{}({})", enum_name, variant, as_.join(", "))
                }
            }
            ExprKind::SuperCall { method, args } => {
                let args_str: Vec<String> = args.iter().map(|a| self.fmt_expr(&a.node)).collect();
                format!("super.{}({})", method, args_str.join(", "))
            }
            ExprKind::If { cond, then_branch, else_branch } => {
                let c = self.fmt_expr(&cond.node);
                let t = self.fmt_expr(&then_branch.node);
                match else_branch {
                    Some(e) => format!("if {} {{ {} }} else {{ {} }}", c, t, self.fmt_expr(&e.node)),
                    None => format!("if {} {{ {} }}", c, t),
                }
            }
            ExprKind::While { cond, body } => {
                format!("while {} {{ {} }}", self.fmt_expr(&cond.node), self.fmt_expr(&body.node))
            }
            ExprKind::For { var, iter, body } => {
                format!("for {} in {} {{ {} }}", var, self.fmt_expr(&iter.node), self.fmt_expr(&body.node))
            }
            ExprKind::Loop { body } => {
                format!("loop {{ {} }}", self.fmt_expr(&body.node))
            }
            ExprKind::Block(stmts) => {
                let parts: Vec<String> = stmts.iter()
                    .map(|s| self.fmt_stmt_kind(&s.node))
                    .filter(|s| !s.is_empty())
                    .collect();
                format!("{{ {} }}", parts.join(" "))
            }
            ExprKind::Match { expr, cases } => {
                let e = self.fmt_expr(&expr.node);
                let mut body = String::new();
                self.indent += 1;
                for case in cases {
                    let pat = fmt_pattern(&case.pattern);
                    let guard = case.guard.as_ref()
                        .map(|g| format!(" if {}", self.fmt_expr(&g.node)))
                        .unwrap_or_default();
                    let case_body = self.fmt_expr(&case.body.node);
                    body.push_str(&format!("{}{}{} => {};\n", self.ind(), pat, guard, case_body));
                }
                self.indent -= 1;
                format!("match {}\n{}{{\n{}{}}}", e, self.ind(), body, self.ind())
            }
            ExprKind::Lambda { params, ret_type, body } => {
                let ps = self.fmt_params(params);
                let ret = ret_type.as_ref().map(|t| format!(" -> {}", self.fmt_type(t))).unwrap_or_default();
                format!("|{}|{} => {}", ps, ret, self.fmt_expr(&body.node))
            }
            ExprKind::Spawn(e) => format!("spawn {}", self.fmt_expr(&e.node)),
            ExprKind::Await(e) => format!("await {}", self.fmt_expr(&e.node)),
            ExprKind::Send { channel, value } => {
                format!("send {} -> {}", self.fmt_expr(&channel.node), self.fmt_expr(&value.node))
            }
            ExprKind::Recv(e) => format!("recv {}", self.fmt_expr(&e.node)),
            ExprKind::Cast { expr, ty } => format!("{} as {}", self.fmt_expr(&expr.node), self.fmt_type(ty)),
            ExprKind::Is { expr, ty } => format!("{} is {}", self.fmt_expr(&expr.node), self.fmt_type(ty)),
            ExprKind::Range { start, end, inclusive } => {
                let op = if *inclusive { "..." } else { ".." };
                format!("{}{}{}", self.fmt_expr(&start.node), op, self.fmt_expr(&end.node))
            }
            ExprKind::Return(Some(e)) => format!("return {}", self.fmt_expr(&e.node)),
            ExprKind::Return(None) => "return".to_string(),
            ExprKind::Throw(e) => format!("throw {}", self.fmt_expr(&e.node)),
            ExprKind::Assign { target, value } => {
                format!("{} = {}", self.fmt_expr(&target.node), self.fmt_expr(&value.node))
            }
            ExprKind::Try { body, catches, finally } => {
                let b = self.fmt_expr(&body.node);
                let mut s = format!("try {{ {} }}", b);
                for catch in catches {
                    let ty = self.fmt_type(&catch.ty);
                    let cb = self.fmt_stmt_kind(&catch.body.node);
                    s.push_str(&format!(" catch {}: {} {{ {} }}", catch.param, ty, cb));
                }
                if let Some(fin) = finally {
                    s.push_str(&format!(" finally {{ {} }}", self.fmt_expr(&fin.node)));
                }
                s
            }
        }
    }

    fn fmt_params(&mut self, params: &[Param]) -> String {
        params.iter()
            .map(|p| format!("{}: {}", p.name, self.fmt_type(&p.param_type)))
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn fmt_type(&self, ty: &Type) -> String {
        match ty {
            Type::Int8 => "Int8".to_string(),
            Type::Int16 => "Int16".to_string(),
            Type::Int32 => "Int32".to_string(),
            Type::Int64 => "Int64".to_string(),
            Type::UInt8 => "UInt8".to_string(),
            Type::UInt16 => "UInt16".to_string(),
            Type::UInt32 => "UInt32".to_string(),
            Type::UInt64 => "UInt64".to_string(),
            Type::Float32 => "Float32".to_string(),
            Type::Float64 => "Float64".to_string(),
            Type::Bool => "Bool".to_string(),
            Type::Char => "Char".to_string(),
            Type::String => "String".to_string(),
            Type::Unit => "Unit".to_string(),
            Type::Never => "Never".to_string(),
            Type::Any => "Any".to_string(),
            Type::Infer => "_".to_string(),
            Type::Named(n) => n.clone(),
            Type::Generic { name, args } => {
                let as_: Vec<String> = args.iter().map(|t| self.fmt_type(t)).collect();
                format!("{}<{}>", name, as_.join(", "))
            }
            Type::Array(inner) => format!("Array<{}>", self.fmt_type(inner)),
            Type::Map(k, v) => format!("Map<{}, {}>", self.fmt_type(k), self.fmt_type(v)),
            Type::Mutable(inner) => format!("mut {}", self.fmt_type(inner)),
            Type::Ref(inner) => format!("&{}", self.fmt_type(inner)),
            Type::Fn { params, ret } => {
                let ps: Vec<String> = params.iter().map(|t| self.fmt_type(t)).collect();
                format!("fn({}) -> {}", ps.join(", "), self.fmt_type(ret))
            }
            Type::Tuple(types) => {
                let ts: Vec<String> = types.iter().map(|t| self.fmt_type(t)).collect();
                format!("({})", ts.join(", "))
            }
        }
    }
}

fn fmt_type_params(params: &[String]) -> String {
    if params.is_empty() {
        String::new()
    } else {
        format!("<{}>", params.join(", "))
    }
}

fn fmt_visibility(vis: &Visibility) -> &'static str {
    match vis {
        Visibility::Public => "",
        Visibility::Private => "private ",
        Visibility::Protected => "protected ",
        Visibility::Package => "",
    }
}

fn fmt_literal(lit: &Literal) -> String {
    match lit {
        Literal::Integer(n) => n.to_string(),
        Literal::Float(f) => {
            let s = format!("{}", f);
            if s.contains('.') { s } else { format!("{}.0", s) }
        }
        Literal::String(s) => format!("\"{}\"", s),
        Literal::Char(c) => format!("'{}'", c),
        Literal::Byte(b) => format!("{}b", b),
        Literal::Bool(b) => b.to_string(),
        Literal::Null => "null".to_string(),
    }
}

fn fmt_binop(op: &BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+", BinaryOp::Sub => "-", BinaryOp::Mul => "*",
        BinaryOp::Div => "/", BinaryOp::Mod => "%", BinaryOp::And => "&&",
        BinaryOp::Or => "||", BinaryOp::BitAnd => "&", BinaryOp::BitOr => "|", BinaryOp::Xor => "^",
        BinaryOp::Shl => "<<", BinaryOp::Shr => ">>", BinaryOp::ShrArith => ">>>",
        BinaryOp::Eq => "==", BinaryOp::Ne => "!=", BinaryOp::Lt => "<",
        BinaryOp::Le => "<=", BinaryOp::Gt => ">", BinaryOp::Ge => ">=",
    }
}

fn fmt_unop(op: &UnaryOp) -> &'static str {
    match op { UnaryOp::Neg => "-", UnaryOp::Not => "!", UnaryOp::BitNot => "~" }
}

fn fmt_compound_op(op: &CompoundOp) -> &'static str {
    match op {
        CompoundOp::Add => "+", CompoundOp::Sub => "-", CompoundOp::Mul => "*",
        CompoundOp::Div => "/", CompoundOp::Mod => "%", CompoundOp::BitAnd => "&",
        CompoundOp::BitOr => "|", CompoundOp::BitXor => "^", CompoundOp::Shl => "<<",
        CompoundOp::Shr => ">>", CompoundOp::ShrArith => ">>>",
    }
}

fn fmt_pattern(pat: &Pattern) -> String {
    match pat {
        Pattern::Wildcard(_) => "_".to_string(),
        Pattern::Ident(name, sub, _) => match sub {
            Some(p) => format!("{} @ {}", name, fmt_pattern(p)),
            None => name.clone(),
        },
        Pattern::Literal(lit, _) => fmt_literal(lit),
        Pattern::Tuple(pats, _) => {
            let ps: Vec<String> = pats.iter().map(fmt_pattern).collect();
            format!("({})", ps.join(", "))
        }
        Pattern::EnumVariant { enum_name, variant, args, .. } => {
            if args.is_empty() {
                format!("{}::{}", enum_name, variant)
            } else {
                let as_: Vec<String> = args.iter().map(fmt_pattern).collect();
                format!("{}::{}({})", enum_name, variant, as_.join(", "))
            }
        }
    }
}
