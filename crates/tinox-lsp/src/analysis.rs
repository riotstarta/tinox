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
                return Some(fn_signature(f));
            }
            hover_stmt(&f.body, offset)
        }
        DeclKind::Class(c) => {
            for m in &c.methods {
                if let Some(s) = hover_stmt(&m.body, offset) {
                    return Some(s);
                }
                if span_contains(m.span, offset) {
                    return Some(method_signature(&c.name, m));
                }
            }
            None
        }
        _ => None,
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
        Type::Unit => "Unit".into(),
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
                items.push(CompletionItem {
                    label: f.name.clone(),
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail: Some(fn_signature(f)),
                    ..Default::default()
                });
            }
            DeclKind::Class(c) => {
                items.push(CompletionItem {
                    label: c.name.clone(),
                    kind: Some(CompletionItemKind::CLASS),
                    detail: Some(format!("class {}", c.name)),
                    ..Default::default()
                });
            }
            DeclKind::Enum(e) => {
                items.push(CompletionItem {
                    label: e.name.clone(),
                    kind: Some(CompletionItemKind::ENUM),
                    ..Default::default()
                });
                for v in &e.variants {
                    items.push(CompletionItem {
                        label: format!("{}.{}", e.name, v.name),
                        kind: Some(CompletionItemKind::ENUM_MEMBER),
                        ..Default::default()
                    });
                }
            }
            DeclKind::Interface(i) => {
                items.push(CompletionItem {
                    label: i.name.clone(),
                    kind: Some(CompletionItemKind::INTERFACE),
                    detail: Some(format!("interface {}", i.name)),
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

fn decl_to_symbol(decl: &Decl) -> Option<tower_lsp::lsp_types::DocumentSymbol> {
    use tower_lsp::lsp_types::{DocumentSymbol, SymbolKind};
    match &decl.node {
        DeclKind::Function(f) => Some(DocumentSymbol {
            name: f.name.clone(),
            detail: Some(fn_signature(f)),
            kind: SymbolKind::FUNCTION,
            range: span_to_range(f.span),
            selection_range: span_to_range(f.span),
            children: None,
            tags: None,
            deprecated: None,
        }),
        DeclKind::Class(c) => {
            let children: Vec<_> = c
                .methods
                .iter()
                .map(|m| DocumentSymbol {
                    name: m.name.clone(),
                    detail: Some(method_signature(&c.name, m)),
                    kind: SymbolKind::METHOD,
                    range: span_to_range(m.span),
                    selection_range: span_to_range(m.span),
                    children: None,
                    tags: None,
                    deprecated: None,
                })
                .collect();
            Some(DocumentSymbol {
                name: c.name.clone(),
                detail: None,
                kind: SymbolKind::CLASS,
                range: span_to_range(c.span),
                selection_range: span_to_range(c.span),
                children: Some(children),
                tags: None,
                deprecated: None,
            })
        }
        DeclKind::Enum(e) => {
            let children: Vec<_> = e
                .variants
                .iter()
                .map(|v| DocumentSymbol {
                    name: v.name.clone(),
                    detail: None,
                    kind: SymbolKind::ENUM_MEMBER,
                    range: span_to_range(v.span),
                    selection_range: span_to_range(v.span),
                    children: None,
                    tags: None,
                    deprecated: None,
                })
                .collect();
            Some(DocumentSymbol {
                name: e.name.clone(),
                detail: None,
                kind: SymbolKind::ENUM,
                range: span_to_range(e.span),
                selection_range: span_to_range(e.span),
                children: Some(children),
                tags: None,
                deprecated: None,
            })
        }
        DeclKind::Interface(i) => Some(DocumentSymbol {
            name: i.name.clone(),
            detail: None,
            kind: SymbolKind::INTERFACE,
            range: span_to_range(i.span),
            selection_range: span_to_range(i.span),
            children: None,
            tags: None,
            deprecated: None,
        }),
        _ => None,
    }
}
