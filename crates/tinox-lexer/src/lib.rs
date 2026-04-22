use tinox_common::{Error, Pos, Span};

/// A part of a string interpolation: either a literal segment or an expression source
#[derive(Debug, Clone, PartialEq)]
pub enum InterpPart {
    Str(String),
    Expr(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literals
    Integer(i64),
    IntegerSuffix(IntSuffix),
    Float(f64),
    FloatSuffix(FloatType),
    String(String),
    RawString(String),
    /// Interpolated string: `"Hello ${name}!"` → parts
    InterpString(Vec<InterpPart>),
    Char(char),
    Byte(u8),
    Bool(bool),

    // Identifiers
    Ident(String),
    Keyword(Keyword),

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Equals,
    EqualsEquals,
    Bang,
    BangEquals,
    Less,
    LessEquals,
    Greater,
    GreaterEquals,
    AmpAmp,
    BarBar,
    Caret,
    Tilde,
    Ampersand,
    Bar,
    LessLess,
    GreaterGreater,
    GreaterGreaterGreater,
    PlusPlus,
    MinusMinus,
    PlusEquals,
    MinusEquals,
    StarEquals,
    SlashEquals,
    PercentEquals,
    AmpersandEquals,
    BarEquals,
    CaretEquals,
    LessLessEquals,
    GreaterGreaterEquals,
    GreaterGreaterGreaterEquals,

    // Compound
    Dot,
    DotDot,
    DotDotDot,
    Colon,
    ColonColon,
    Semicolon,
    Comma,
    At,
    Question,
    QuestionQuestion,
    QuestionColon,
    FatArrow,
    ThinArrow,
    Pipe,
    Backslash,

    // Delimiters
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,

    // Special
    Comment(String),
    DocComment(String),
    Whitespace,
    Newline,
    Underscore,
    Eof,
    Error(String),
}

impl TokenKind {
    pub fn is_trivia(&self) -> bool {
        matches!(
            self,
            TokenKind::Whitespace | TokenKind::Newline | TokenKind::Comment(_)
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }

    pub fn dummy(kind: TokenKind) -> Self {
        Self {
            kind,
            span: Span::dummy(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum IntSuffix {
    Unsigned8,
    Unsigned16,
    Unsigned32,
    Unsigned64,
    Signed8,
    Signed16,
    Signed32,
    Signed64,
    Size,
    Usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FloatType {
    Float32,
    Float64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyword {
    If,
    Else,
    While,
    For,
    Loop,
    Return,
    Break,
    Continue,
    Match,
    Case,

    Class,
    Interface,
    Extends,
    Implements,
    Trait,
    Enum,
    New,
    This,
    Super,
    Self_,
    SelfType,

    Public,
    Private,
    Protected,
    Static,
    Final,
    Abstract,
    Override,
    Readonly,

    Var,
    Val,
    Let,
    Const,
    Mut,

    Fn,
    Fnc,
    Throws,
    Throw,
    Try,
    Catch,
    Finally,
    Defer,

    Spawn,
    Channel,
    Send,
    Recv,
    Select,
    Default,
    Async,
    Await,

    Module,
    Namespace,
    Import,
    Export,
    As,
    In,
    Where,
    Extern,
    Unsafe,

    Ref,
    SizeOf,
    TypeOf,
    Is,
    As_,
    Cast,
    Null,
    True,
    False,
    Unit,
    Never,
    Any,
    DotSelf,
}

impl Keyword {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "if" => Some(Keyword::If),
            "else" => Some(Keyword::Else),
            "while" => Some(Keyword::While),
            "for" => Some(Keyword::For),
            "loop" => Some(Keyword::Loop),
            "return" => Some(Keyword::Return),
            "break" => Some(Keyword::Break),
            "continue" => Some(Keyword::Continue),
            "match" => Some(Keyword::Match),
            "case" => Some(Keyword::Case),
            "class" => Some(Keyword::Class),
            "interface" => Some(Keyword::Interface),
            "extends" => Some(Keyword::Extends),
            "implements" => Some(Keyword::Implements),
            "trait" => Some(Keyword::Trait),
            "enum" => Some(Keyword::Enum),
            "new" => Some(Keyword::New),
            "this" => Some(Keyword::This),
            "super" => Some(Keyword::Super),
            "self" => Some(Keyword::Self_),
            "Self" => Some(Keyword::SelfType),
            "public" => Some(Keyword::Public),
            "private" => Some(Keyword::Private),
            "protected" => Some(Keyword::Protected),
            "static" => Some(Keyword::Static),
            "final" => Some(Keyword::Final),
            "abstract" => Some(Keyword::Abstract),
            "override" => Some(Keyword::Override),
            "readonly" => Some(Keyword::Readonly),
            "var" => Some(Keyword::Var),
            "val" => Some(Keyword::Val),
            "let" => Some(Keyword::Let),
            "const" => Some(Keyword::Const),
            "mut" => Some(Keyword::Mut),
            "fn" => Some(Keyword::Fn),
            "fnc" => Some(Keyword::Fnc),
            "throws" => Some(Keyword::Throws),
            "try" => Some(Keyword::Try),
            "catch" => Some(Keyword::Catch),
            "finally" => Some(Keyword::Finally),
            "defer" => Some(Keyword::Defer),
            "spawn" => Some(Keyword::Spawn),
            "channel" => Some(Keyword::Channel),
            "send" => Some(Keyword::Send),
            "recv" => Some(Keyword::Recv),
            "select" => Some(Keyword::Select),
            "default" => Some(Keyword::Default),
            "async" => Some(Keyword::Async),
            "await" => Some(Keyword::Await),
            "module" => Some(Keyword::Module),
            "namespace" => Some(Keyword::Namespace),
            "import" => Some(Keyword::Import),
            "export" => Some(Keyword::Export),
            "as" => Some(Keyword::As),
            "in" => Some(Keyword::In),
            "where" => Some(Keyword::Where),
            "extern" => Some(Keyword::Extern),
            "unsafe" => Some(Keyword::Unsafe),
            "ref" => Some(Keyword::Ref),
            "sizeof" => Some(Keyword::SizeOf),
            "typeof" => Some(Keyword::TypeOf),
            "is" => Some(Keyword::Is),
            "cast" => Some(Keyword::Cast),
            "null" => Some(Keyword::Null),
            "true" => Some(Keyword::True),
            "false" => Some(Keyword::False),
            "Unit" => Some(Keyword::Unit),
            "Never" => Some(Keyword::Never),
            "Any" => Some(Keyword::Any),
            ".self" => Some(Keyword::DotSelf),
            _ => None,
        }
    }
}

pub struct Lexer<'a> {
    input: &'a str,
    chars: Vec<char>,
    pos: usize,
    start: usize,
    line: u32,
    column: u32,
    start_column: u32,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        let chars: Vec<char> = input.chars().collect();
        Self {
            input,
            chars,
            pos: 0,
            start: 0,
            line: 1,
            column: 1,
            start_column: 1,
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, Vec<Error>> {
        let mut tokens = Vec::new();
        let mut errors = Vec::new();

        loop {
            let token = match self.next_token() {
                Ok(t) => t,
                Err(e) => {
                    errors.push(e);
                    // Skip the problematic character
                    self.pos += 1;
                    continue;
                }
            };

            if token.kind == TokenKind::Eof {
                tokens.push(token);
                break;
            }

            if !token.kind.is_trivia() {
                tokens.push(token);
            }
        }

        if errors.is_empty() {
            Ok(tokens)
        } else {
            Err(errors)
        }
    }

    fn next_token(&mut self) -> Result<Token, Error> {
        self.start = self.pos;
        self.start_column = self.column;

        if self.pos >= self.chars.len() {
            return Ok(Token::new(
                TokenKind::Eof,
                Span::new(self.mk_pos(), self.mk_pos()),
            ));
        }

        let ch = self.peek();

        let token = match ch {
            '\n' => {
                self.bump();
                self.line += 1;
                self.column = 1;
                Token::new(TokenKind::Newline, self.mk_span())
            }
            ' ' | '\t' | '\r' => {
                self.bump();
                Token::new(TokenKind::Whitespace, self.mk_span())
            }
            '/' => {
                if self.peek_next() == '/' {
                    self.read_line_comment()
                } else if self.peek_next() == '*' {
                    self.read_block_comment()
                } else {
                    self.read_operator()
                }
            }
            '"' => {
                if self.peek_next() == '#' {
                    self.read_raw_string()?
                } else {
                    self.read_string()?
                }
            }
            'r' => {
                if self.peek_next() == '"' || (self.peek_next() == '#' && self.chars.get(self.pos + 2) == Some(&'"')) {
                    self.read_raw_string()?
                } else {
                    self.read_ident_or_keyword()
                }
            }
            '\'' => {
                self.read_char()?
            }
            '0'..='9' => self.read_number()?,
            'a'..='z' | 'A'..='Z' | '_' => {
                if ch == 'b' && self.peek_next() == '\'' {
                    self.bump(); // consume 'b'
                    self.read_byte_literal()? // now we're at the opening quote
                } else {
                    self.read_ident_or_keyword()
                }
            }
            '+' => self.read_plus(),
            '-' => self.read_minus(),
            '*' => self.read_star(),
            '%' => self.read_percent(),
            '=' => self.read_equals(),
            '!' => self.read_bang(),
            '<' => self.read_less(),
            '>' => self.read_greater(),
            '&' => self.read_ampersand(),
            '|' => self.read_bar(),
            '^' => self.read_caret(),
            '~' => {
                self.bump();
                Token::new(TokenKind::Tilde, self.mk_span())
            }
            '\\' => {
                self.bump();
                Token::new(TokenKind::Backslash, self.mk_span())
            }
            '.' => self.read_dot(),
            ':' => self.read_colon(),
            ';' => {
                self.bump();
                Token::new(TokenKind::Semicolon, self.mk_span())
            }
            ',' => {
                self.bump();
                Token::new(TokenKind::Comma, self.mk_span())
            }
            '@' => {
                self.bump();
                Token::new(TokenKind::At, self.mk_span())
            }
            '?' => self.read_question(),
            '(' => {
                self.bump();
                Token::new(TokenKind::LParen, self.mk_span())
            }
            ')' => {
                self.bump();
                Token::new(TokenKind::RParen, self.mk_span())
            }
            '{' => {
                self.bump();
                Token::new(TokenKind::LBrace, self.mk_span())
            }
            '}' => {
                self.bump();
                Token::new(TokenKind::RBrace, self.mk_span())
            }
            '[' => {
                self.bump();
                Token::new(TokenKind::LBracket, self.mk_span())
            }
            ']' => {
                self.bump();
                Token::new(TokenKind::RBracket, self.mk_span())
            }
            _ => {
                let span = self.mk_span();
                self.bump();
                return Err(Error::new(span, format!("unexpected character: {}", ch)));
            }
        };

        Ok(token)
    }

    fn peek(&self) -> char {
        self.chars.get(self.pos).copied().unwrap_or('\0')
    }

    fn peek_next(&self) -> char {
        self.chars.get(self.pos + 1).copied().unwrap_or('\0')
    }

    fn bump(&mut self) {
        self.pos += 1;
        self.column += 1;
    }

    fn mk_pos(&self) -> Pos {
        Pos {
            offset: self.pos as u32,
            line: self.line,
            column: self.start_column,
        }
    }

    fn mk_span(&self) -> Span {
        Span::new(
            Pos {
                offset: self.start as u32,
                line: self.line,
                column: self.start_column,
            },
            Pos {
                offset: self.pos as u32,
                line: self.line,
                column: self.column,
            },
        )
    }

    fn read_operator(&mut self) -> Token {
        self.bump();
        Token::new(TokenKind::Slash, self.mk_span())
    }

    fn read_line_comment(&mut self) -> Token {
        self.bump(); // /
        self.bump(); // /
        let start = self.pos;
        while self.pos < self.chars.len() && self.peek() != '\n' {
            self.bump();
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        Token::new(TokenKind::Comment(text), self.mk_span())
    }

    fn read_block_comment(&mut self) -> Token {
        self.bump(); // /
        self.bump(); // *
        let is_doc = self.peek() == '*';
        if is_doc {
            self.bump(); // *
        }
        let start = self.pos;
        let mut depth = 1;
        while depth > 0 && self.pos < self.chars.len() {
            if self.peek() == '/' && self.peek_next() == '*' {
                self.bump();
                self.bump();
                depth += 1;
            } else if self.peek() == '*' && self.peek_next() == '/' {
                self.bump();
                self.bump();
                depth -= 1;
            } else {
                if self.peek() == '\n' {
                    self.line += 1;
                    self.column = 0;
                }
                self.bump();
            }
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        if is_doc {
            Token::new(TokenKind::DocComment(text), self.mk_span())
        } else {
            Token::new(TokenKind::Comment(text), self.mk_span())
        }
    }

    fn read_string(&mut self) -> Result<Token, Error> {
        self.bump(); // opening "
        let _start = self.pos;
        let mut current_str = String::new();
        let mut parts: Vec<InterpPart> = Vec::new();
        let mut has_interp = false;

        while self.pos < self.chars.len() && self.peek() != '"' {
            let ch = self.peek();
            if ch == '\n' {
                return Err(Error::new(self.mk_span(), "unterminated string literal"));
            }
            // Detect ${ for interpolation
            if ch == '$' && self.pos + 1 < self.chars.len() && self.chars[self.pos + 1] == '{' {
                has_interp = true;
                parts.push(InterpPart::Str(std::mem::take(&mut current_str)));
                self.bump(); // $
                self.bump(); // {
                // Collect expression source until matching }
                let mut expr_src = String::new();
                let mut depth = 1usize;
                while self.pos < self.chars.len() && depth > 0 {
                    let ec = self.peek();
                    if ec == '{' { depth += 1; }
                    if ec == '}' {
                        depth -= 1;
                        if depth == 0 { self.bump(); break; }
                    }
                    expr_src.push(ec);
                    self.bump();
                }
                parts.push(InterpPart::Expr(expr_src));
            } else if ch == '\\' {
                self.bump();
                if self.pos >= self.chars.len() {
                    return Err(Error::new(self.mk_span(), "unterminated escape sequence"));
                }
                let escaped = self.read_escape()?;
                current_str.push(escaped);
            } else {
                current_str.push(ch);
                self.bump();
            }
        }

        if self.pos >= self.chars.len() {
            return Err(Error::new(self.mk_span(), "unterminated string literal"));
        }

        self.bump(); // closing "

        if has_interp {
            parts.push(InterpPart::Str(current_str));
            Ok(Token::new(TokenKind::InterpString(parts), self.mk_span()))
        } else {
            Ok(Token::new(TokenKind::String(current_str), self.mk_span()))
        }
    }

    fn read_raw_string(&mut self) -> Result<Token, Error> {
        // Count hash characters
        let mut hashes = 0usize;
        let ch = self.peek();
        if ch == 'r' {
            self.bump(); // r
        }
        while self.peek() == '#' {
            self.bump();
            hashes += 1;
        }
        
        // Expect opening "
        if self.peek() != '"' {
            return Err(Error::new(self.mk_span(), "expected opening \" for raw string"));
        }
        self.bump(); // opening "
        
        let start = self.pos;
        let mut value = String::new();
        
        // Read until we find closing delimiter: " followed by hashes
        loop {
            if self.pos >= self.chars.len() {
                return Err(Error::new(self.mk_span(), "unterminated raw string literal"));
            }
            
            let ch = self.peek();
            if ch == '"' {
                // Check if followed by correct number of hashes
                let mut h = 0usize;
                let mut check_pos = self.pos + 1;
                while h < hashes && check_pos < self.chars.len() && self.chars[check_pos] == '#' {
                    h += 1;
                    check_pos += 1;
                }
                
                if h == hashes {
                    // Found closing delimiter
                    value = self.chars[start..self.pos].iter().collect();
                    self.pos = check_pos; // Move past closing delimiter
                    self.column = (self.column as usize + hashes + 1) as u32;
                    return Ok(Token::new(TokenKind::RawString(value), self.mk_span()));
                }
            }
            
            if ch == '\n' {
                self.line += 1;
                self.column = 0;
            }
            value.push(ch);
            self.bump();
        }
    }

    fn read_escape(&mut self) -> Result<char, Error> {
        let ch = self.peek();
        self.bump();
        match ch {
            'n' => Ok('\n'),
            't' => Ok('\t'),
            'r' => Ok('\r'),
            '\\' => Ok('\\'),
            '"' => Ok('"'),
            '\'' => Ok('\''),
            '0' => Ok('\0'),
            'x' => {
                let d1 = self.read_hex_digit()?;
                let d2 = self.read_hex_digit()?;
                let code = (d1 * 16 + d2) as u32;
                char::from_u32(code).ok_or_else(|| Error::new(self.mk_span(), "invalid hex escape"))
            }
            'u' => {
                self.expect_char('{')?;
                let mut value = 0u32;
                let mut count = 0;
                while self.peek() != '}' && count < 6 {
                    let d = self.read_hex_digit()?;
                    value = value * 16 + d as u32;
                    count += 1;
                }
                self.expect_char('}')?;
                char::from_u32(value)
                    .ok_or_else(|| Error::new(self.mk_span(), "invalid unicode escape"))
            }
            'U' => {
                let mut value = 0u32;
                for _ in 0..8 {
                    value = value * 16 + self.read_hex_digit()? as u32;
                }
                char::from_u32(value)
                    .ok_or_else(|| Error::new(self.mk_span(), "invalid unicode escape"))
            }
            _ => Err(Error::new(
                self.mk_span(),
                format!("unknown escape sequence: \\{}", ch),
            )),
        }
    }

    fn read_hex_digit(&mut self) -> Result<u32, Error> {
        let ch = self.peek();
        self.bump();
        match ch {
            '0'..='9' => Ok(ch as u32 - '0' as u32),
            'a'..='f' => Ok(ch as u32 - 'a' as u32 + 10),
            'A'..='F' => Ok(ch as u32 - 'A' as u32 + 10),
            _ => Err(Error::new(
                self.mk_span(),
                format!("expected hex digit, found: {}", ch),
            )),
        }
    }

    fn expect_char(&mut self, ch: char) -> Result<(), Error> {
        if self.peek() == ch {
            self.bump();
            Ok(())
        } else {
            Err(Error::new(self.mk_span(), format!("expected '{}'", ch)))
        }
    }

    fn read_char(&mut self) -> Result<Token, Error> {
        self.bump(); // opening '
        let ch = if self.peek() == '\\' {
            self.bump();
            self.read_escape()?
        } else {
            let ch = self.peek();
            if ch == '\'' {
                return Err(Error::new(self.mk_span(), "empty character literal"));
            }
            self.bump();
            ch
        };

        if self.peek() != '\'' {
            return Err(Error::new(self.mk_span(), "unterminated char literal"));
        }
        self.bump(); // closing '

        Ok(Token::new(TokenKind::Char(ch), self.mk_span()))
    }

    fn read_byte_literal(&mut self) -> Result<Token, Error> {
        self.bump(); // ' (skip the opening quote, 'b' was already consumed)
        let ch = if self.peek() == '\\' {
            self.bump();
            let escaped = self.read_escape()?;
            escaped as u8
        } else {
            let ch = self.peek();
            if ch == '\'' {
                return Err(Error::new(self.mk_span(), "empty byte literal"));
            }
            if ch as u32 > 127 {
                return Err(Error::new(self.mk_span(), "byte literal must be in range 0-127"));
            }
            self.bump();
            ch as u8
        };

        if self.peek() != '\'' {
            return Err(Error::new(self.mk_span(), "unterminated byte literal"));
        }
        self.bump(); // closing '

        Ok(Token::new(TokenKind::Byte(ch), self.mk_span()))
    }

    fn read_number(&mut self) -> Result<Token, Error> {
        let start = self.pos;

        // Integer part
        if self.peek() == '0' {
            match self.peek_next() {
                'x' | 'X' => return self.read_hex_int(start),
                'o' | 'O' => return self.read_octal_int(start),
                'b' | 'B' => return self.read_binary_int(start),
                '0'..='9' | '.' | '_' => {}
                _ => {
                    self.bump();
                    return Ok(Token::new(TokenKind::Integer(0), self.mk_span()));
                }
            }
        }

        while self.peek().is_ascii_digit() || self.peek() == '_' {
            self.bump();
        }

        // Check for float — but not if this number was itself accessed via a dot (e.g. tuple.0.1)
        let preceded_by_dot = start > 0 && self.chars[start - 1] == '.';
        if !preceded_by_dot && self.peek() == '.' && (self.peek_next().is_ascii_digit() || self.peek_next() == '_') {
            self.bump(); // .
            while self.peek().is_ascii_digit() || self.peek() == '_' {
                self.bump();
            }

            // Scientific notation
            if self.peek() == 'e' || self.peek() == 'E' {
                self.bump();
                if self.peek() == '+' || self.peek() == '-' {
                    self.bump();
                }
                while self.peek().is_ascii_digit() || self.peek() == '_' {
                    self.bump();
                }
            }

            // Check for float suffix
            let suffix = self.read_float_suffix();
            
            let text: String = self.chars[start..self.pos].iter().filter(|c| **c != '_').collect();
            let value: f64 = text.parse().unwrap_or(0.0);
            
            return Ok(if suffix == FloatType::Float64 {
                Token::new(TokenKind::Float(value), self.mk_span())
            } else {
                Token::new(TokenKind::Float(value), self.mk_span())
            });
        }

        // Check for integer suffix
        let suffix = self.read_int_suffix();
        
        let text: String = self.chars[start..self.pos].iter().filter(|c| **c != '_').collect();
        let value: i64 = text.parse().unwrap_or(0);
        
        match suffix {
            Some(s) => Ok(Token::new(TokenKind::IntegerSuffix(s), self.mk_span())),
            None => Ok(Token::new(TokenKind::Integer(value), self.mk_span())),
        }
    }

    fn read_int_suffix(&mut self) -> Option<IntSuffix> {
        let start = self.pos;
        
        // Skip underscore before suffix (e.g., 42_i32)
        if self.peek() == '_' {
            self.bump();
        }
        
        let suffix = match (self.peek(), self.peek_next()) {
            ('u', '8') => { self.bump(); self.bump(); Some(IntSuffix::Unsigned8) }
            ('u', '1') if self.chars.get(self.pos + 2) == Some(&'6') => { self.bump(); self.bump(); self.bump(); Some(IntSuffix::Unsigned16) }
            ('u', '3') if self.chars.get(self.pos + 2) == Some(&'2') => { self.bump(); self.bump(); self.bump(); Some(IntSuffix::Unsigned32) }
            ('u', '6') if self.chars.get(self.pos + 2) == Some(&'4') => { self.bump(); self.bump(); self.bump(); Some(IntSuffix::Unsigned64) }
            ('i', '8') => { self.bump(); self.bump(); Some(IntSuffix::Signed8) }
            ('i', '1') if self.chars.get(self.pos + 2) == Some(&'6') => { self.bump(); self.bump(); self.bump(); Some(IntSuffix::Signed16) }
            ('i', '3') if self.chars.get(self.pos + 2) == Some(&'2') => { self.bump(); self.bump(); self.bump(); Some(IntSuffix::Signed32) }
            ('i', '6') if self.chars.get(self.pos + 2) == Some(&'4') => { self.bump(); self.bump(); self.bump(); Some(IntSuffix::Signed64) }
            ('u', 's') if self.chars.get(self.pos + 2) == Some(&'i') && self.chars.get(self.pos + 3) == Some(&'z') => { 
                self.bump(); self.bump(); self.bump(); self.bump(); Some(IntSuffix::Usize) 
            }
            ('i', 's') if self.chars.get(self.pos + 2) == Some(&'i') && self.chars.get(self.pos + 3) == Some(&'z') => { 
                self.bump(); self.bump(); self.bump(); self.bump(); Some(IntSuffix::Size) 
            }
            _ => None,
        };
        
        if suffix.is_some() {
            self.pos = start; // Reset if we want to handle this in parser instead
            None // For now, just strip underscores and return basic integer
        } else {
            None
        }
    }

    fn read_float_suffix(&mut self) -> FloatType {
        let start = self.pos;
        
        // Skip underscore before suffix (e.g., 42.0_f64)
        if self.peek() == '_' {
            self.bump();
        }
        
        match (self.peek(), self.peek_next()) {
            ('f', '3') if self.chars.get(self.pos + 2) == Some(&'2') => {
                self.bump(); self.bump(); self.bump();
                FloatType::Float32
            }
            ('f', '6') if self.chars.get(self.pos + 2) == Some(&'4') => {
                self.bump(); self.bump(); self.bump();
                FloatType::Float64
            }
            _ => FloatType::Float64,
        }
    }

    fn read_hex_int(&mut self, _start: usize) -> Result<Token, Error> {
        self.bump(); // 0
        self.bump(); // x
        while self.peek().is_ascii_hexdigit() || self.peek() == '_' {
            self.bump();
        }
        let text: String = self.chars[_start..self.pos].iter().filter(|c| **c != '_').collect();
        let value = i64::from_str_radix(&text[2..], 16).unwrap_or(0);
        Ok(Token::new(TokenKind::Integer(value), self.mk_span()))
    }

    fn read_octal_int(&mut self, _start: usize) -> Result<Token, Error> {
        self.bump(); // 0
        self.bump(); // o
        while self.peek().is_ascii_digit() && self.peek() != '8' && self.peek() != '9' || self.peek() == '_' {
            self.bump();
        }
        let text: String = self.chars[_start..self.pos].iter().filter(|c| **c != '_').collect();
        let value = i64::from_str_radix(&text[2..], 8).unwrap_or(0);
        Ok(Token::new(TokenKind::Integer(value), self.mk_span()))
    }

    fn read_binary_int(&mut self, _start: usize) -> Result<Token, Error> {
        self.bump(); // 0
        self.bump(); // b
        while self.peek() == '0' || self.peek() == '1' || self.peek() == '_' {
            self.bump();
        }
        let text: String = self.chars[_start..self.pos].iter().filter(|c| **c != '_').collect();
        let value = i64::from_str_radix(&text[2..], 2).unwrap_or(0);
        Ok(Token::new(TokenKind::Integer(value), self.mk_span()))
    }

    fn read_ident_or_keyword(&mut self) -> Token {
        let start = self.pos;
        while matches!(self.peek(), 'a'..='z' | 'A'..='Z' | '_' | '0'..='9') {
            self.bump();
        }

        let text: String = self.chars[start..self.pos].iter().collect();

        if text == "true" {
            return Token::new(TokenKind::Bool(true), self.mk_span());
        }
        if text == "false" {
            return Token::new(TokenKind::Bool(false), self.mk_span());
        }

        if let Some(keyword) = Keyword::from_str(&text) {
            return Token::new(TokenKind::Keyword(keyword), self.mk_span());
        }

        Token::new(TokenKind::Ident(text), self.mk_span())
    }

    fn read_plus(&mut self) -> Token {
        self.bump();
        match self.peek() {
            '+' => {
                self.bump();
                Token::new(TokenKind::PlusPlus, self.mk_span())
            }
            '=' => {
                self.bump();
                Token::new(TokenKind::PlusEquals, self.mk_span())
            }
            _ => Token::new(TokenKind::Plus, self.mk_span()),
        }
    }

    fn read_minus(&mut self) -> Token {
        self.bump();
        match self.peek() {
            '-' => {
                self.bump();
                Token::new(TokenKind::MinusMinus, self.mk_span())
            }
            '=' => {
                self.bump();
                Token::new(TokenKind::MinusEquals, self.mk_span())
            }
            '>' => {
                self.bump();
                Token::new(TokenKind::ThinArrow, self.mk_span())
            }
            _ => Token::new(TokenKind::Minus, self.mk_span()),
        }
    }

    fn read_star(&mut self) -> Token {
        self.bump();
        if self.peek() == '=' {
            self.bump();
            Token::new(TokenKind::StarEquals, self.mk_span())
        } else {
            Token::new(TokenKind::Star, self.mk_span())
        }
    }

    fn read_percent(&mut self) -> Token {
        self.bump();
        if self.peek() == '=' {
            self.bump();
            Token::new(TokenKind::PercentEquals, self.mk_span())
        } else {
            Token::new(TokenKind::Percent, self.mk_span())
        }
    }

    fn read_equals(&mut self) -> Token {
        self.bump();
        match self.peek() {
            '=' => {
                self.bump();
                Token::new(TokenKind::EqualsEquals, self.mk_span())
            }
            '>' => {
                self.bump();
                Token::new(TokenKind::FatArrow, self.mk_span())
            }
            _ => Token::new(TokenKind::Equals, self.mk_span()),
        }
    }

    fn read_bang(&mut self) -> Token {
        self.bump();
        match self.peek() {
            '=' => {
                self.bump();
                Token::new(TokenKind::BangEquals, self.mk_span())
            }
            '?' => {
                self.bump();
                Token::new(TokenKind::QuestionQuestion, self.mk_span())
            }
            _ => Token::new(TokenKind::Bang, self.mk_span()),
        }
    }

    fn read_less(&mut self) -> Token {
        self.bump();
        match self.peek() {
            '=' => {
                self.bump();
                Token::new(TokenKind::LessEquals, self.mk_span())
            }
            '<' => {
                self.bump();
                if self.peek() == '=' {
                    self.bump();
                    Token::new(TokenKind::LessLessEquals, self.mk_span())
                } else {
                    Token::new(TokenKind::LessLess, self.mk_span())
                }
            }
            _ => Token::new(TokenKind::Less, self.mk_span()),
        }
    }

    fn read_greater(&mut self) -> Token {
        self.bump();
        match self.peek() {
            '=' => {
                self.bump();
                Token::new(TokenKind::GreaterEquals, self.mk_span())
            }
            '>' => {
                self.bump();
                if self.peek() == '=' {
                    self.bump();
                    Token::new(TokenKind::GreaterGreaterGreaterEquals, self.mk_span())
                } else if self.peek() == '>' {
                    self.bump();
                    Token::new(TokenKind::GreaterGreaterGreater, self.mk_span())
                } else {
                    Token::new(TokenKind::GreaterGreater, self.mk_span())
                }
            }
            _ => Token::new(TokenKind::Greater, self.mk_span()),
        }
    }

    fn read_ampersand(&mut self) -> Token {
        self.bump();
        match self.peek() {
            '&' => {
                self.bump();
                Token::new(TokenKind::AmpAmp, self.mk_span())
            }
            '=' => {
                self.bump();
                Token::new(TokenKind::AmpersandEquals, self.mk_span())
            }
            _ => Token::new(TokenKind::Ampersand, self.mk_span()),
        }
    }

    fn read_bar(&mut self) -> Token {
        self.bump();
        match self.peek() {
            '|' => {
                self.bump();
                Token::new(TokenKind::BarBar, self.mk_span())
            }
            '=' => {
                self.bump();
                Token::new(TokenKind::BarEquals, self.mk_span())
            }
            _ => Token::new(TokenKind::Bar, self.mk_span()),
        }
    }

    fn read_caret(&mut self) -> Token {
        self.bump();
        if self.peek() == '=' {
            self.bump();
            Token::new(TokenKind::CaretEquals, self.mk_span())
        } else {
            Token::new(TokenKind::Caret, self.mk_span())
        }
    }

    fn read_dot(&mut self) -> Token {
        self.bump();
        match self.peek() {
            '.' => {
                self.bump();
                if self.peek() == '.' {
                    self.bump();
                    Token::new(TokenKind::DotDotDot, self.mk_span())
                } else {
                    Token::new(TokenKind::DotDot, self.mk_span())
                }
            }
            _ => Token::new(TokenKind::Dot, self.mk_span()),
        }
    }

    fn read_colon(&mut self) -> Token {
        self.bump();
        if self.peek() == ':' {
            self.bump();
            Token::new(TokenKind::ColonColon, self.mk_span())
        } else if self.peek() == '=' {
            self.bump();
            Token::new(TokenKind::FatArrow, self.mk_span())
        } else {
            Token::new(TokenKind::Colon, self.mk_span())
        }
    }

    fn read_question(&mut self) -> Token {
        self.bump();
        match self.peek() {
            '?' => {
                self.bump();
                Token::new(TokenKind::QuestionQuestion, self.mk_span())
            }
            ':' => {
                self.bump();
                Token::new(TokenKind::QuestionColon, self.mk_span())
            }
            _ => Token::new(TokenKind::Question, self.mk_span()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lexer_integer() {
        let mut lexer = Lexer::new("42");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Integer(42)));
    }

    #[test]
    fn test_lexer_keywords() {
        let mut lexer = Lexer::new("class fn if else while return");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Keyword(Keyword::Class)));
        assert!(matches!(tokens[1].kind, TokenKind::Keyword(Keyword::Fn)));
        assert!(matches!(tokens[2].kind, TokenKind::Keyword(Keyword::If)));
    }

    #[test]
    fn test_lexer_string() {
        let mut lexer = Lexer::new("\"hello world\"");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(&tokens[0].kind, TokenKind::String(s) if s == "hello world"));
    }

    #[test]
    fn test_lexer_comment() {
        let mut lexer = Lexer::new("// this is a comment\n42");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Integer(42)));
    }

    #[test]
    fn test_lexer_doc_comment() {
        let mut lexer = Lexer::new("/** doc comment */\n42");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(&tokens[0].kind, TokenKind::DocComment(s) if s.contains("doc comment")));
        // Find the integer token (skip whitespace/newline tokens)
        let int_token = tokens.iter().find(|t| matches!(t.kind, TokenKind::Integer(_))).unwrap();
        assert!(matches!(int_token.kind, TokenKind::Integer(42)));
    }

    #[test]
    fn test_lexer_doc_comment_multiline() {
        let mut lexer = Lexer::new("/**\n * line 1\n * line 2\n */\nclass Foo");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(&tokens[0].kind, TokenKind::DocComment(_)));
        if let TokenKind::DocComment(s) = &tokens[0].kind {
            eprintln!("DocComment text: [{}]", s);
            for (i, c) in s.chars().enumerate() {
                eprintln!("  char {}: {:?}", i, c);
            }
        }
    }

    #[test]
    fn test_lexer_raw_string() {
        let mut lexer = Lexer::new(r#"r"hello world""#);
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(&tokens[0].kind, TokenKind::RawString(s) if s == "hello world"));
    }

    #[test]
    fn test_lexer_raw_string_with_hashes() {
        let mut lexer = Lexer::new(r##"r#"hello "world""#"##);
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(&tokens[0].kind, TokenKind::RawString(s) if s == r#"hello "world""#));
    }

    #[test]
    fn test_lexer_digit_separator() {
        let mut lexer = Lexer::new("1_000_000");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Integer(1000000)));
    }

    #[test]
    fn test_lexer_hex_with_separator() {
        let mut lexer = Lexer::new("0xFF_FF");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Integer(65535)));
    }

    #[test]
    fn test_lexer_byte_literal() {
        let mut lexer = Lexer::new("b'x'");
        let tokens = lexer.tokenize().unwrap();
        println!("First token: {:?}", tokens[0]);
        assert!(matches!(tokens[0].kind, TokenKind::Byte(120))); // 'x' = 120
    }

    #[test]
    fn test_lexer_escape_sequences() {
        let mut lexer = Lexer::new(r#""hello\nworld""#);
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(&tokens[0].kind, TokenKind::String(s) if s == "hello\nworld"));
    }

    #[test]
    fn test_lexer_hex_escape() {
        let mut lexer = Lexer::new(r#""\x48\x65\x6c\x6c\x6f""#);
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(&tokens[0].kind, TokenKind::String(s) if s == "Hello"));
    }
}
