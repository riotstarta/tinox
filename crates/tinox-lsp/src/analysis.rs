use tinox_common::{Pos, Span};
use tinox_parser::ast::*;
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, Location, Position, Range, Url};

pub fn lsp_pos_to_offset(src: &str, pos: Position) -> u32 {
    let mut line = 0u32;
    let mut col = 0u32;
    let mut offset = 0u32;
    for ch in src.chars() {
        if line == pos.line && col == pos.character {
            return offset;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
        offset += ch.len_utf8() as u32;
    }
    offset
}

fn span_contains(span: Span, offset: u32) -> bool {
    span.start.offset <= offset && offset <= span.end.offset
}

pub fn pos_to_lsp(p: Pos) -> Position {
    Position {
        line: p.line.saturating_sub(1),
        character: p.column.saturating_sub(1),
    }
}

pub fn span_to_range(span: Span) -> Range {
    Range {
        start: pos_to_lsp(span.start),
        end: pos_to_lsp(span.end),
    }
}

// --- Hover ---

pub fn hover_at(source: &SourceFile, offset: u32) -> Option<String> {
    for decl in &source.decls {
        if let Some(s) = hover_decl(decl, offset) {
            return Some(s);
        }
    }
    None
}

fn hover_decl(decl: &Decl, offset: u32) -> Option<String> {
    // Note: decl.span is a point-span (mk_span limitation), so we skip the outer guard
    // and rely on expression-level spans (from tokens) which are accurate.
    match &decl.node {
        DeclKind::Function(f) => {
            // f.span is a point at the start of `fn` keyword
            // Show full signature when hovering the fn keyword or name (before the body starts)
            if offset < f.body.span.start.offset {
                let mut content = fn_signature(f);
                if let Some(doc) = &f.doc {
                    content.push_str("\n\n---\n\n");
                    content.push_str(&format_doc(doc));
                }
                return Some(content);
            }
            hover_stmt(&f.body, offset)
        }
        DeclKind::Class(c) => {
            // Check if hovering on class declaration itself
            if offset < c.span.start.offset + c.name.len() as u32 + 10 {
                let mut content = format!("class {}", c.name);
                if let Some(doc) = &c.doc {
                    content.push_str("\n\n---\n\n");
                    content.push_str(&format_doc(doc));
                }
                return Some(content);
            }
            for m in &c.methods {
                if let Some(s) = hover_stmt(&m.body, offset) {
                    return Some(s);
                }
                if span_contains(m.span, offset) {
                    let mut content = method_signature(&c.name, m);
                    if let Some(doc) = &m.doc {
                        content.push_str("\n\n---\n\n");
                        content.push_str(&format_doc(doc));
                    }
                    return Some(content);
                }
            }
            // Check fields
            for f in &c.fields {
                if span_contains(f.span, offset) {
                    let mut content = format!("{}: {}", f.name, type_str(&f.field_type));
                    if let Some(doc) = &f.doc {
                        content.push_str("\n\n---\n\n");
                        content.push_str(&format_doc(doc));
                    }
                    return Some(content);
                }
            }
            None
        }
        DeclKind::Enum(e) => {
            if offset < e.span.start.offset + e.name.len() as u32 + 10 {
                let mut content = format!("enum {}", e.name);
                if let Some(doc) = &e.doc {
                    content.push_str("\n\n---\n\n");
                    content.push_str(&format_doc(doc));
                }
                return Some(content);
            }
            for v in &e.variants {
                if span_contains(v.span, offset) {
                    let mut content = format!("{}.{}", e.name, v.name);
                    if let Some(doc) = &v.doc {
                        content.push_str("\n\n---\n\n");
                        content.push_str(&format_doc(doc));
                    }
                    return Some(content);
                }
            }
            None
        }
        DeclKind::Interface(i) => {
            if offset < i.span.start.offset + i.name.len() as u32 + 10 {
                let mut content = format!("interface {}", i.name);
                if let Some(doc) = &i.doc {
                    content.push_str("\n\n---\n\n");
                    content.push_str(&format_doc(doc));
                }
                return Some(content);
            }
            for m in &i.methods {
                if span_contains(m.span, offset) {
                    let mut content = fn_signature(m);
                    if let Some(doc) = &m.doc {
                        content.push_str("\n\n---\n\n");
                        content.push_str(&format_doc(doc));
                    }
                    return Some(content);
                }
            }
            None
        }
        DeclKind::Trait(t) => {
            if offset < t.span.start.offset + t.name.len() as u32 + 10 {
                let mut content = format!("trait {}", t.name);
                if let Some(doc) = &t.doc {
                    content.push_str("\n\n---\n\n");
                    content.push_str(&format_doc(doc));
                }
                return Some(content);
            }
            for m in &t.methods {
                if span_contains(m.span, offset) {
                    let mut content = fn_signature(m);
                    if let Some(doc) = &m.doc {
                        content.push_str("\n\n---\n\n");
                        content.push_str(&format_doc(doc));
                    }
                    return Some(content);
                }
            }
            None
        }
        DeclKind::Immutable(u) => {
            if offset < u.span.start.offset + u.name.len() as u32 + 20 {
                let fields: Vec<String> = u.fields.iter()
                    .map(|f| format!("{}: {}", f.name, type_str(&f.param_type)))
                    .collect();
                return Some(format!("immutable {}({})", u.name, fields.join(", ")));
            }
            for f in &u.fields {
                if span_contains(f.span, offset) {
                    return Some(format!("{}: {}", f.name, type_str(&f.param_type)));
                }
            }
            None
        }
        _ => None,
    }
}

fn format_doc(doc: &str) -> String {
    let lines: Vec<&str> = doc.lines().collect();
    if lines.len() <= 1 {
        doc.trim().to_string()
    } else {
        let mut out = String::new();
        for line in &lines {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                out.push('\n');
                continue;
            }
            let cleaned = trimmed.trim_start_matches('*').trim_end_matches("*/").trim();
            if !cleaned.is_empty() {
                out.push_str(cleaned);
                out.push('\n');
            }
        }
        out.trim_end().to_string()
    }
}

