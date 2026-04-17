use tinox_common::{Error, ErrorBag};
use tinox_parser::{Parser, SourceFile};

pub struct TypeChecker {
    errors: Vec<Error>,
}

impl TypeChecker {
    pub fn new() -> Self {
        Self { errors: Vec::new() }
    }

    pub fn check(&mut self, source: &SourceFile) -> Result<SourceFile, ErrorBag> {
        // V1: Type checking is a pass-through
        // Full type checking will be implemented in V2
        Ok(source.clone())
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

pub fn typecheck(source: &SourceFile) -> Result<SourceFile, ErrorBag> {
    let mut checker = TypeChecker::new();
    checker.check(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tinox_lexer::Lexer;

    #[test]
    fn test_typecheck_passthrough() {
        let code = "fn main() -> Int32 { return 42; }";
        let mut lexer = Lexer::new(code);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        let ast = parser.parse().unwrap();
        let result = typecheck(&ast);
        assert!(result.is_ok());
    }
}
