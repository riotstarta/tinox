use tinox_common::{Error, ErrorBag, Span, Spanned};
use tinox_lexer::{InterpPart, Keyword, Lexer, Token, TokenKind};

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

    /// Parse a single expression from a source string (used for string interpolation)
    fn parse_expr_str(src: &str, span: Span) -> Result<Expr, Error> {
        let tokens = Lexer::new(src).tokenize()
            .map_err(|_| Error::new(span, format!("invalid expression in string interpolation: {}", src)))?;
        let mut p = Parser::new(tokens);
        p.parse_expr().map_err(|_| Error::new(span, format!("invalid expression in string interpolation: {}", src)))
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
        let annotations = self.parse_annotations();
        let doc = self.take_doc();
        let start = self.mk_span();

        let decl = if self.consume_keyword(Keyword::Async) {
            if self.check_keyword(Keyword::Fn) {
                let mut f = self.parse_fn()?;
                f.is_async = true;
                f.doc = doc;
                f.annotations = annotations;
                DeclKind::Function(f)
            } else {
                return Err(self.error("expected 'fn' after 'async'"));
            }
        } else if self.check_keyword(Keyword::Fn) {
            let mut f = self.parse_fn()?;
            f.doc = doc;
            f.annotations = annotations;
            DeclKind::Function(f)
        } else if self.check_keyword(Keyword::Class) {
            let mut c = self.parse_class()?;
            c.doc = doc;
            c.annotations = annotations;
            DeclKind::Class(c)
        } else if self.check_keyword(Keyword::Interface) {
            let mut i = self.parse_interface()?;
            i.doc = doc;
            i.annotations = annotations;
            DeclKind::Interface(i)
        } else if self.check_keyword(Keyword::Enum) {
            let mut e = self.parse_enum()?;
            e.doc = doc;
            e.annotations = annotations;
            DeclKind::Enum(e)
        } else if self.check_keyword(Keyword::Trait) {
            let mut t = self.parse_trait()?;
            t.doc = doc;
            t.annotations = annotations;
            DeclKind::Trait(t)
        } else if self.check_keyword(Keyword::Import) {
            DeclKind::Import(self.parse_import()?)
        } else if self.check_keyword(Keyword::Module) {
            self.bump();
            let mut name = self.parse_ident()?;
            while self.consume(TokenKind::Dot) {
                name.push('.');
                name.push_str(&self.parse_ident()?);
            }
            self.consume(TokenKind::Semicolon);
            DeclKind::Module(name)
        } else if self.check_keyword(Keyword::Namespace) {
            let mut ns = self.parse_namespace()?;
            ns.annotations = annotations;
            DeclKind::Namespace(ns)
        } else if self.check_keyword(Keyword::Extern) {
            self.parse_extern_fn()?
        } else if self.check_keyword(Keyword::Immutable) {
            let mut u = self.parse_immutable()?;
            u.doc = doc;
            u.annotations = annotations;
            DeclKind::Immutable(u)
        } else {
            return Err(self.error("expected declaration"));
        };

        Ok(Spanned::new(decl, start))
    }

    fn parse_immutable(&mut self) -> Result<ImmutableDecl, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::Immutable)?;
        let name = self.parse_ident()?;
        self.expect(TokenKind::LParen)?;
        let mut fields = Vec::new();
        if !self.check(TokenKind::RParen) {
            fields.push(self.parse_param()?);
            while self.consume(TokenKind::Comma) {
                fields.push(self.parse_param()?);
            }
        }
        self.expect(TokenKind::RParen)?;
        self.consume(TokenKind::Semicolon);
        Ok(ImmutableDecl { name, fields, span, doc: None, annotations: vec![] })
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
            doc: None,
            annotations: vec![],
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

        let body = if self.consume(TokenKind::Semicolon) {
            // Abstract method declaration (e.g. in interfaces): `fn foo() -> T;`
            let s = self.mk_span();
            Spanned::new(StmtKind::Block(vec![]), s)
        } else {
            self.parse_block()?
        };

        Ok(Function {
            name,
            type_params,
            params,
            ret_type,
            body,
            span,
            is_async: false,
            doc: None,
            annotations: vec![],
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
            let member_annotations = self.parse_annotations();
            let doc = self.take_doc();
            let vis = self.parse_visibility();
            // `var` = mutable field, `let` = immutable field; consume both
            let mutable = self.consume_keyword(Keyword::Var)
                || self.consume_keyword(Keyword::Let)
                || self.consume_keyword(Keyword::Mut);
            let is_async = self.consume_keyword(Keyword::Async);

            if self.check_keyword(Keyword::Fn) {
                let mut m = self.parse_method(vis, false)?;
                m.is_async = is_async;
                m.doc = doc;
                m.annotations = member_annotations;
                methods.push(m);
            } else if self.check_keyword(Keyword::Fnc) {
                let mut m = self.parse_method(vis, true)?;
                m.is_async = is_async;
                m.doc = doc;
                m.annotations = member_annotations;
                methods.push(m);
            } else {
                let mut f = self.parse_field(vis, mutable)?;
                f.doc = doc;
                f.annotations = member_annotations;
                fields.push(f);
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
            doc: None,
            annotations: vec![],
        })
    }

    fn parse_method_name(&mut self) -> Result<String, Error> {
        match self.peek().kind.clone() {
            TokenKind::Ident(s) => { self.bump(); Ok(s) }
            TokenKind::Keyword(Keyword::New) => { self.bump(); Ok("new".to_string()) }
            _ => Err(Error::new(self.mk_span(), "expected method name")),
        }
    }

    fn parse_method(&mut self, visibility: Visibility, static_: bool) -> Result<Method, Error> {
        let span = self.mk_span();
        if static_ {
            self.expect_keyword(Keyword::Fnc)?;
        } else {
            self.expect_keyword(Keyword::Fn)?;
        }
        let name = self.parse_method_name()?;
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
            doc: None,
            annotations: vec![],
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
            doc: None,
            annotations: vec![],
        })
    }

    fn parse_visibility(&mut self) -> Visibility {
        if self.consume_keyword(Keyword::Public) {
            Visibility::Public
        } else if self.consume_keyword(Keyword::Private) {
            Visibility::Private
        } else if self.consume_keyword(Keyword::Protected) {
            Visibility::Protected
        } else if self.consume_keyword(Keyword::Package) {
            Visibility::Package
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
            let doc = self.take_doc();
            let mut m = self.parse_fn()?;
            m.doc = doc;
            methods.push(m);
        }

        self.expect(TokenKind::RBrace)?;

        Ok(Interface {
            name,
            extends,
            methods,
            span,
            doc: None,
            annotations: vec![],
        })
    }

    fn parse_enum(&mut self) -> Result<Enum, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::Enum)?;
        let name = self.parse_ident()?;
        self.expect(TokenKind::LBrace)?;

        let mut variants = Vec::new();
        while !self.check(TokenKind::RBrace) {
            let doc = self.take_doc();
            let mut v = self.parse_enum_variant()?;
            v.doc = doc;
            variants.push(v);
            if !self.check(TokenKind::RBrace) {
                if !self.consume(TokenKind::Comma) && !self.consume(TokenKind::Semicolon) {
                    return Err(self.error("expected ',' or ';'"));
                }
            } else {
                self.consume(TokenKind::Comma);
                self.consume(TokenKind::Semicolon);
            }
        }

        self.expect(TokenKind::RBrace)?;

        Ok(Enum {
            name,
            variants,
            span,
            doc: None,
            annotations: vec![],
        })
    }

    fn parse_enum_variant(&mut self) -> Result<EnumVariant, Error> {
        let span = self.mk_span();
        let name = self.parse_ident()?;

        let mut args = Vec::new();
        if self.consume(TokenKind::LParen) {
            if !self.check(TokenKind::RParen) {
                // Support optional `name: Type` named fields (names are ignored)
                if matches!(self.peek().kind, TokenKind::Ident(_))
                    && matches!(self.peek_ahead(1).map(|t| t.kind), Some(TokenKind::Colon))
                {
                    self.bump(); self.bump(); // consume name + colon
                }
                args.push(self.parse_type()?);
                while self.consume(TokenKind::Comma) {
                    if self.check(TokenKind::RParen) { break; }
                    if matches!(self.peek().kind, TokenKind::Ident(_))
                        && matches!(self.peek_ahead(1).map(|t| t.kind), Some(TokenKind::Colon))
                    {
                        self.bump(); self.bump();
                    }
                    args.push(self.parse_type()?);
                }
            }
            self.expect(TokenKind::RParen)?;
        }

        Ok(EnumVariant { name, args, span, doc: None })
    }

    fn parse_trait(&mut self) -> Result<Trait, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::Trait)?;
        let name = self.parse_ident()?;
        self.expect(TokenKind::LBrace)?;

        let mut methods = Vec::new();
        while !self.check(TokenKind::RBrace) {
            let doc = self.take_doc();
            let mut m = self.parse_fn()?;
            m.doc = doc;
            methods.push(m);
        }

        self.expect(TokenKind::RBrace)?;

        Ok(Trait {
            name,
            methods,
            span,
            doc: None,
            annotations: vec![],
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

    fn parse_namespace(&mut self) -> Result<Namespace, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::Namespace)?;
        let mut name = vec![self.parse_ident()?];
        while self.consume(TokenKind::Dot) {
            name.push(self.parse_ident()?);
        }
        self.expect(TokenKind::LBrace)?;

        let mut decls = Vec::new();
        while !self.check(TokenKind::RBrace) && !self.is_at_end() {
            let doc = self.take_doc();
            let decl_start = self.mk_span();
            let inner = if self.check_keyword(Keyword::Class) {
                let mut c = self.parse_class()?;
                c.doc = doc;
                DeclKind::Class(c)
            } else if self.check_keyword(Keyword::Interface) {
                let mut i = self.parse_interface()?;
                i.doc = doc;
                DeclKind::Interface(i)
            } else if self.check_keyword(Keyword::Enum) {
                let mut e = self.parse_enum()?;
                e.doc = doc;
                DeclKind::Enum(e)
            } else if self.check_keyword(Keyword::Trait) {
                let mut t = self.parse_trait()?;
                t.doc = doc;
                DeclKind::Trait(t)
            } else if self.check_keyword(Keyword::Immutable) {
                let u = self.parse_immutable()?;
                DeclKind::Immutable(u)
            } else {
                let e = self.error("expected 'class', 'interface', 'enum', 'trait', or 'immutable' inside namespace");
                self.errors.push(e);
                self.synchronize();
                continue;
            };
            decls.push(Spanned::new(inner, decl_start));
        }

        self.expect(TokenKind::RBrace)?;
        Ok(Namespace { name, decls, span, annotations: vec![] })
    }

    fn parse_type(&mut self) -> Result<Type, Error> {
        // Tuple type: (T1, T2, ...)
        if self.check(TokenKind::LParen) {
            self.bump();
            let mut types = Vec::new();
            if !self.check(TokenKind::RParen) {
                types.push(self.parse_type()?);
                while self.consume(TokenKind::Comma) {
                    types.push(self.parse_type()?);
                }
            }
            self.expect(TokenKind::RParen)?;
            return Ok(Type::Tuple(types));
        }

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

        // fnc(T1, T2) -> R  — function/closure type
        if self.check_keyword(Keyword::Fnc) {
            self.bump();
            self.expect(TokenKind::LParen)?;
            let mut params = Vec::new();
            if !self.check(TokenKind::RParen) {
                params.push(self.parse_type()?);
                while self.consume(TokenKind::Comma) {
                    params.push(self.parse_type()?);
                }
            }
            self.expect(TokenKind::RParen)?;
            let ret = if self.consume(TokenKind::ThinArrow) {
                Box::new(self.parse_type()?)
            } else {
                Box::new(Type::Unit)
            };
            return Ok(Type::Fn { params, ret });
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
        // `Unit` is a keyword token, not an identifier — handle it before parse_ident
        if self.check_keyword(Keyword::Unit) {
            self.bump();
            return Ok(Type::Unit);
        }
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
            "Map" => {
                self.expect(TokenKind::Less)?;
                let k = self.parse_type()?;
                self.expect(TokenKind::Comma)?;
                let v = self.parse_type()?;
                self.expect(TokenKind::Greater)?;
                Ok(Type::Map(Box::new(k), Box::new(v)))
            }
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
                    let ident_span = self.peek().span;
                    let name = self.parse_ident()?;
                    if self.check(TokenKind::Equals) {
                        self.bump();
                        let value = self.parse_expr()?;
                        self.expect(TokenKind::Semicolon)?;
                        let target = Spanned::new(ExprKind::Ident(name.clone()), ident_span);
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
                        let target = Spanned::new(ExprKind::Ident(name), ident_span);
                        StmtKind::Expr(Spanned::new(
                            ExprKind::CompoundAssign {
                                op,
                                target: Box::new(target),
                                value: Box::new(value),
                            },
                            ident_span,
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
                                obj: Box::new(Spanned::new(ExprKind::Ident(name), ident_span)),
                                index: Box::new(index),
                            },
                            ident_span,
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
                                func: Box::new(Spanned::new(ExprKind::Ident(name), ident_span)),
                                args,
                            },
                            ident_span,
                        ))
                    } else if self.check(TokenKind::Dot) {
                        // Method call / field-access chain: m.set(...); obj.field.method();
                        let mut expr = Spanned::new(ExprKind::Ident(name), ident_span);
                        while self.consume(TokenKind::Dot) {
                            let field = self.parse_ident()?;
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
                                expr = Spanned::new(ExprKind::MethodCall { obj: Box::new(expr), method: field, args }, ident_span);
                            } else if self.check(TokenKind::Equals) {
                                self.bump();
                                let value = self.parse_expr()?;
                                self.expect(TokenKind::Semicolon)?;
                                let target = Spanned::new(ExprKind::FieldAccess { obj: Box::new(expr), field }, ident_span);
                                return Ok(Spanned::new(StmtKind::Assignment { target, value }, span));
                            } else {
                                expr = Spanned::new(ExprKind::FieldAccess { obj: Box::new(expr), field }, ident_span);
                            }
                        }
                        // obj.field[key] = value;  — index assignment on a field chain
                        if self.check(TokenKind::LBracket) {
                            self.bump();
                            let index = self.parse_expr()?;
                            self.expect(TokenKind::RBracket)?;
                            self.expect(TokenKind::Equals)?;
                            let value = self.parse_expr()?;
                            self.expect(TokenKind::Semicolon)?;
                            let target = Spanned::new(
                                ExprKind::Index { obj: Box::new(expr), index: Box::new(index) },
                                ident_span,
                            );
                            return Ok(Spanned::new(StmtKind::Assignment { target, value }, span));
                        }
                        self.expect(TokenKind::Semicolon)?;
                        StmtKind::Expr(expr)
                    } else {
                        self.expect(TokenKind::Semicolon)?;
                        StmtKind::Expr(Spanned::new(ExprKind::Ident(name), ident_span))
                    }
                } else {
                    let expr = self.parse_expr()?;
                    let is_block_expr = matches!(
                        expr.node,
                        ExprKind::Match { .. } | ExprKind::Block(_) | ExprKind::If { .. }
                    );
                    if !is_block_expr && !self.check(TokenKind::RBrace) {
                        self.expect(TokenKind::Semicolon)?;
                    } else {
                        self.consume(TokenKind::Semicolon);
                    }
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
            let has_paren = self.consume(TokenKind::LParen);
            let param = self.parse_ident()?;
            self.expect(TokenKind::Colon)?;
            let ty = self.parse_type()?;
            if has_paren { self.expect(TokenKind::RParen)?; }
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

        // Arrow lambda: `n => expr` or `(a, b) => expr`
        if self.check(TokenKind::FatArrow) {
            self.bump();
            let span = lhs.span;
            let params = Self::expr_to_lambda_params(lhs)?;
            let body = self.parse_assign_expr()?;
            return Ok(Spanned::new(
                ExprKind::Lambda { params, ret_type: None, body: Box::new(body) },
                span,
            ));
        }

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
                // Tuple index access: expr.0, expr.1, ...
                if let TokenKind::Integer(idx) = self.peek().kind.clone() {
                    let span = expr.span;
                    self.bump();
                    expr = Spanned::new(
                        ExprKind::TupleIndex {
                            tuple: Box::new(expr),
                            index: idx as usize,
                        },
                        span,
                    );
                    continue;
                }
                let name = self.parse_method_name()?;
                let span = expr.span;
                // `TypeName.Variant` → treat as enum variant (heuristic: obj is uppercase ident)
                let is_type_access = matches!(&expr.node, ExprKind::Ident(n) if n.chars().next().map_or(false, |c| c.is_uppercase()));
                if is_type_access {
                    let enum_name = match &expr.node { ExprKind::Ident(n) => n.clone(), _ => unreachable!() };
                    let mut args = vec![];
                    if self.check(TokenKind::LParen) {
                        self.bump();
                        if !self.check(TokenKind::RParen) {
                            // Skip optional `name:` named arg syntax
                            if matches!(self.peek().kind, TokenKind::Ident(_))
                                && matches!(self.peek_ahead(1).map(|t| t.kind), Some(TokenKind::Colon))
                            {
                                self.bump(); self.bump();
                            }
                            args.push(self.parse_expr()?);
                            while self.consume(TokenKind::Comma) {
                                if self.check(TokenKind::RParen) { break; }
                                if matches!(self.peek().kind, TokenKind::Ident(_))
                                    && matches!(self.peek_ahead(1).map(|t| t.kind), Some(TokenKind::Colon))
                                {
                                    self.bump(); self.bump();
                                }
                                args.push(self.parse_expr()?);
                            }
                        }
                        self.expect(TokenKind::RParen)?;
                    }
                    expr = Spanned::new(
                        ExprKind::EnumValue {
                            enum_name,
                            variant: name,
                            args,
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
                    expr = Spanned::new(
                        ExprKind::MethodCall {
                            obj: Box::new(expr),
                            method: name,
                            args,
                        },
                        span,
                    );
                } else {
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
                    let variant = self.parse_method_name()?;
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
            } else if self.check_keyword(Keyword::As) {
                self.bump();
                let span = expr.span;
                let ty = self.parse_type()?;
                expr = Spanned::new(ExprKind::Cast { expr: Box::new(expr), ty }, span);
            } else {
                break;
            }
        }

        Ok(expr)
    }

    fn parse_primary_expr(&mut self) -> Result<Expr, Error> {
        let token = self.peek();

        match &token.kind {
            TokenKind::At => {
                self.bump(); // consume @
                self.expect(TokenKind::LBrace)?;
                let mut entries = Vec::new();
                if !self.check(TokenKind::RBrace) {
                    let key = self.parse_expr()?;
                    self.expect(TokenKind::FatArrow)?;
                    let val = self.parse_expr()?;
                    entries.push((key, val));
                    while self.consume(TokenKind::Comma) {
                        if self.check(TokenKind::RBrace) { break; }
                        let key = self.parse_expr()?;
                        self.expect(TokenKind::FatArrow)?;
                        let val = self.parse_expr()?;
                        entries.push((key, val));
                    }
                }
                self.expect(TokenKind::RBrace)?;
                Ok(Spanned::new(ExprKind::MapLiteral(entries), token.span))
            }
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
            TokenKind::InterpString(parts) => {
                self.bump();
                let span = token.span;
                // Build concat tree: "" + toString(expr) + "str" + ...
                // Start with empty string, then fold over parts
                let mut result: Expr = Spanned::new(
                    ExprKind::Literal(Literal::String(String::new())),
                    span,
                );
                let mut is_first = true;
                for part in parts {
                    let part_expr: Expr = match part {
                        InterpPart::Str(s) => Spanned::new(
                            ExprKind::Literal(Literal::String(s.clone())),
                            span,
                        ),
                        InterpPart::Expr(src) => {
                            // Re-lex and re-parse the expression source
                            let inner_expr = Parser::parse_expr_str(&src, span)?;
                            // Wrap in toString(inner_expr)
                            Spanned::new(
                                ExprKind::Call {
                                    func: Box::new(Spanned::new(
                                        ExprKind::Ident("toString".to_string()),
                                        span,
                                    )),
                                    args: vec![inner_expr],
                                },
                                span,
                            )
                        }
                    };
                    if is_first {
                        result = part_expr;
                        is_first = false;
                    } else {
                        result = Spanned::new(
                            ExprKind::Binary {
                                op: BinaryOp::Add,
                                lhs: Box::new(result),
                                rhs: Box::new(part_expr),
                            },
                            span,
                        );
                    }
                }
                Ok(result)
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
            TokenKind::Keyword(Keyword::Fnc) if self.peek_ahead(1).map_or(false, |t| matches!(t.kind, TokenKind::LParen)) => self.parse_fnc_lambda(),
            TokenKind::Keyword(Keyword::Fn) if self.peek_ahead(1).map_or(false, |t| matches!(t.kind, TokenKind::LParen)) => self.parse_fn_lambda(),
            TokenKind::LParen => {
                // Peek ahead: `(ident :` → typed lambda params
                let is_typed_lambda = matches!(
                    (self.peek_ahead(1).map(|t| t.kind.clone()), self.peek_ahead(2).map(|t| t.kind.clone())),
                    (Some(TokenKind::Ident(_)), Some(TokenKind::Colon))
                );
                if is_typed_lambda {
                    self.parse_typed_lambda()
                } else {
                    self.parse_tuple_or_grouped()
                }
            }
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
            let body_is_block = matches!(body.node, ExprKind::Block(_));

            cases.push(MatchCase {
                pattern,
                guard,
                body,
                span: pattern_span,
            });

            // Block bodies don't need a trailing separator
            if !body_is_block {
                if !self.check(TokenKind::RBrace) {
                    if !self.consume(TokenKind::Comma) && !self.consume(TokenKind::Semicolon) {
                        return Err(self.error("expected ',' or ';'"));
                    }
                } else {
                    self.consume(TokenKind::Comma);
                    self.consume(TokenKind::Semicolon);
                }
            } else {
                self.consume(TokenKind::Comma);
                self.consume(TokenKind::Semicolon);
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

        // Handle enum variant patterns: Color::Red, Color.Red, or Option::Some(x)
        if self.check(TokenKind::ColonColon) || self.check(TokenKind::Dot) {
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

    fn parse_typed_lambda(&mut self) -> Result<Expr, Error> {
        let span = self.mk_span();
        self.expect(TokenKind::LParen)?;
        let mut params = Vec::new();
        if !self.check(TokenKind::RParen) {
            params.push(self.parse_lambda_param()?);
            while self.consume(TokenKind::Comma) {
                if self.check(TokenKind::RParen) { break; }
                params.push(self.parse_lambda_param()?);
            }
        }
        self.expect(TokenKind::RParen)?;
        self.expect(TokenKind::FatArrow)?;
        let body = self.parse_assign_expr()?;
        Ok(Spanned::new(ExprKind::Lambda { params, ret_type: None, body: Box::new(body) }, span))
    }

    fn parse_fn_lambda(&mut self) -> Result<Expr, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::Fn)?;
        self.expect(TokenKind::LParen)?;
        let mut params = Vec::new();
        if !self.check(TokenKind::RParen) {
            params.push(self.parse_fn_lambda_param()?);
            while self.consume(TokenKind::Comma) {
                if self.check(TokenKind::RParen) { break; }
                params.push(self.parse_fn_lambda_param()?);
            }
        }
        self.expect(TokenKind::RParen)?;
        let ret_type = if self.consume(TokenKind::ThinArrow) {
            Some(self.parse_type()?)
        } else {
            None
        };
        let body = self.parse_expr()?;
        Ok(Spanned::new(
            ExprKind::Lambda {
                params,
                ret_type,
                body: Box::new(body),
            },
            span,
        ))
    }

    fn parse_fnc_lambda(&mut self) -> Result<Expr, Error> {
        let span = self.mk_span();
        self.expect_keyword(Keyword::Fnc)?;
        self.expect(TokenKind::LParen)?;
        let mut params = Vec::new();
        if !self.check(TokenKind::RParen) {
            params.push(self.parse_fn_lambda_param()?);
            while self.consume(TokenKind::Comma) {
                if self.check(TokenKind::RParen) { break; }
                params.push(self.parse_fn_lambda_param()?);
            }
        }
        self.expect(TokenKind::RParen)?;
        let ret_type = if self.consume(TokenKind::ThinArrow) {
            Some(self.parse_type()?)
        } else {
            None
        };
        let body = self.parse_expr()?;
        Ok(Spanned::new(ExprKind::Lambda { params, ret_type, body: Box::new(body) }, span))
    }

    fn parse_fn_lambda_param(&mut self) -> Result<Param, Error> {
        let span = self.mk_span();
        let name = self.parse_ident()?;
        let param_type = if self.consume(TokenKind::Colon) {
            self.parse_type()?
        } else {
            Type::Infer
        };
        Ok(Param { name, param_type, span })
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

    fn expr_to_lambda_params(expr: Expr) -> Result<Vec<Param>, Error> {
        match expr.node {
            ExprKind::Ident(name) => Ok(vec![Param { name, param_type: Type::Infer, span: expr.span }]),
            ExprKind::Tuple(exprs) => {
                let mut params = Vec::new();
                for e in exprs {
                    match e.node {
                        ExprKind::Ident(name) => params.push(Param { name, param_type: Type::Infer, span: e.span }),
                        _ => return Err(Error::new(e.span, "expected identifier in lambda parameter list")),
                    }
                }
                Ok(params)
            }
            _ => Err(Error::new(expr.span, "expected identifier or parameter list before '=>'")),
        }
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

    fn peek_ahead(&self, offset: usize) -> Option<Token> {
        self.tokens.get(self.pos + offset).cloned()
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

    fn take_doc(&mut self) -> Option<String> {
        while self.peek().kind.is_trivia() && !self.is_at_end() {
            if let TokenKind::DocComment(text) = &self.peek().kind {
                let doc = text.clone();
                self.bump();
                return Some(doc);
            }
            self.bump();
        }
        if let TokenKind::DocComment(text) = &self.peek().kind {
            let doc = text.clone();
            self.bump();
            Some(doc)
        } else {
            None
        }
    }

    fn parse_annotations(&mut self) -> Vec<Annotation> {
        let mut annotations = Vec::new();
        loop {
            if self.check(TokenKind::At) {
                let next = self.peek_ahead(1);
                if let Some(tok) = next {
                    if matches!(tok.kind, TokenKind::Ident(_)) {
                        let span = self.mk_span();
                        self.bump(); // consume @
                        let name = self.parse_ident().unwrap_or_default();
                        let mut args = Vec::new();
                        if self.check(TokenKind::LParen) {
                            self.bump(); // consume (
                            if !self.check(TokenKind::RParen) {
                                if let Ok(arg) = self.parse_annotation_arg() {
                                    args.push(arg);
                                    while self.consume(TokenKind::Comma) {
                                        if let Ok(arg) = self.parse_annotation_arg() {
                                            args.push(arg);
                                        }
                                    }
                                }
                            }
                            self.expect(TokenKind::RParen).ok();
                        }
                        annotations.push(Annotation { name, args, span });
                        continue;
                    }
                }
            }
            break;
        }
        annotations
    }

    fn parse_annotation_arg(&mut self) -> Result<Literal, Error> {
        let token = self.peek();
        match &token.kind {
            TokenKind::Integer(n) => {
                let val = *n;
                self.bump();
                Ok(Literal::Integer(val))
            }
            TokenKind::Float(f) => {
                let val = *f;
                self.bump();
                Ok(Literal::Float(val))
            }
            TokenKind::String(s) => {
                let val = s.clone();
                self.bump();
                Ok(Literal::String(val))
            }
            TokenKind::Bool(b) => {
                let val = *b;
                self.bump();
                Ok(Literal::Bool(val))
            }
            TokenKind::Keyword(Keyword::True) => {
                self.bump();
                Ok(Literal::Bool(true))
            }
            TokenKind::Keyword(Keyword::False) => {
                self.bump();
                Ok(Literal::Bool(false))
            }
            _ => Err(Error::new(token.span, "expected annotation argument (string, int, float, or bool)")),
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
        Span::new(tok.span.start, tok.span.start)
    }

    fn error(&self, msg: &str) -> Error {
        Error::new(self.mk_span(), msg)
    }

    fn synchronize(&mut self) {
        let start_pos = self.pos;
        while !self.is_at_end() {
            match self.peek().kind {
                TokenKind::Semicolon => {
                    self.bump();
                    return;
                }
                TokenKind::Keyword(
                    Keyword::Fn
                    | Keyword::Fnc
                    | Keyword::Class
                    | Keyword::Interface
                    | Keyword::Enum
                    | Keyword::Trait
                    | Keyword::Namespace
                    | Keyword::If
                    | Keyword::While
                    | Keyword::For
                    | Keyword::Loop
                    | Keyword::Return,
                ) => {
                    if self.pos > start_pos {
                        return;
                    }
                    self.bump();
                }
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