fn hover_stmt(stmt: &Stmt, offset: u32) -> Option<String> {
    match &stmt.node {
        StmtKind::Let { name, ty, value, .. } => {
            let ty_str = ty.as_ref().map(type_str).unwrap_or_else(|| "inferred".into());
            if let Some(v) = value {
                if let Some(s) = hover_expr(v, offset) {
                    return Some(s);
                }
            }
            Some(format!("let {}: {}", name, ty_str))
        }
        StmtKind::Var { name, ty, value, .. } => {
            let ty_str = ty.as_ref().map(type_str).unwrap_or_else(|| "inferred".into());
            if let Some(v) = value {
                if let Some(s) = hover_expr(v, offset) {
                    return Some(s);
                }
            }
            Some(format!("var {}: {}", name, ty_str))
        }
        StmtKind::Block(stmts) => {
            for s in stmts {
                if let Some(r) = hover_stmt(s, offset) {
                    return Some(r);
                }
            }
            None
        }
        StmtKind::Expr(e) => hover_expr(e, offset),
        StmtKind::Return(Some(e)) => hover_expr(e, offset),
        StmtKind::If { cond, then_branch, else_branch } => {
            hover_expr(cond, offset)
                .or_else(|| hover_stmt(then_branch, offset))
                .or_else(|| else_branch.as_ref().and_then(|e| hover_stmt(e, offset)))
        }
        StmtKind::While { cond, body } => {
            hover_expr(cond, offset).or_else(|| hover_stmt(body, offset))
        }
        StmtKind::For { body, .. } => hover_stmt(body, offset),
        StmtKind::Loop { body } => hover_stmt(body, offset),
        _ => None,
    }
}

fn hover_expr(expr: &Expr, offset: u32) -> Option<String> {
    if !span_contains(expr.span, offset) {
        return None;
    }
    match &expr.node {
        ExprKind::Call { func, args } => {
            if let ExprKind::Ident(name) = &func.node {
                if span_contains(func.span, offset) {
                    return Some(format!("fn {}(…)", name));
                }
            }
            for a in args {
                if let Some(s) = hover_expr(a, offset) {
                    return Some(s);
                }
            }
            None
        }
        ExprKind::Literal(lit) => Some(format!("{}: {}", literal_str(lit), type_str(&lit.ty()))),
        ExprKind::Binary { lhs, rhs, .. } => {
            hover_expr(lhs, offset).or_else(|| hover_expr(rhs, offset))
        }
        ExprKind::Block(stmts) => {
            for s in stmts {
                if let Some(r) = hover_stmt(s, offset) {
                    return Some(r);
                }
            }
            None
        }
        _ => None,
    }
}

