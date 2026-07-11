//! Vergibt eindeutige NodeIds an alle Expressions (TESTPLAN Phase 4).
//!
//! Läuft einmal nach dem Import-Resolve über den fertigen AST. Die IDs
//! keyen die Typ-Tabelle des Typechecks (`expr_types`), die der Codegen
//! konsumiert — Spans taugen nicht als Key, weil desugarte Knoten den
//! Span ihres Ursprungs teilen. ID 0 bleibt "nicht vergeben"
//! (synthetische Knoten, ungenummerte Pfade); jede hier vergessene
//! Stelle degradiert also nur zu "kein Tabelleneintrag", nie zu einem
//! falschen Typ.

use crate::ast::*;

pub fn assign_node_ids(sf: &mut SourceFile) -> u32 {
    let mut n = Numberer { next: 1 };
    for d in &mut sf.decls {
        n.decl(d);
    }
    n.next
}

struct Numberer {
    next: u32,
}

impl Numberer {
    fn decl(&mut self, d: &mut Decl) {
        match &mut d.node {
            DeclKind::Function(f) => self.stmt(&mut f.body),
            DeclKind::Class(c) => {
                for m in &mut c.methods {
                    self.stmt(&mut m.body);
                }
            }
            DeclKind::Interface(i) => {
                for f in &mut i.methods {
                    self.stmt(&mut f.body);
                }
            }
            DeclKind::Trait(t) => {
                for f in &mut t.methods {
                    self.stmt(&mut f.body);
                }
            }
            DeclKind::Namespace(ns) => {
                for d in &mut ns.decls {
                    self.decl(d);
                }
            }
            DeclKind::Enum(_)
            | DeclKind::Import(_)
            | DeclKind::Module(_)
            | DeclKind::Immutable(_) => {}
        }
    }

    fn stmt(&mut self, s: &mut Stmt) {
        match &mut s.node {
            StmtKind::Expr(e) | StmtKind::Throw(e) => self.expr(e),
            StmtKind::Let { value, .. } | StmtKind::Var { value, .. } => {
                if let Some(e) = value {
                    self.expr(e);
                }
            }
            StmtKind::Assignment { target, value } => {
                self.expr(target);
                self.expr(value);
            }
            StmtKind::If { cond, then_branch, else_branch } => {
                self.expr(cond);
                self.stmt(then_branch);
                if let Some(e) = else_branch {
                    self.stmt(e);
                }
            }
            StmtKind::While { cond, body } => {
                self.expr(cond);
                self.stmt(body);
            }
            StmtKind::For { iter, body, .. } => {
                self.expr(iter);
                self.stmt(body);
            }
            StmtKind::ForC { init, cond, update, body } => {
                if let Some(s) = init {
                    self.stmt(s);
                }
                if let Some(e) = cond {
                    self.expr(e);
                }
                if let Some(e) = update {
                    self.expr(e);
                }
                self.stmt(body);
            }
            StmtKind::Loop { body } | StmtKind::Defer(body) => self.stmt(body),
            StmtKind::Return(e) => {
                if let Some(e) = e {
                    self.expr(e);
                }
            }
            StmtKind::Try { body, catches, finally } => {
                self.stmt(body);
                for c in catches {
                    self.stmt(&mut c.body);
                }
                if let Some(f) = finally {
                    self.stmt(f);
                }
            }
            StmtKind::Block(stmts) => {
                for s in stmts {
                    self.stmt(s);
                }
            }
            StmtKind::Select { arms, default } => {
                for a in arms {
                    self.expr(&mut a.channel);
                    self.stmt(&mut a.body);
                }
                if let Some(d) = default {
                    self.stmt(d);
                }
            }
            StmtKind::Break | StmtKind::Continue | StmtKind::Empty => {}
        }
    }

    fn exprs(&mut self, es: &mut [Expr]) {
        for e in es {
            self.expr(e);
        }
    }

    fn expr(&mut self, e: &mut Expr) {
        e.id = self.next;
        self.next += 1;
        match &mut e.node {
            ExprKind::Literal(_)
            | ExprKind::Ident(_)
            | ExprKind::This
            | ExprKind::Break
            | ExprKind::Continue
            | ExprKind::Channel => {}
            ExprKind::ArrayLiteral(es) | ExprKind::Tuple(es) => self.exprs(es),
            ExprKind::MapLiteral(entries) => {
                for (k, v) in entries {
                    self.expr(k);
                    self.expr(v);
                }
            }
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Unary { operand, .. } => self.expr(operand),
            ExprKind::Call { func, args } => {
                self.expr(func);
                self.exprs(args);
            }
            ExprKind::MethodCall { obj, args, .. } => {
                self.expr(obj);
                self.exprs(args);
            }
            ExprKind::Index { obj, index } => {
                self.expr(obj);
                self.expr(index);
            }
            ExprKind::FieldAccess { obj, .. } => self.expr(obj),
            ExprKind::SuperCall { args, .. }
            | ExprKind::New { args, .. }
            | ExprKind::EnumValue { args, .. } => self.exprs(args),
            ExprKind::StructLiteral { fields, .. } => {
                for (_, e) in fields {
                    self.expr(e);
                }
            }
            ExprKind::Block(stmts) => {
                for s in stmts {
                    self.stmt(s);
                }
            }
            ExprKind::If { cond, then_branch, else_branch } => {
                self.expr(cond);
                self.expr(then_branch);
                if let Some(e) = else_branch {
                    self.expr(e);
                }
            }
            ExprKind::While { cond, body } => {
                self.expr(cond);
                self.expr(body);
            }
            ExprKind::For { iter, body, .. } => {
                self.expr(iter);
                self.expr(body);
            }
            ExprKind::Loop { body } => self.expr(body),
            ExprKind::Match { expr, cases } => {
                self.expr(expr);
                for c in cases {
                    if let Some(g) = &mut c.guard {
                        self.expr(g);
                    }
                    self.expr(&mut c.body);
                }
            }
            ExprKind::Return(e) => {
                if let Some(e) = e {
                    self.expr(e);
                }
            }
            ExprKind::Throw(e)
            | ExprKind::Spawn(e)
            | ExprKind::Await(e)
            | ExprKind::Recv(e) => self.expr(e),
            ExprKind::Try { body, catches, finally } => {
                self.expr(body);
                for c in catches {
                    self.stmt(&mut c.body);
                }
                if let Some(f) = finally {
                    self.expr(f);
                }
            }
            ExprKind::Assign { target, value }
            | ExprKind::CompoundAssign { target, value, .. }
            | ExprKind::Send { channel: target, value } => {
                self.expr(target);
                self.expr(value);
            }
            ExprKind::Lambda { body, .. } => self.expr(body),
            ExprKind::Cast { expr, .. } | ExprKind::Is { expr, .. } => self.expr(expr),
            ExprKind::Range { start, end, .. } => {
                self.expr(start);
                self.expr(end);
            }
            ExprKind::TupleIndex { tuple, .. } => self.expr(tuple),
        }
    }
}
