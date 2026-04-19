use tinox_common::{Error, ErrorBag, Pos, Span, Spanned};
use tinox_lexer::{Keyword, Lexer, Token, TokenKind};

use crate::ast::*;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    errors: Vec<Error>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            errors: Vec::new(),
        }
    }

    pub fn parse(&mut self) -> Result<SourceFile, ErrorBag> {
        let mut decls = Vec::new();

        while !self.is_at_end() {
            match self.parse_decl() {
                Ok(decl) => decls.push(decl),
                Err(e) => {
                    self.errors.push(e);
                    self.synchronize();
                }
            }
        }

        if self.errors.is_empty() {
            Ok(SourceFile::new(decls))
        } else {
            Err(ErrorBag {
                errors: self.errors.clone(),
            })
        }
    }

    fn parse_decl(&mut self) -> Result<Decl, Error> {
        let start = self.mk_span();

        let decl = if self.consume_keyword(Keyword::Async) {
            if self.check_keyword(Keyword::Fn) {
                let mut f = self.parse_fn()?;
                f.is_async = true;
                DeclKind::Function(f)
            } else {
                return Err(self.error("expected 'fn' after 'async'"));
            }
        } else if self.check_keyword(Keyword::Fn) {
            DeclKind::Function(self.parse_fn()?)
        } else if self.check_keyword(Keyword::Class) {
            DeclKind::Class(self.parse_class()?)
        } else if self.check_keyword(Keyword::Interface) {
            DeclKind::Interface(self.parse_interface()?)
        } else if self.check_keyword(Keyword::Enum) {
            DeclKind::Enum(self.parse_enum()?)
        } else if self.check_keyword(Keyword::Trait) {
            DeclKind::Trait(self.parse_trait()?)
        } else if self.check_keyword(Keyword::Import) {
            DeclKind::Import(self.parse_import()?)
        } else if self.check_keyword(Keyword::Module) {
            self.bump();
            let name = self.parse_ident()?;
            self.consume(TokenKind::Semicolon);
            DeclKind::Module(name)
        } else if self.check_keyword(Keyword::Extern) {
            self.parse_extern_fn()?
        } else {
            return Err(self.error("expected declaration"));
        };

        Ok(Spanned::new(decl, start))
    }

    fn parse_extern_fn(&mut self) -> Result<DeclKind, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::Extern)?;
        self.expect_keyword(Keyword::Fn)?;
        let name = self.parse_ident()?;
        self.expect(TokenKind::LParen)?;

        let mut params = Vec::new();
        if !self.check(TokenKind::RParen) {
            params.push(self.parse_param()?);
            while self.consume(TokenKind::Comma) {
                params.push(self.parse_param()?);
            }
        }

        self.expect(TokenKind::RParen)?;
        let ret_type = if self.consume(TokenKind::ThinArrow) {
            self.parse_type()?
        } else {
            Type::Unit
        };

        self.expect(TokenKind::Semicolon)?;

        Ok(DeclKind::Function(Function {
            name,
            type_params: vec![],
            params,
            ret_type,
            body: Spanned::new(StmtKind::Empty, span),
            span,
            is_async: false,
        }))
    }

    fn parse_type_params(&mut self) -> Result<Vec<String>, Error> {
        self.expect(TokenKind::Less)?;
        let mut params = vec![self.parse_ident()?];
        while self.consume(TokenKind::Comma) {
            params.push(self.parse_ident()?);
        }
        self.expect(TokenKind::Greater)?;
        Ok(params)
    }

    fn parse_fn(&mut self) -> Result<Function, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::Fn)?;
        let name = self.parse_ident()?;
        let type_params = if self.check(TokenKind::Less) {
            self.parse_type_params()?
        } else {
            vec![]
        };
        self.expect(TokenKind::LParen)?;

        let mut params = Vec::new();
        if !self.check(TokenKind::RParen) {
            params.push(self.parse_param()?);
            while self.consume(TokenKind::Comma) {
                params.push(self.parse_param()?);
            }
        }

        self.expect(TokenKind::RParen)?;
        let ret_type = if self.consume(TokenKind::ThinArrow) {
            self.parse_type()?
        } else {
            Type::Unit
        };

        let body = self.parse_block()?;

        Ok(Function {
            name,
            type_params,
            params,
            ret_type,
            body,
            span,
            is_async: false,
        })
    }

    fn parse_param(&mut self) -> Result<Param, Error> {
        let span = self.mk_span();
        let name = self.parse_ident()?;
        self.expect(TokenKind::Colon)?;
        let param_type = self.parse_type()?;
        Ok(Param {
            name,
            param_type,
            span,
        })
    }

    fn parse_class(&mut self) -> Result<Class, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::Class)?;
        let name = self.parse_ident()?;
        let type_params = if self.check(TokenKind::Less) {
            self.parse_type_params()?
        } else {
            vec![]
        };

        let extends = if self.consume_keyword(Keyword::Extends) {
            Some(self.parse_ident()?)
        } else {
            None
        };

        let implements = if self.consume_keyword(Keyword::Implements) {
            let mut interfaces = vec![self.parse_ident()?];
            while self.consume(TokenKind::Comma) {
                interfaces.push(self.parse_ident()?);
            }
            interfaces
        } else {
            Vec::new()
        };

        self.expect(TokenKind::LBrace)?;

        let mut fields = Vec::new();
        let mut methods = Vec::new();

        while !self.check(TokenKind::RBrace) {
            let vis = self.parse_visibility();
            let mutable = self.consume_keyword(Keyword::Mut);
            let is_async = self.consume_keyword(Keyword::Async);

            if self.check_keyword(Keyword::Fn) {
                let mut m = self.parse_method(vis, false)?;
                m.is_async = is_async;
                methods.push(m);
            } else {
                fields.push(self.parse_field(vis, mutable)?);
            }
        }

        self.expect(TokenKind::RBrace)?;

        Ok(Class {
            name,
            type_params,
            extends,
            implements,
            fields,
            methods,
            span,
        })
    }

    fn parse_method(&mut self, visibility: Visibility, static_: bool) -> Result<Method, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::Fn)?;
        let name = self.parse_ident()?;
        self.expect(TokenKind::LParen)?;

        let mut params = Vec::new();
        if !self.check(TokenKind::RParen) {
            params.push(self.parse_param()?);
            while self.consume(TokenKind::Comma) {
                params.push(self.parse_param()?);
            }
        }

        self.expect(TokenKind::RParen)?;
        let ret_type = if self.consume(TokenKind::ThinArrow) {
            self.parse_type()?
        } else {
            Type::Unit
        };

        let body = self.parse_block()?;

        Ok(Method {
            name,
            params,
            ret_type,
            body,
            static_,
            visibility,
            span,
            is_async: false,
        })
    }

    fn parse_field(&mut self, visibility: Visibility, mutable: bool) -> Result<FieldDef, Error> {
        let span = self.mk_span();
        let name = self.parse_ident()?;
        self.expect(TokenKind::Colon)?;
        let field_type = self.parse_type()?;
        self.expect(TokenKind::Semicolon)?;

        Ok(FieldDef {
            name,
            field_type,
            visibility,
            mutable,
            span,
        })
    }

    fn parse_visibility(&mut self) -> Visibility {
        if self.consume_keyword(Keyword::Public) {
            Visibility::Public
        } else if self.consume_keyword(Keyword::Private) {
            Visibility::Private
        } else if self.consume_keyword(Keyword::Protected) {
            Visibility::Protected
        } else {
            Visibility::Package
        }
    }

    fn parse_interface(&mut self) -> Result<Interface, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::Interface)?;
        let name = self.parse_ident()?;

        let extends = if self.consume_keyword(Keyword::Extends) {
            let mut interfaces = vec![self.parse_ident()?];
            while self.consume(TokenKind::Comma) {
                interfaces.push(self.parse_ident()?);
            }
            interfaces
        } else {
            Vec::new()
        };

        self.expect(TokenKind::LBrace)?;

        let mut methods = Vec::new();
        while !self.check(TokenKind::RBrace) {
            methods.push(self.parse_fn()?);
        }

        self.expect(TokenKind::RBrace)?;

        Ok(Interface {
            name,
            extends,
            methods,
            span,
        })
    }

    fn parse_enum(&mut self) -> Result<Enum, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::Enum)?;
        let name = self.parse_ident()?;
        self.expect(TokenKind::LBrace)?;

        let mut variants = Vec::new();
        while !self.check(TokenKind::RBrace) {
            variants.push(self.parse_enum_variant()?);
            if !self.check(TokenKind::RBrace) {
                if !self.consume(TokenKind::Comma) {
                    return Err(self.error("expected ','"));
                }
            }
        }

        self.expect(TokenKind::RBrace)?;

        Ok(Enum {
            name,
            variants,
            span,
        })
    }

    fn parse_enum_variant(&mut self) -> Result<EnumVariant, Error> {
        let span = self.mk_span();
        let name = self.parse_ident()?;

        let mut args = Vec::new();
        if self.consume(TokenKind::LParen) {
            if !self.check(TokenKind::RParen) {
                args.push(self.parse_type()?);
                while self.consume(TokenKind::Comma) {
                    args.push(self.parse_type()?);
                }
            }
            self.expect(TokenKind::RParen)?;
        }

        Ok(EnumVariant { name, args, span })
    }

    fn parse_trait(&mut self) -> Result<Trait, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::Trait)?;
        let name = self.parse_ident()?;
        self.expect(TokenKind::LBrace)?;

        let mut methods = Vec::new();
        while !self.check(TokenKind::RBrace) {
            methods.push(self.parse_fn()?);
        }

        self.expect(TokenKind::RBrace)?;

        Ok(Trait {
            name,
            methods,
            span,
        })
    }

    fn parse_import(&mut self) -> Result<Import, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::Import)?;
        let mut path = vec![self.parse_ident()?];
        while self.consume(TokenKind::Dot) {
            path.push(self.parse_ident()?);
        }

        let alias = if self.consume_keyword(Keyword::As) {
            Some(self.parse_ident()?)
        } else {
            None
        };

        self.expect(TokenKind::Semicolon)?;

        Ok(Import { path, alias, span })
    }

    fn parse_type(&mut self) -> Result<Type, Error> {
        if self.check(TokenKind::Star) {
            self.bump();
            let inner = self.parse_type()?;
            return Ok(Type::Ref(Box::new(inner)));
        }

        if self.check(TokenKind::Question) {
            self.bump();
            let inner = self.parse_type()?;
            return Ok(Type::Ref(Box::new(inner)));
        }

        if self.consume_keyword(Keyword::Mut) {
            let inner = self.parse_type()?;
            return Ok(Type::Mutable(Box::new(inner)));
        }

        let mut base = self.parse_type_base()?;

        while self.check(TokenKind::LBracket) {
            self.bump();
            while !self.check(TokenKind::RBracket) {
                self.bump();
            }
            self.bump();
            base = Type::Array(Box::new(base));
        }

        if self.check(TokenKind::Star) {
            self.bump();
            base = Type::Ref(Box::new(base));
        }

        if self.check(TokenKind::LParen) {
            self.bump();
            let mut params = vec![base];
            if !self.check(TokenKind::RParen) {
                params.push(self.parse_type()?);
                while self.consume(TokenKind::Comma) {
                    params.push(self.parse_type()?);
                }
            }
            self.expect(TokenKind::RParen)?;
            self.expect(TokenKind::ThinArrow)?;
            let ret = Box::new(self.parse_type()?);
            return Ok(Type::Fn { params, ret });
        }

        Ok(base)
    }

    fn parse_type_base(&mut self) -> Result<Type, Error> {
        let ident = self.parse_ident()?;

        match ident.as_str() {
            "Int8" => Ok(Type::Int8),
            "Int16" => Ok(Type::Int16),
            "Int32" => Ok(Type::Int32),
            "Int64" => Ok(Type::Int64),
            "UInt8" => Ok(Type::UInt8),
            "UInt16" => Ok(Type::UInt16),
            "UInt32" => Ok(Type::UInt32),
            "UInt64" => Ok(Type::UInt64),
            "Float32" => Ok(Type::Float32),
            "Float64" => Ok(Type::Float64),
            "Bool" => Ok(Type::Bool),
            "Char" => Ok(Type::Char),
            "String" => Ok(Type::String),
            "Unit" => Ok(Type::Unit),
            "Never" => Ok(Type::Never),
            "Any" => Ok(Type::Any),
            s => {
                // Check for generic type arguments: Box<Int64>
                if self.check(TokenKind::Less) {
                    self.bump();
                    let mut args = vec![self.parse_type()?];
                    while self.consume(TokenKind::Comma) {
                        args.push(self.parse_type()?);
                    }
                    self.expect(TokenKind::Greater)?;
                    Ok(Type::Generic { name: s.to_string(), args })
                } else {
                    Ok(Type::Named(s.to_string()))
                }
            }
        }
    }

    fn parse_block(&mut self) -> Result<Stmt, Error> {
        let span = self.mk_span();
        self.expect(TokenKind::LBrace)?;
        let stmts = self.parse_block_stmts();
        self.expect(TokenKind::RBrace)?;
        Ok(Spanned::new(StmtKind::Block(stmts), span))
    }

    fn parse_block_stmts(&mut self) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        while !self.check(TokenKind::RBrace) && !self.is_at_end() {
            match self.parse_stmt() {
                Ok(stmt) => stmts.push(stmt),
                Err(e) => {
                    self.errors.push(e);
                    self.synchronize();
                    if self.check(TokenKind::RBrace) {
                        break;
                    }
                }
            }
        }
        stmts
    }

    fn parse_stmt(&mut self) -> Result<Stmt, Error> {
        let span = self.mk_span();

        let stmt = match self.peek().kind {
            TokenKind::Keyword(Keyword::Let) => self.parse_let_stmt()?,
            TokenKind::Keyword(Keyword::Var) => self.parse_var_stmt()?,
            TokenKind::Keyword(Keyword::If) => self.parse_if_stmt()?,
            TokenKind::Keyword(Keyword::While) => self.parse_while_stmt()?,
            TokenKind::Keyword(Keyword::For) => self.parse_for_stmt()?,
            TokenKind::Keyword(Keyword::Loop) => self.parse_loop_stmt()?,
            TokenKind::Keyword(Keyword::Return) => self.parse_return_stmt()?,
            TokenKind::Keyword(Keyword::Break) => {
                self.bump();
                self.expect(TokenKind::Semicolon)?;
                StmtKind::Break
            }
            TokenKind::Keyword(Keyword::Continue) => {
                self.bump();
                self.expect(TokenKind::Semicolon)?;
                StmtKind::Continue
            }
            TokenKind::Keyword(Keyword::Throw) => self.parse_throw_stmt()?,
            TokenKind::Keyword(Keyword::Try) => self.parse_try_stmt()?,
            TokenKind::Keyword(Keyword::Defer) => self.parse_defer_stmt()?,
            TokenKind::Keyword(Keyword::Select) => self.parse_select_stmt()?,
            TokenKind::LBrace => {
                self.bump(); // consume LBrace
                let stmts = self.parse_block_stmts();
                self.expect(TokenKind::RBrace)?;
                StmtKind::Block(stmts)
            }
            TokenKind::Semicolon => {
                self.bump();
                StmtKind::Empty
            }
            _ => {
                if let TokenKind::Ident(_) = &self.peek().kind {
                    let name = self.parse_ident()?;
                    if self.check(TokenKind::Equals) {
                        self.bump();
                        let value = self.parse_expr()?;
                        self.expect(TokenKind::Semicolon)?;
                        let target = Spanned::new(ExprKind::Ident(name.clone()), Span::dummy());
                        StmtKind::Assignment { target, value }
                    } else if self.check(TokenKind::PlusEquals)
                        || self.check(TokenKind::MinusEquals)
                        || self.check(TokenKind::StarEquals)
                        || self.check(TokenKind::SlashEquals)
                        || self.check(TokenKind::PercentEquals)
                    {
                        let op = match self.peek().kind {
                            TokenKind::PlusEquals => CompoundOp::Add,
                            TokenKind::MinusEquals => CompoundOp::Sub,
                            TokenKind::StarEquals => CompoundOp::Mul,
                            TokenKind::SlashEquals => CompoundOp::Div,
                            TokenKind::PercentEquals => CompoundOp::Mod,
                            _ => unreachable!(),
                        };
                        self.bump();
                        let value = self.parse_expr()?;
                        self.expect(TokenKind::Semicolon)?;
                        let target = Spanned::new(ExprKind::Ident(name), Span::dummy());
                        StmtKind::Expr(Spanned::new(
                            ExprKind::CompoundAssign {
                                op,
                                target: Box::new(target),
                                value: Box::new(value),
                            },
                            Span::dummy(),
                        ))
                    } else if self.check(TokenKind::LBracket) {
                        self.bump();
                        let index = self.parse_expr()?;
                        self.expect(TokenKind::RBracket)?;
                        self.expect(TokenKind::Equals)?;
                        let value = self.parse_expr()?;
                        self.expect(TokenKind::Semicolon)?;
                        let target = Spanned::new(
                            ExprKind::Index {
                                obj: Box::new(Spanned::new(ExprKind::Ident(name), Span::dummy())),
                                index: Box::new(index),
                            },
                            Span::dummy(),
                        );
                        StmtKind::Assignment { target, value }
                    } else if self.check(TokenKind::LParen) {
                        let mut args = Vec::new();
                        self.bump();
                        if !self.check(TokenKind::RParen) {
                            args.push(self.parse_expr()?);
                            while self.consume(TokenKind::Comma) {
                                args.push(self.parse_expr()?);
                            }
                        }
                        self.expect(TokenKind::RParen)?;
                        self.expect(TokenKind::Semicolon)?;
                        StmtKind::Expr(Spanned::new(
                            ExprKind::Call {
                                func: Box::new(Spanned::new(ExprKind::Ident(name), Span::dummy())),
                                args,
                            },
                            Span::dummy(),
                        ))
                    } else {
                        self.expect(TokenKind::Semicolon)?;
                        StmtKind::Expr(Spanned::new(ExprKind::Ident(name), Span::dummy()))
                    }
                } else {
                    let expr = self.parse_expr()?;
                    self.expect(TokenKind::Semicolon)?;
                    StmtKind::Expr(expr)
                }
            }
        };

        Ok(Spanned::new(stmt, span))
    }

    fn parse_let_stmt(&mut self) -> Result<StmtKind, Error> {
        self.expect_keyword(Keyword::Let)?;
        let name = self.parse_ident()?;
        let ty = if self.consume(TokenKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };
        let value = if self.consume(TokenKind::Equals) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.expect(TokenKind::Semicolon)?;
        Ok(StmtKind::Let { name, ty, value })
    }

    fn parse_var_stmt(&mut self) -> Result<StmtKind, Error> {
        self.expect_keyword(Keyword::Var)?;
        let name = self.parse_ident()?;
        let ty = if self.consume(TokenKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };
        let value = if self.consume(TokenKind::Equals) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.expect(TokenKind::Semicolon)?;
        Ok(StmtKind::Var {
            name,
            ty,
            value,
            mutable: true,
        })
    }

    fn parse_if_stmt(&mut self) -> Result<StmtKind, Error> {
        self.expect_keyword(Keyword::If)?;
        let cond = self.parse_expr()?;
        let then_branch = Box::new(self.parse_block()?);
        let else_branch = if self.consume_keyword(Keyword::Else) {
            Some(if self.check_keyword(Keyword::If) {
                Box::new(Spanned::new(self.parse_if_stmt()?, Span::dummy()))
            } else {
                Box::new(self.parse_block()?)
            })
        } else {
            None
        };
        Ok(StmtKind::If {
            cond,
            then_branch,
            else_branch,
        })
    }

    fn parse_while_stmt(&mut self) -> Result<StmtKind, Error> {
        self.expect_keyword(Keyword::While)?;
        let cond = self.parse_expr()?;
        let body = Box::new(self.parse_block()?);
        Ok(StmtKind::While { cond, body })
    }

    fn parse_for_stmt(&mut self) -> Result<StmtKind, Error> {
        self.bump(); // consume 'for'
        // for x in expr { body }
        if !self.check(TokenKind::LParen) {
            let var = self.parse_ident()?;
            self.expect_keyword(Keyword::In)?;
            let iter = self.parse_expr()?;
            let body = Box::new(self.parse_block()?);
            return Ok(StmtKind::For { var, iter, body });
        }
        self.expect(TokenKind::LParen)?;

        let init = if !self.check(TokenKind::Semicolon) {
            let stmt = if self.check_keyword(Keyword::Let) {
                self.parse_let_stmt()?
            } else if self.check_keyword(Keyword::Var) {
                self.parse_var_stmt()?
            } else {
                let expr = self.parse_expr()?;
                self.expect(TokenKind::Semicolon)?;
                StmtKind::Expr(expr)
            };
            Some(Box::new(Spanned::new(stmt, Span::dummy())))
        } else {
            self.bump();
            None
        };

        let cond = if !self.check(TokenKind::Semicolon) {
            Some(self.parse_expr()?)
        } else {
            self.bump();
            None
        };
        self.expect(TokenKind::Semicolon)?;

        let update = if !self.check(TokenKind::RParen) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.expect(TokenKind::RParen)?;

        let body = Box::new(self.parse_block()?);

        Ok(StmtKind::ForC {
            init,
            cond,
            update,
            body,
        })
    }

    fn parse_loop_stmt(&mut self) -> Result<StmtKind, Error> {
        self.expect_keyword(Keyword::Loop)?;
        let body = Box::new(self.parse_block()?);
        Ok(StmtKind::Loop { body })
    }

    fn parse_return_stmt(&mut self) -> Result<StmtKind, Error> {
        self.expect_keyword(Keyword::Return)?;
        if self.check(TokenKind::Semicolon) {
            self.bump();
            Ok(StmtKind::Return(None))
        } else {
            let value = self.parse_expr()?;
            self.expect(TokenKind::Semicolon)?;
            Ok(StmtKind::Return(Some(value)))
        }
    }

    fn parse_throw_stmt(&mut self) -> Result<StmtKind, Error> {
        self.expect_keyword(Keyword::Throw)?;
        let expr = self.parse_expr()?;
        self.expect(TokenKind::Semicolon)?;
        Ok(StmtKind::Throw(expr))
    }

    fn parse_try_stmt(&mut self) -> Result<StmtKind, Error> {
        self.expect_keyword(Keyword::Try)?;
        let body = Box::new(self.parse_block()?);

        let mut catches = Vec::new();
        while self.consume_keyword(Keyword::Catch) {
            self.expect(TokenKind::LParen)?;
            let param = self.parse_ident()?;
            self.expect(TokenKind::Colon)?;
            let ty = self.parse_type()?;
            self.expect(TokenKind::RParen)?;
            let catch_body = self.parse_block()?;
            catches.push(CatchClause {
                param,
                ty,
                body: catch_body,
                span: Span::dummy(),
            });
        }

        let finally_block = if self.consume_keyword(Keyword::Finally) {
            Some(Box::new(self.parse_block()?))
        } else {
            None
        };

        Ok(StmtKind::Try {
            body,
            catches,
            finally: finally_block,
        })
    }

    fn parse_defer_stmt(&mut self) -> Result<StmtKind, Error> {
        self.expect_keyword(Keyword::Defer)?;
        let stmt = self.parse_stmt()?;
        Ok(StmtKind::Defer(Box::new(stmt)))
    }

    fn parse_expr(&mut self) -> Result<Expr, Error> {
        self.parse_assign_expr()
    }

    fn parse_assign_expr(&mut self) -> Result<Expr, Error> {
        let lhs = self.parse_or_expr()?;

        if self.check(TokenKind::Equals) {
            self.bump();
            let rhs = self.parse_assign_expr()?;
            let span = lhs.span;
            return Ok(Spanned::new(
                ExprKind::Assign {
                    target: Box::new(lhs),
                    value: Box::new(rhs),
                },
                span,
            ));
        }

        if self.check(TokenKind::PlusEquals)
            || self.check(TokenKind::MinusEquals)
            || self.check(TokenKind::StarEquals)
            || self.check(TokenKind::SlashEquals)
            || self.check(TokenKind::PercentEquals)
        {
            let op = match self.peek().kind {
                TokenKind::PlusEquals => CompoundOp::Add,
                TokenKind::MinusEquals => CompoundOp::Sub,
                TokenKind::StarEquals => CompoundOp::Mul,
                TokenKind::SlashEquals => CompoundOp::Div,
                TokenKind::PercentEquals => CompoundOp::Mod,
                _ => unreachable!(),
            };
            self.bump();
            let rhs = self.parse_assign_expr()?;
            let span = lhs.span;
            return Ok(Spanned::new(
                ExprKind::CompoundAssign {
                    op,
                    target: Box::new(lhs),
                    value: Box::new(rhs),
                },
                span,
            ));
        }

        Ok(lhs)
    }

    fn parse_or_expr(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_and_expr()?;

        while self.check(TokenKind::BarBar) {
            self.bump();
            let rhs = self.parse_and_expr()?;
            let span = lhs.span;
            lhs = Spanned::new(
                ExprKind::Binary {
                    op: BinaryOp::Or,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            );
        }

        Ok(lhs)
    }

    fn parse_and_expr(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_bitwise_or_expr()?;

        while self.check(TokenKind::AmpAmp) {
            self.bump();
            let rhs = self.parse_bitwise_or_expr()?;
            let span = lhs.span;
            lhs = Spanned::new(
                ExprKind::Binary {
                    op: BinaryOp::And,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            );
        }

        Ok(lhs)
    }

    fn parse_bitwise_or_expr(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_bitwise_xor_expr()?;

        while self.check(TokenKind::Bar) && !self.check(TokenKind::BarBar) {
            self.bump();
            let rhs = self.parse_bitwise_xor_expr()?;
            let span = lhs.span;
            lhs = Spanned::new(
                ExprKind::Binary {
                    op: BinaryOp::BitOr,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            );
        }

        Ok(lhs)
    }

    fn parse_bitwise_xor_expr(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_bitwise_and_expr()?;

        while self.check(TokenKind::Caret) {
            self.bump();
            let rhs = self.parse_bitwise_and_expr()?;
            let span = lhs.span;
            lhs = Spanned::new(
                ExprKind::Binary {
                    op: BinaryOp::Xor,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            );
        }

        Ok(lhs)
    }

    fn parse_bitwise_and_expr(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_equality_expr()?;

        while self.check(TokenKind::Ampersand) && !self.check(TokenKind::AmpAmp) {
            self.bump();
            let rhs = self.parse_equality_expr()?;
            let span = lhs.span;
            lhs = Spanned::new(
                ExprKind::Binary {
                    op: BinaryOp::And,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            );
        }

        Ok(lhs)
    }

    fn parse_equality_expr(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_relational_expr()?;

        while self.check(TokenKind::EqualsEquals) || self.check(TokenKind::BangEquals) {
            let op = if self.check(TokenKind::EqualsEquals) {
                BinaryOp::Eq
            } else {
                BinaryOp::Ne
            };
            self.bump();
            let rhs = self.parse_relational_expr()?;
            let span = lhs.span;
            lhs = Spanned::new(
                ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            );
        }

        Ok(lhs)
    }

    fn parse_relational_expr(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_shift_expr()?;

        while self.check(TokenKind::Less)
            || self.check(TokenKind::LessEquals)
            || self.check(TokenKind::Greater)
            || self.check(TokenKind::GreaterEquals)
        {
            let op = match self.peek().kind {
                TokenKind::Less => BinaryOp::Lt,
                TokenKind::LessEquals => BinaryOp::Le,
                TokenKind::Greater => BinaryOp::Gt,
                TokenKind::GreaterEquals => BinaryOp::Ge,
                _ => unreachable!(),
            };
            self.bump();
            let rhs = self.parse_shift_expr()?;
            let span = lhs.span;
            lhs = Spanned::new(
                ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            );
        }

        Ok(lhs)
    }

    fn parse_shift_expr(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_additive_expr()?;

        while self.check(TokenKind::LessLess)
            || self.check(TokenKind::GreaterGreater)
            || self.check(TokenKind::GreaterGreaterGreater)
        {
            let op = match self.peek().kind {
                TokenKind::LessLess => BinaryOp::Shl,
                TokenKind::GreaterGreater => BinaryOp::Shr,
                TokenKind::GreaterGreaterGreater => BinaryOp::ShrArith,
                _ => unreachable!(),
            };
            self.bump();
            let rhs = self.parse_additive_expr()?;
            let span = lhs.span;
            lhs = Spanned::new(
                ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            );
        }

        Ok(lhs)
    }

    fn parse_additive_expr(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_multiplicative_expr()?;

        while self.check(TokenKind::Plus) || self.check(TokenKind::Minus) {
            let op = if self.check(TokenKind::Plus) {
                BinaryOp::Add
            } else {
                BinaryOp::Sub
            };
            self.bump();
            let rhs = self.parse_multiplicative_expr()?;
            let span = lhs.span;
            lhs = Spanned::new(
                ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            );
        }

        Ok(lhs)
    }

    fn parse_multiplicative_expr(&mut self) -> Result<Expr, Error> {
        let mut lhs = self.parse_unary_expr()?;

        while self.check(TokenKind::Star)
            || self.check(TokenKind::Slash)
            || self.check(TokenKind::Percent)
        {
            let op = match self.peek().kind {
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                TokenKind::Percent => BinaryOp::Mod,
                _ => unreachable!(),
            };
            self.bump();
            let rhs = self.parse_unary_expr()?;
            let span = lhs.span;
            lhs = Spanned::new(
                ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            );
        }

        Ok(lhs)
    }

    fn parse_unary_expr(&mut self) -> Result<Expr, Error> {
        if self.check(TokenKind::Plus) {
            self.bump();
            return self.parse_unary_expr();
        }

        if self.check(TokenKind::Minus) {
            self.bump();
            let operand = self.parse_unary_expr()?;
            return Ok(Spanned::new(
                ExprKind::Unary {
                    op: UnaryOp::Neg,
                    operand: Box::new(operand),
                },
                self.mk_span(),
            ));
        }

        if self.check(TokenKind::Bang) {
            self.bump();
            let operand = self.parse_unary_expr()?;
            return Ok(Spanned::new(
                ExprKind::Unary {
                    op: UnaryOp::Not,
                    operand: Box::new(operand),
                },
                self.mk_span(),
            ));
        }

        if self.check(TokenKind::Tilde) {
            self.bump();
            let operand = self.parse_unary_expr()?;
            return Ok(Spanned::new(
                ExprKind::Unary {
                    op: UnaryOp::BitNot,
                    operand: Box::new(operand),
                },
                self.mk_span(),
            ));
        }

        self.parse_postfix_expr()
    }

    fn parse_postfix_expr(&mut self) -> Result<Expr, Error> {
        let mut expr = self.parse_primary_expr()?;

        loop {
            if self.check(TokenKind::Dot) {
                self.bump();
                let name = self.parse_ident()?;
                if self.check(TokenKind::LParen) {
                    self.bump();
                    let mut args = Vec::new();
                    if !self.check(TokenKind::RParen) {
                        args.push(self.parse_expr()?);
                        while self.consume(TokenKind::Comma) {
                            args.push(self.parse_expr()?);
                        }
                    }
                    self.expect(TokenKind::RParen)?;
                    let span = expr.span;
                    expr = Spanned::new(
                        ExprKind::MethodCall {
                            obj: Box::new(expr),
                            method: name,
                            args,
                        },
                        span,
                    );
                } else {
                    let span = expr.span;
                    expr = Spanned::new(
                        ExprKind::FieldAccess {
                            obj: Box::new(expr),
                            field: name,
                        },
                        span,
                    );
                }
            } else if self.check(TokenKind::ColonColon) {
                // Handle enum value construction: Color::Red or Option::Some(value)
                if let ExprKind::Ident(enum_name) = &expr.node {
                    self.bump(); // consume ::
                    let variant = self.parse_ident()?;
                    let span = expr.span;

                    let mut args = Vec::new();
                    if self.check(TokenKind::LParen) {
                        self.bump();
                        if !self.check(TokenKind::RParen) {
                            args.push(self.parse_expr()?);
                            while self.consume(TokenKind::Comma) {
                                args.push(self.parse_expr()?);
                            }
                        }
                        self.expect(TokenKind::RParen)?;
                    }

                    expr = Spanned::new(
                        ExprKind::EnumValue {
                            enum_name: enum_name.clone(),
                            variant,
                            args,
                        },
                        span,
                    );
                } else {
                    return Err(self.error("expected enum name before ::"));
                }
            } else if self.check(TokenKind::LBracket) {
                self.bump();
                let index = self.parse_expr()?;
                self.expect(TokenKind::RBracket)?;
                let span = expr.span;
                expr = Spanned::new(
                    ExprKind::Index {
                        obj: Box::new(expr),
                        index: Box::new(index),
                    },
                    span,
                );
            } else if self.check(TokenKind::DotDot) || self.check(TokenKind::DotDotDot) {
                let inclusive = self.check(TokenKind::DotDotDot);
                self.bump();
                let end = self.parse_postfix_expr()?;
                let span = expr.span;
                expr = Spanned::new(
                    ExprKind::Range {
                        start: Box::new(expr),
                        end: Box::new(end),
                        inclusive,
                    },
                    span,
                );
            } else if self.check(TokenKind::LParen) {
                self.bump();
                let mut args = Vec::new();
                if !self.check(TokenKind::RParen) {
                    args.push(self.parse_expr()?);
                    while self.consume(TokenKind::Comma) {
                        args.push(self.parse_expr()?);
                    }
                }
                self.expect(TokenKind::RParen)?;
                let span = expr.span;
                expr = Spanned::new(
                    ExprKind::Call {
                        func: Box::new(expr),
                        args,
                    },
                    span,
                );
            } else {
                break;
            }
        }

        Ok(expr)
    }

    fn parse_primary_expr(&mut self) -> Result<Expr, Error> {
        let token = self.peek();

        match &token.kind {
            TokenKind::LBracket => {
                self.bump();
                let mut elements = Vec::new();
                if !self.check(TokenKind::RBracket) {
                    elements.push(self.parse_expr()?);
                    while self.consume(TokenKind::Comma) {
                        elements.push(self.parse_expr()?);
                    }
                }
                self.expect(TokenKind::RBracket)?;
                Ok(Spanned::new(ExprKind::ArrayLiteral(elements), token.span))
            }
            TokenKind::Integer(n) => {
                self.bump();
                Ok(Spanned::new(
                    ExprKind::Literal(Literal::Integer(*n)),
                    token.span,
                ))
            }
            TokenKind::Float(f) => {
                self.bump();
                Ok(Spanned::new(
                    ExprKind::Literal(Literal::Float(*f)),
                    token.span,
                ))
            }
            TokenKind::String(s) => {
                self.bump();
                Ok(Spanned::new(
                    ExprKind::Literal(Literal::String(s.clone())),
                    token.span,
                ))
            }
            TokenKind::RawString(s) => {
                self.bump();
                Ok(Spanned::new(
                    ExprKind::Literal(Literal::String(s.clone())),
                    token.span,
                ))
            }
            TokenKind::Byte(b) => {
                self.bump();
                Ok(Spanned::new(
                    ExprKind::Literal(Literal::Byte(*b)),
                    token.span,
                ))
            }
            TokenKind::Char(c) => {
                self.bump();
                Ok(Spanned::new(
                    ExprKind::Literal(Literal::Char(*c)),
                    token.span,
                ))
            }
            TokenKind::Bool(b) => {
                self.bump();
                Ok(Spanned::new(
                    ExprKind::Literal(Literal::Bool(*b)),
                    token.span,
                ))
            }
            TokenKind::Keyword(Keyword::True) => {
                self.bump();
                Ok(Spanned::new(
                    ExprKind::Literal(Literal::Bool(true)),
                    token.span,
                ))
            }
            TokenKind::Keyword(Keyword::False) => {
                self.bump();
                Ok(Spanned::new(
                    ExprKind::Literal(Literal::Bool(false)),
                    token.span,
                ))
            }
            TokenKind::Keyword(Keyword::Null) => {
                self.bump();
                Ok(Spanned::new(ExprKind::Literal(Literal::Null), token.span))
            }
            TokenKind::Keyword(Keyword::This) => {
                self.bump();
                Ok(Spanned::new(ExprKind::This, token.span))
            }
            TokenKind::Keyword(Keyword::Super) => {
                let span = token.span;
                self.bump();
                self.expect(TokenKind::Dot)?;
                let method = self.parse_ident()?;
                self.expect(TokenKind::LParen)?;
                let mut args = Vec::new();
                if !self.check(TokenKind::RParen) {
                    args.push(self.parse_expr()?);
                    while self.consume(TokenKind::Comma) {
                        args.push(self.parse_expr()?);
                    }
                }
                self.expect(TokenKind::RParen)?;
                Ok(Spanned::new(ExprKind::SuperCall { method, args }, span))
            }
            TokenKind::Keyword(Keyword::If) => self.parse_if_expr(),
            TokenKind::Keyword(Keyword::While) => self.parse_while_expr(),
            TokenKind::Keyword(Keyword::Loop) => self.parse_loop_expr(),
            TokenKind::Keyword(Keyword::Match) => self.parse_match_expr(),
            TokenKind::Keyword(Keyword::New) => self.parse_new_expr(),
            TokenKind::Keyword(Keyword::Spawn) => self.parse_spawn_expr(),
            TokenKind::Keyword(Keyword::Await) => self.parse_await_expr(),
            TokenKind::Keyword(Keyword::Send) => self.parse_send_expr(),
            TokenKind::Keyword(Keyword::Recv) => self.parse_recv_expr(),
            TokenKind::Keyword(Keyword::Return) => {
                let span = token.span;
                self.bump();
                let value = if self.check(TokenKind::Semicolon)
                    || self.check(TokenKind::RBrace)
                    || self.check(TokenKind::RParen)
                {
                    None
                } else {
                    Some(Box::new(self.parse_expr()?))
                };
                Ok(Spanned::new(ExprKind::Return(value), span))
            }
            TokenKind::Keyword(Keyword::Break) => {
                let span = token.span;
                self.bump();
                Ok(Spanned::new(ExprKind::Break, span))
            }
            TokenKind::Keyword(Keyword::Continue) => {
                let span = token.span;
                self.bump();
                Ok(Spanned::new(ExprKind::Continue, span))
            }
            TokenKind::Keyword(Keyword::Cast) => self.parse_cast_expr(),
            TokenKind::Keyword(Keyword::Is) => self.parse_is_expr(),
            TokenKind::Keyword(Keyword::Channel) => {
                self.bump();
                Ok(Spanned::new(ExprKind::Channel, token.span))
            }
            TokenKind::Backslash => self.parse_lambda(),
            TokenKind::LParen => self.parse_tuple_or_grouped(),
            TokenKind::LBrace => {
                let block_stmt = self.parse_block()?;
                let span = block_stmt.span;
                let stmts = if let StmtKind::Block(stmts) = block_stmt.node {
                    stmts
                } else {
                    unreachable!()
                };
                Ok(Spanned::new(ExprKind::Block(stmts), span))
            }
            TokenKind::Ident(s) => {
                self.bump();
                if self.check(TokenKind::LBrace) {
                    // Lookahead to check if this is really a struct literal
                    // Struct literals have Ident { field: value, ... }
                    // We check if the next token after { is either } or an Ident followed by :
                    let is_struct_literal = {
                        let saved_pos = self.pos;
                        self.bump(); // consume {
                        let result = if self.check(TokenKind::RBrace) {
                            true // empty struct literal
                        } else if let TokenKind::Ident(_) = self.peek().kind {
                            self.bump(); // consume potential field name
                            self.check(TokenKind::Colon) // check if followed by :
                        } else {
                            false // not a struct literal
                        };
                        self.pos = saved_pos; // restore position
                        result
                    };

                    if is_struct_literal {
                        self.bump(); // consume {
                        let mut fields = Vec::new();
                        while !self.check(TokenKind::RBrace) {
                            let name = self.parse_ident()?;
                            self.expect(TokenKind::Colon)?;
                            let value = self.parse_expr()?;
                            fields.push((name, value));
                            if !self.check(TokenKind::RBrace) {
                                self.expect(TokenKind::Comma)?;
                            }
                        }
                        self.expect(TokenKind::RBrace)?;
                        Ok(Spanned::new(
                            ExprKind::StructLiteral {
                                name: s.clone(),
                                fields,
                            },
                            token.span,
                        ))
                    } else {
                        Ok(Spanned::new(ExprKind::Ident(s.clone()), token.span))
                    }
                } else {
                    Ok(Spanned::new(ExprKind::Ident(s.clone()), token.span))
                }
            }
            _ => Err(Error::new(
                token.span,
                format!("unexpected token: {:?}", token.kind),
            )),
        }
    }

    fn parse_if_expr(&mut self) -> Result<Expr, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::If)?;
        let cond = self.parse_expr()?;
        let then_branch = Box::new(self.parse_expr()?);
        let else_branch = if self.consume_keyword(Keyword::Else) {
            Some(Box::new(self.parse_expr()?))
        } else {
            None
        };
        Ok(Spanned::new(
            ExprKind::If {
                cond: Box::new(cond),
                then_branch,
                else_branch,
            },
            span,
        ))
    }

    fn parse_while_expr(&mut self) -> Result<Expr, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::While)?;
        let cond = self.parse_expr()?;
        let body = Box::new(self.parse_expr()?);
        Ok(Spanned::new(
            ExprKind::While {
                cond: Box::new(cond),
                body,
            },
            span,
        ))
    }

    fn parse_loop_expr(&mut self) -> Result<Expr, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::Loop)?;
        let body = Box::new(self.parse_expr()?);
        Ok(Spanned::new(ExprKind::Loop { body }, span))
    }

    fn parse_match_expr(&mut self) -> Result<Expr, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::Match)?;
        let expr = Box::new(self.parse_expr()?);
        self.expect(TokenKind::LBrace)?;

        let mut cases = Vec::new();
        while !self.check(TokenKind::RBrace) {
            let pattern_span = self.mk_span();
            let pattern = self.parse_pattern()?;

            let guard = if self.consume_keyword(Keyword::If) {
                Some(self.parse_expr()?)
            } else {
                None
            };

            self.expect(TokenKind::FatArrow)?;
            let body = self.parse_expr()?;

            cases.push(MatchCase {
                pattern,
                guard,
                body,
                span: pattern_span,
            });

            if !self.check(TokenKind::RBrace) {
                if !self.consume(TokenKind::Comma) {
                    return Err(self.error("expected ','"));
                }
            }
        }

        self.expect(TokenKind::RBrace)?;

        Ok(Spanned::new(ExprKind::Match { expr, cases }, span))
    }

    fn parse_pattern(&mut self) -> Result<Pattern, Error> {
        let span = self.mk_span();

        if self.check(TokenKind::Underscore) {
            self.bump();
            return Ok(Pattern::Wildcard(span));
        }

        if self.check(TokenKind::LParen) {
            self.bump();
            let mut patterns = Vec::new();
            if !self.check(TokenKind::RParen) {
                patterns.push(self.parse_pattern()?);
                while self.consume(TokenKind::Comma) {
                    patterns.push(self.parse_pattern()?);
                }
            }
            self.expect(TokenKind::RParen)?;
            return Ok(Pattern::Tuple(patterns, span));
        }

        // Try to parse a literal
        match &self.peek().kind {
            TokenKind::Integer(n) => {
                let n = *n;
                self.bump();
                return Ok(Pattern::Literal(Literal::Integer(n), span));
            }
            TokenKind::Float(f) => {
                let f = *f;
                self.bump();
                return Ok(Pattern::Literal(Literal::Float(f), span));
            }
            TokenKind::String(s) => {
                let s = s.clone();
                self.bump();
                return Ok(Pattern::Literal(Literal::String(s), span));
            }
            TokenKind::Char(c) => {
                let c = *c;
                self.bump();
                return Ok(Pattern::Literal(Literal::Char(c), span));
            }
            TokenKind::Byte(b) => {
                let b = *b;
                self.bump();
                return Ok(Pattern::Literal(Literal::Byte(b), span));
            }
            TokenKind::Keyword(Keyword::True) => {
                self.bump();
                return Ok(Pattern::Literal(Literal::Bool(true), span));
            }
            TokenKind::Keyword(Keyword::False) => {
                self.bump();
                return Ok(Pattern::Literal(Literal::Bool(false), span));
            }
            _ => {}
        }

        let name = self.parse_ident()?;

        // Handle enum variant patterns: Color::Red or Option::Some(x)
        if self.check(TokenKind::ColonColon) {
            self.bump();
            let variant = self.parse_ident()?;
            let mut args = Vec::new();
            if self.check(TokenKind::LParen) {
                self.bump();
                if !self.check(TokenKind::RParen) {
                    args.push(self.parse_pattern()?);
                    while self.consume(TokenKind::Comma) {
                        args.push(self.parse_pattern()?);
                    }
                }
                self.expect(TokenKind::RParen)?;
            }
            return Ok(Pattern::EnumVariant {
                enum_name: name,
                variant,
                args,
                span,
            });
        }

        if self.check(TokenKind::LParen) {
            self.bump();
            let mut args = Vec::new();
            if !self.check(TokenKind::RParen) {
                args.push(self.parse_pattern()?);
                while self.consume(TokenKind::Comma) {
                    args.push(self.parse_pattern()?);
                }
            }
            self.expect(TokenKind::RParen)?;
            return Ok(Pattern::EnumVariant {
                enum_name: name,
                variant: String::new(),
                args,
                span,
            });
        }

        Ok(Pattern::Ident(name, None, span))
    }

    fn parse_new_expr(&mut self) -> Result<Expr, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::New)?;
        let class = self.parse_ident()?;
        let type_args = if self.consume(TokenKind::Less) {
            let mut targs = vec![self.parse_type()?];
            while self.consume(TokenKind::Comma) {
                targs.push(self.parse_type()?);
            }
            self.expect(TokenKind::Greater)?;
            targs
        } else {
            vec![]
        };
        self.expect(TokenKind::LParen)?;
        let mut args = Vec::new();
        if !self.check(TokenKind::RParen) {
            args.push(self.parse_expr()?);
            while self.consume(TokenKind::Comma) {
                args.push(self.parse_expr()?);
            }
        }
        self.expect(TokenKind::RParen)?;
        Ok(Spanned::new(ExprKind::New { class, type_args, args }, span))
    }

    fn parse_spawn_expr(&mut self) -> Result<Expr, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::Spawn)?;
        let expr = Box::new(self.parse_expr()?);
        Ok(Spanned::new(ExprKind::Spawn(expr), span))
    }

    fn parse_await_expr(&mut self) -> Result<Expr, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::Await)?;
        let expr = Box::new(self.parse_expr()?);
        Ok(Spanned::new(ExprKind::Await(expr), span))
    }

    fn parse_send_expr(&mut self) -> Result<Expr, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::Send)?;
        let channel = Box::new(self.parse_expr()?);
        self.expect(TokenKind::ThinArrow)?;
        let value = Box::new(self.parse_expr()?);
        Ok(Spanned::new(ExprKind::Send { channel, value }, span))
    }

    fn parse_recv_expr(&mut self) -> Result<Expr, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::Recv)?;
        let channel = Box::new(self.parse_expr()?);
        Ok(Spanned::new(ExprKind::Recv(channel), span))
    }

    /// select { recv ch -> v { body } ... default { body } }
    fn parse_select_stmt(&mut self) -> Result<StmtKind, Error> {
        self.expect_keyword(Keyword::Select)?;
        self.expect(TokenKind::LBrace)?;
        let mut arms: Vec<SelectArm> = Vec::new();
        let mut default: Option<Box<Stmt>> = None;
        while !self.check(TokenKind::RBrace) && !self.is_at_end() {
            let arm_span = self.mk_span();
            if self.check_keyword(Keyword::Default) {
                self.bump();
                let body = self.parse_block()?;
                default = Some(Box::new(body));
            } else if self.check_keyword(Keyword::Recv) {
                self.bump(); // consume recv
                let channel = self.parse_expr()?;
                self.expect(TokenKind::ThinArrow)?;
                let var = self.parse_ident()?;
                let body = self.parse_block()?;
                arms.push(SelectArm { channel, var, body, span: arm_span });
            } else {
                let tok = self.peek().clone();
                return Err(Error::new(tok.span, format!("expected 'recv' or 'default' in select, got {:?}", tok.kind)));
            }
        }
        self.expect(TokenKind::RBrace)?;
        Ok(StmtKind::Select { arms, default })
    }

    fn parse_cast_expr(&mut self) -> Result<Expr, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::Cast)?;
        let expr = Box::new(self.parse_expr()?);
        self.expect_keyword(Keyword::As)?;
        let ty = self.parse_type()?;
        Ok(Spanned::new(ExprKind::Cast { expr, ty }, span))
    }

    fn parse_is_expr(&mut self) -> Result<Expr, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::Is)?;
        let expr = Box::new(self.parse_expr()?);
        self.expect(TokenKind::ThinArrow)?;
        let ty = self.parse_type()?;
        Ok(Spanned::new(ExprKind::Is { expr, ty }, span))
    }

    fn parse_lambda(&mut self) -> Result<Expr, Error> {
        let span = self.mk_span();
        self.expect(TokenKind::Backslash)?; // \

        let mut params = Vec::new();
        if !self.check(TokenKind::ThinArrow) {
            params.push(self.parse_lambda_param()?);
            while self.consume(TokenKind::Comma) {
                params.push(self.parse_lambda_param()?);
            }
        }

        self.expect(TokenKind::ThinArrow)?;

        let body = self.parse_expr()?;

        Ok(Spanned::new(
            ExprKind::Lambda {
                params,
                ret_type: None,
                body: Box::new(body),
            },
            span,
        ))
    }

    fn parse_lambda_param(&mut self) -> Result<Param, Error> {
        let span = self.mk_span();
        let name = self.parse_ident()?;
        let param_type = if self.consume(TokenKind::Colon) {
            self.parse_type()?
        } else {
            Type::Infer
        };
        Ok(Param {
            name,
            param_type,
            span,
        })
    }

    fn parse_tuple_or_grouped(&mut self) -> Result<Expr, Error> {
        let span = self.mk_span();
        self.expect(TokenKind::LParen)?;

        if self.check(TokenKind::RParen) {
            self.bump();
            return Ok(Spanned::new(ExprKind::Block(vec![]), span));
        }

        let expr = self.parse_expr()?;

        if self.check(TokenKind::Comma) {
            self.bump();
            let mut exprs = vec![expr];
            exprs.push(self.parse_expr()?);
            while self.consume(TokenKind::Comma) {
                exprs.push(self.parse_expr()?);
            }
            self.expect(TokenKind::RParen)?;
            return Ok(Spanned::new(ExprKind::Tuple(exprs), span));
        }

        self.expect(TokenKind::RParen)?;
        Ok(expr)
    }

    fn parse_ident(&mut self) -> Result<String, Error> {
        if let TokenKind::Ident(s) = &self.peek().kind {
            self.bump();
            Ok(s.clone())
        } else {
            Err(Error::new(self.mk_span(), "expected identifier"))
        }
    }

    fn check(&self, kind: TokenKind) -> bool {
        self.peek().kind == kind
    }

    fn check_keyword(&self, kw: Keyword) -> bool {
        matches!(&self.peek().kind, TokenKind::Keyword(k) if *k == kw)
    }

    fn peek(&self) -> Token {
        self.tokens
            .get(self.pos)
            .cloned()
            .unwrap_or(Token::dummy(TokenKind::Eof))
    }

    fn bump(&mut self) {
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
    }

    fn is_at_end(&self) -> bool {
        self.pos >= self.tokens.len() || self.peek().kind == TokenKind::Eof
    }

    fn consume(&mut self, kind: TokenKind) -> bool {
        if self.check(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn consume_keyword(&mut self, kw: Keyword) -> bool {
        if self.check_keyword(kw) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: TokenKind) -> Result<(), Error> {
        if self.check(kind.clone()) {
            self.bump();
            Ok(())
        } else {
            Err(Error::new(
                self.mk_span(),
                format!("expected {:?}, found {:?}", kind, self.peek().kind),
            ))
        }
    }

    fn expect_keyword(&mut self, kw: Keyword) -> Result<(), Error> {
        if self.check_keyword(kw) {
            self.bump();
            Ok(())
        } else {
            Err(Error::new(
                self.mk_span(),
                format!("expected keyword {:?}, found {:?}", kw, self.peek().kind),
            ))
        }
    }

    fn mk_span(&self) -> Span {
        let pos = self.pos;
        let tok = self
            .tokens
            .get(pos)
            .unwrap_or(&self.tokens[pos.saturating_sub(1)]);
        let end_offset = tok.span.end.offset;
        Span::new(
            Pos {
                offset: end_offset,
                line: tok.span.end.line,
                column: tok.span.end.column,
            },
            Pos {
                offset: end_offset,
                line: tok.span.end.line,
                column: tok.span.end.column,
            },
        )
    }

    fn error(&self, msg: &str) -> Error {
        Error::new(self.mk_span(), msg)
    }

    fn synchronize(&mut self) {
        while !self.is_at_end() {
            match self.peek().kind {
                TokenKind::Semicolon => {
                    self.bump();
                    return;
                }
                TokenKind::Keyword(
                    Keyword::Fn
                    | Keyword::Class
                    | Keyword::Interface
                    | Keyword::Enum
                    | Keyword::Trait
                    | Keyword::If
                    | Keyword::While
                    | Keyword::For
                    | Keyword::Loop
                    | Keyword::Return,
                ) => return,
                _ => self.bump(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tinox_lexer::Lexer;

    #[test]
    fn test_parse_fn() {
        let code = "fn main() -> Int32 { return 42; }";
        let mut lexer = Lexer::new(code);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        let result = parser.parse();
        assert!(result.is_ok(), "{:?}", result);
    }

    #[test]
    fn test_parse_class() {
        let code = "class Foo { x: Int32; }";
        let mut lexer = Lexer::new(code);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        let result = parser.parse();
        assert!(result.is_ok(), "{:?}", result);
    }
}