pub fn fn_signature(f: &Function) -> String {
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, type_str(&p.param_type)))
        .collect();
    let async_ = if f.is_async { "async " } else { "" };
    format!("{}fn {}({}) -> {}", async_, f.name, params.join(", "), type_str(&f.ret_type))
}

pub fn method_signature(class: &str, m: &Method) -> String {
    let params: Vec<String> = m
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, type_str(&p.param_type)))
        .collect();
    format!(
        "{}.{}({}) -> {}",
        class,
        m.name,
        params.join(", "),
        type_str(&m.ret_type)
    )
}

pub fn type_str(ty: &Type) -> String {
    match ty {
        Type::Int8 => "Int8".into(),
        Type::Int16 => "Int16".into(),
        Type::Int32 => "Int32".into(),
        Type::Int64 => "Int64".into(),
        Type::UInt8 => "UInt8".into(),
        Type::UInt16 => "UInt16".into(),
        Type::UInt32 => "UInt32".into(),
        Type::UInt64 => "UInt64".into(),
        Type::Float32 => "Float32".into(),
        Type::Float64 => "Float64".into(),
        Type::Bool => "Bool".into(),
        Type::String => "String".into(),
        Type::Char => "Char".into(),
        Type::Nothing => "Nothing".into(),
        Type::Never => "Never".into(),
        Type::Any => "Any".into(),
        Type::Infer => "_".into(),
        Type::Named(n) => n.clone(),
        Type::Array(inner) => format!("Array<{}>", type_str(inner)),
        Type::Tuple(types) => {
            let inner: Vec<_> = types.iter().map(type_str).collect();
            format!("({})", inner.join(", "))
        }
        Type::Fn { params, ret } => {
            let ps: Vec<_> = params.iter().map(type_str).collect();
            format!("fn({}) -> {}", ps.join(", "), type_str(ret))
        }
        Type::Generic { name, args } => {
            let as_: Vec<_> = args.iter().map(type_str).collect();
            format!("{}<{}>", name, as_.join(", "))
        }
        Type::Map(k, v) => format!("Map<{}, {}>", type_str(k), type_str(v)),
        Type::Mutable(inner) => format!("mut {}", type_str(inner)),
        Type::Ref(inner) => format!("&{}", type_str(inner)),
    }
}

fn literal_str(lit: &Literal) -> String {
    match lit {
        Literal::Integer(n) => n.to_string(),
        Literal::Float(f) => f.to_string(),
        Literal::String(s) => format!("\"{}\"", s),
        Literal::Bool(b) => b.to_string(),
        Literal::Char(c) => format!("'{}'", c),
        Literal::Byte(b) => format!("0x{:02x}", b),
        Literal::Null => "null".into(),
    }
}

// --- Completion ---

const KEYWORDS: &[(&str, CompletionItemKind)] = &[
    ("fn", CompletionItemKind::KEYWORD),
    ("let", CompletionItemKind::KEYWORD),
    ("var", CompletionItemKind::KEYWORD),
    ("return", CompletionItemKind::KEYWORD),
    ("if", CompletionItemKind::KEYWORD),
    ("else", CompletionItemKind::KEYWORD),
    ("while", CompletionItemKind::KEYWORD),
    ("for", CompletionItemKind::KEYWORD),
    ("in", CompletionItemKind::KEYWORD),
    ("loop", CompletionItemKind::KEYWORD),
    ("break", CompletionItemKind::KEYWORD),
    ("continue", CompletionItemKind::KEYWORD),
    ("class", CompletionItemKind::KEYWORD),
    ("interface", CompletionItemKind::KEYWORD),
    ("enum", CompletionItemKind::KEYWORD),
    ("extends", CompletionItemKind::KEYWORD),
    ("implements", CompletionItemKind::KEYWORD),
    ("new", CompletionItemKind::KEYWORD),
    ("this", CompletionItemKind::KEYWORD),
    ("match", CompletionItemKind::KEYWORD),
    ("try", CompletionItemKind::KEYWORD),
    ("catch", CompletionItemKind::KEYWORD),
    ("finally", CompletionItemKind::KEYWORD),
    ("import", CompletionItemKind::KEYWORD),
    ("async", CompletionItemKind::KEYWORD),
    ("spawn", CompletionItemKind::KEYWORD),
    ("await", CompletionItemKind::KEYWORD),
    ("send", CompletionItemKind::KEYWORD),
    ("recv", CompletionItemKind::KEYWORD),
    ("channel", CompletionItemKind::KEYWORD),
    ("true", CompletionItemKind::VALUE),
    ("false", CompletionItemKind::VALUE),
    ("null", CompletionItemKind::VALUE),
];

