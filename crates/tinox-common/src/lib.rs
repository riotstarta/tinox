use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: Pos,
    pub end: Pos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Pos {
    pub offset: u32,
    pub line: u32,
    pub column: u32,
}

impl Span {
    pub fn new(start: Pos, end: Pos) -> Self {
        Self { start, end }
    }

    pub fn dummy() -> Self {
        Self {
            start: Pos::dummy(),
            end: Pos::dummy(),
        }
    }

    pub fn merge(self, other: Span) -> Span {
        Span {
            start: if self.start.offset < other.start.offset {
                self.start
            } else {
                other.start
            },
            end: if self.end.offset > other.end.offset {
                self.end
            } else {
                other.end
            },
        }
    }
}

impl Pos {
    pub fn dummy() -> Self {
        Self {
            offset: 0,
            line: 1,
            column: 1,
        }
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}",
            self.start.line, self.start.column, self.end.offset
        )
    }
}

#[derive(Debug, Clone)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: Span) -> Self {
        Self { node, span }
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Spanned<U> {
        Spanned {
            node: f(self.node),
            span: self.span,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Error {
    pub span: Span,
    pub message: String,
}

impl Error {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }

    pub fn new_at(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: error: {}", self.span, self.message)
    }
}

impl std::error::Error for Error {}

#[derive(Debug)]
pub struct ErrorBag {
    pub errors: Vec<Error>,
}

impl ErrorBag {
    pub fn new() -> Self {
        Self { errors: Vec::new() }
    }

    pub fn push(&mut self, error: Error) {
        self.errors.push(error);
    }

    pub fn extend(&mut self, other: ErrorBag) {
        self.errors.extend(other.errors);
    }

    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn check<T>(result: Result<T, Error>) -> Result<T, ErrorBag> {
        result.map_err(|e| {
            let mut bag = ErrorBag::new();
            bag.push(e);
            bag
        })
    }
}

impl Default for ErrorBag {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ErrorBag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for error in &self.errors {
            writeln!(f, "{}", error)?;
        }
        Ok(())
    }
}

impl std::error::Error for ErrorBag {}

pub trait ToSpanned {
    fn to_span(self, span: Span) -> Spanned<Self>
    where
        Self: Sized;
}

impl<T> ToSpanned for T {
    fn to_span(self, span: Span) -> Spanned<Self>
    where
        Self: Sized,
    {
        Spanned::new(self, span)
    }
}