const BUILTINS: &[(&str, &str)] = &[
    ("println", "println(value: Any)"),
    ("print", "print(value: Any)"),
    ("assert", "assert(cond: Bool)"),
    ("exit", "exit(code: Int64)"),
    ("abs", "abs(x: Int64) -> Int64"),
    ("min", "min(a: Int64, b: Int64) -> Int64"),
    ("max", "max(a: Int64, b: Int64) -> Int64"),
    ("sqrt", "sqrt(x: Float64) -> Float64"),
    ("pow", "pow(x: Float64, y: Float64) -> Float64"),
    ("floor", "floor(x: Float64) -> Float64"),
    ("ceil", "ceil(x: Float64) -> Float64"),
    ("round", "round(x: Float64) -> Float64"),
];

pub fn completions(source: &SourceFile) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = Vec::new();

    for (kw, kind) in KEYWORDS {
        items.push(CompletionItem {
            label: kw.to_string(),
            kind: Some(*kind),
            ..Default::default()
        });
    }

    for (name, detail) in BUILTINS {
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some(detail.to_string()),
            ..Default::default()
        });
    }

    for decl in &source.decls {
        match &decl.node {
            DeclKind::Function(f) => {
                let doc = f.doc.as_ref().map(|d| format_doc(d));
                items.push(CompletionItem {
                    label: f.name.clone(),
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail: Some(fn_signature(f)),
                    documentation: doc.map(|d| tower_lsp::lsp_types::Documentation::MarkupContent(
                        tower_lsp::lsp_types::MarkupContent {
                            kind: tower_lsp::lsp_types::MarkupKind::Markdown,
                            value: d,
                        }
                    )),
                    ..Default::default()
                });
            }
            DeclKind::Class(c) => {
                let doc = c.doc.as_ref().map(|d| format_doc(d));
                items.push(CompletionItem {
                    label: c.name.clone(),
                    kind: Some(CompletionItemKind::CLASS),
                    detail: Some(format!("class {}", c.name)),
                    documentation: doc.map(|d| tower_lsp::lsp_types::Documentation::MarkupContent(
                        tower_lsp::lsp_types::MarkupContent {
                            kind: tower_lsp::lsp_types::MarkupKind::Markdown,
                            value: d,
                        }
                    )),
                    ..Default::default()
                });
            }
            DeclKind::Immutable(u) => {
                let fields: Vec<String> = u.fields.iter()
                    .map(|f| format!("{}: {}", f.name, type_str(&f.param_type)))
                    .collect();
                items.push(CompletionItem {
                    label: u.name.clone(),
                    kind: Some(CompletionItemKind::STRUCT),
                    detail: Some(format!("immutable {}({})", u.name, fields.join(", "))),
                    ..Default::default()
                });
            }
            DeclKind::Enum(e) => {
                let doc = e.doc.as_ref().map(|d| format_doc(d));
                items.push(CompletionItem {
                    label: e.name.clone(),
                    kind: Some(CompletionItemKind::ENUM),
                    documentation: doc.map(|d| tower_lsp::lsp_types::Documentation::MarkupContent(
                        tower_lsp::lsp_types::MarkupContent {
                            kind: tower_lsp::lsp_types::MarkupKind::Markdown,
                            value: d,
                        }
                    )),
                    ..Default::default()
                });
                for v in &e.variants {
                    let doc = v.doc.as_ref().map(|d| format_doc(d));
                    items.push(CompletionItem {
                        label: format!("{}.{}", e.name, v.name),
                        kind: Some(CompletionItemKind::ENUM_MEMBER),
                        documentation: doc.map(|d| tower_lsp::lsp_types::Documentation::MarkupContent(
                            tower_lsp::lsp_types::MarkupContent {
                                kind: tower_lsp::lsp_types::MarkupKind::Markdown,
                                value: d,
                            }
                        )),
                        ..Default::default()
                    });
                }
            }
            DeclKind::Interface(i) => {
                let doc = i.doc.as_ref().map(|d| format_doc(d));
                items.push(CompletionItem {
                    label: i.name.clone(),
                    kind: Some(CompletionItemKind::INTERFACE),
                    detail: Some(format!("interface {}", i.name)),
                    documentation: doc.map(|d| tower_lsp::lsp_types::Documentation::MarkupContent(
                        tower_lsp::lsp_types::MarkupContent {
                            kind: tower_lsp::lsp_types::MarkupKind::Markdown,
                            value: d,
                        }
                    )),
                    ..Default::default()
                });
            }
            _ => {}
        }
    }

    items
}

// --- Go to Definition ---

pub fn definition_at(source: &SourceFile, uri: &Url, offset: u32) -> Option<Location> {
    let target = ident_at(source, offset)?;
    for decl in &source.decls {
        let (name, span) = match &decl.node {
            DeclKind::Function(f) => (f.name.as_str(), f.span),
            DeclKind::Class(c) => (c.name.as_str(), c.span),
            DeclKind::Enum(e) => (e.name.as_str(), e.span),
            DeclKind::Interface(i) => (i.name.as_str(), i.span),
            DeclKind::Immutable(u) => (u.name.as_str(), u.span),
            _ => continue,
        };
        if name == target {
            return Some(Location {
                uri: uri.clone(),
                range: span_to_range(span),
            });
        }
    }
    None
}

fn ident_at(source: &SourceFile, offset: u32) -> Option<String> {
    for decl in &source.decls {
        if let Some(s) = ident_in_decl(decl, offset) {
            return Some(s);
        }
    }
    None
}

fn ident_in_decl(decl: &Decl, offset: u32) -> Option<String> {
    if !span_contains(decl.span, offset) {
        return None;
    }
    match &decl.node {
        DeclKind::Function(f) => ident_in_stmt(&f.body, offset),
        DeclKind::Class(c) => {
            for m in &c.methods {
                if span_contains(m.span, offset) {
                    if let Some(s) = ident_in_stmt(&m.body, offset) {
                        return Some(s);
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn ident_in_stmt(stmt: &Stmt, offset: u32) -> Option<String> {
    if !span_contains(stmt.span, offset) {
        return None;
    }
    match &stmt.node {
        StmtKind::Block(stmts) => {
            for s in stmts {
                if let Some(r) = ident_in_stmt(s, offset) {
                    return Some(r);
                }
            }
            None
        }
        StmtKind::Expr(e) => ident_in_expr(e, offset),
        StmtKind::Return(Some(e)) => ident_in_expr(e, offset),
        StmtKind::Let { value: Some(v), .. } => ident_in_expr(v, offset),
        StmtKind::Var { value: Some(v), .. } => ident_in_expr(v, offset),
        StmtKind::If { cond, then_branch, else_branch } => {
            ident_in_expr(cond, offset)
                .or_else(|| ident_in_stmt(then_branch, offset))
                .or_else(|| else_branch.as_ref().and_then(|e| ident_in_stmt(e, offset)))
        }
        StmtKind::While { cond, body } => {
            ident_in_expr(cond, offset).or_else(|| ident_in_stmt(body, offset))
        }
        StmtKind::For { body, .. } => ident_in_stmt(body, offset),
        StmtKind::Loop { body } => ident_in_stmt(body, offset),
        _ => None,
    }
}

fn ident_in_expr(expr: &Expr, offset: u32) -> Option<String> {
    if !span_contains(expr.span, offset) {
        return None;
    }
    match &expr.node {
        ExprKind::Ident(name) => Some(name.clone()),
        ExprKind::Call { func, args } => {
            ident_in_expr(func, offset).or_else(|| {
                args.iter().find_map(|a| ident_in_expr(a, offset))
            })
        }
        ExprKind::Binary { lhs, rhs, .. } => {
            ident_in_expr(lhs, offset).or_else(|| ident_in_expr(rhs, offset))
        }
        ExprKind::Block(stmts) => stmts.iter().find_map(|s| ident_in_stmt(s, offset)),
        _ => None,
    }
}

// --- Document Symbols ---

pub fn document_symbols(source: &SourceFile) -> Vec<tower_lsp::lsp_types::DocumentSymbol> {
    source.decls.iter().filter_map(decl_to_symbol).collect()
}

#[allow(deprecated)]
fn decl_to_symbol(decl: &Decl) -> Option<tower_lsp::lsp_types::DocumentSymbol> {
    use tower_lsp::lsp_types::{DocumentSymbol, SymbolKind};
    match &decl.node {
        DeclKind::Function(f) => {
            let detail = if let Some(doc) = &f.doc {
                Some(format!("{}\n\n{}", fn_signature(f), format_doc(doc)))
            } else {
                Some(fn_signature(f))
            };
            Some(DocumentSymbol {
                name: f.name.clone(),
                detail,
                kind: SymbolKind::FUNCTION,
                range: span_to_range(f.span),
                selection_range: span_to_range(f.span),
                children: None,
                tags: None,
                deprecated: None,
            })
        }
        DeclKind::Class(c) => {
            let detail = if let Some(doc) = &c.doc {
                Some(format_doc(doc))
            } else {
                None
            };
            let children: Vec<_> = c
                .methods
                .iter()
                .map(|m| {
                    let method_detail = if let Some(doc) = &m.doc {
                        Some(format!("{}\n\n{}", method_signature(&c.name, m), format_doc(doc)))
                    } else {
                        Some(method_signature(&c.name, m))
                    };
                    DocumentSymbol {
                        name: m.name.clone(),
                        detail: method_detail,
                        kind: SymbolKind::METHOD,
                        range: span_to_range(m.span),
                        selection_range: span_to_range(m.span),
                        children: None,
                        tags: None,
                deprecated: None,
                            }
                })
                .collect();
            Some(DocumentSymbol {
                name: c.name.clone(),
                detail,
                kind: SymbolKind::CLASS,
                range: span_to_range(c.span),
                selection_range: span_to_range(c.span),
                children: Some(children),
                tags: None,
                deprecated: None,
            })
        }
        DeclKind::Enum(e) => {
            let detail = if let Some(doc) = &e.doc {
                Some(format_doc(doc))
            } else {
                None
            };
            let children: Vec<_> = e
                .variants
                .iter()
                .map(|v| {
                    let variant_detail = if let Some(doc) = &v.doc {
                        Some(format_doc(doc))
                    } else {
                        None
                    };
                    DocumentSymbol {
                        name: v.name.clone(),
                        detail: variant_detail,
                        kind: SymbolKind::ENUM_MEMBER,
                        range: span_to_range(v.span),
                        selection_range: span_to_range(v.span),
                        children: None,
                        tags: None,
                deprecated: None,
                            }
                })
                .collect();
            Some(DocumentSymbol {
                name: e.name.clone(),
                detail,
                kind: SymbolKind::ENUM,
                range: span_to_range(e.span),
                selection_range: span_to_range(e.span),
                children: Some(children),
                tags: None,
                deprecated: None,
            })
        }
        DeclKind::Interface(i) => {
            let detail = if let Some(doc) = &i.doc {
                Some(format_doc(doc))
            } else {
                None
            };
            Some(DocumentSymbol {
                name: i.name.clone(),
                detail,
                kind: SymbolKind::INTERFACE,
                range: span_to_range(i.span),
                selection_range: span_to_range(i.span),
                children: None,
                tags: None,
                deprecated: None,
            })
        }
        DeclKind::Immutable(u) => {
            let fields: Vec<String> = u.fields.iter()
                .map(|f| format!("{}: {}", f.name, type_str(&f.param_type)))
                .collect();
            let children: Vec<_> = u.fields.iter().map(|f| {
                DocumentSymbol {
                    name: f.name.clone(),
                    detail: Some(type_str(&f.param_type)),
                    kind: SymbolKind::FIELD,
                    range: span_to_range(f.span),
                    selection_range: span_to_range(f.span),
                    children: None,
                    tags: None,
                deprecated: None,
                    }
            }).collect();
            Some(DocumentSymbol {
                name: u.name.clone(),
                detail: Some(format!("immutable {}({})", u.name, fields.join(", "))),
                kind: SymbolKind::STRUCT,
                range: span_to_range(u.span),
                selection_range: span_to_range(u.span),
                children: Some(children),
                tags: None,
                deprecated: None,
            })
        }
        _ => None,
    }
}
