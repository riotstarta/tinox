mod analysis;

use analysis::{completions, definition_at, document_symbols, hover_at, lsp_pos_to_offset, pos_to_lsp};
use dashmap::DashMap;
use tinox_common::Error;
use tinox_lexer::Lexer;
use tinox_parser::{ast::SourceFile, Parser};
use tinox_typecheck::typecheck;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

struct Backend {
    client: Client,
    docs: DashMap<Url, String>,
    asts: DashMap<Url, SourceFile>,
}

impl Backend {
    fn new(client: Client) -> Self {
        Self {
            client,
            docs: DashMap::new(),
            asts: DashMap::new(),
        }
    }

    async fn update(&self, uri: Url, text: String) {
        let diags = compile(&text, |ast| {
            self.asts.insert(uri.clone(), ast);
        });
        self.docs.insert(uri.clone(), text);
        self.client.publish_diagnostics(uri, diags, None).await;
    }
}

fn err_to_diag(e: Error) -> Diagnostic {
    Diagnostic {
        range: Range {
            start: pos_to_lsp(e.span.start),
            end: pos_to_lsp(e.span.end),
        },
        severity: Some(DiagnosticSeverity::ERROR),
        message: e.message,
        source: Some("tinox".into()),
        ..Default::default()
    }
}

// Lex → parse → typecheck; calls on_ast if parsing succeeded; returns diagnostics.
fn compile(src: &str, on_ast: impl FnOnce(SourceFile)) -> Vec<Diagnostic> {
    let tokens = match Lexer::new(src).tokenize() {
        Ok(t) => t,
        Err(errs) => return errs.into_iter().map(err_to_diag).collect(),
    };

    let ast = match Parser::new(tokens).parse() {
        Ok(a) => a,
        Err(bag) => return bag.errors.into_iter().map(err_to_diag).collect(),
    };

    let diags = match typecheck(&ast) {
        Ok(_) => vec![],
        Err(bag) => bag.errors.into_iter().map(err_to_diag).collect(),
    };

    on_ast(ast);
    diags
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".into(), " ".into()]),
                    ..Default::default()
                }),
                definition_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "tinox-lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "tinox-lsp ready")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, p: DidOpenTextDocumentParams) {
        self.update(p.text_document.uri, p.text_document.text).await;
    }

    async fn did_change(&self, p: DidChangeTextDocumentParams) {
        if let Some(change) = p.content_changes.into_iter().last() {
            self.update(p.text_document.uri, change.text).await;
        }
    }

    async fn did_save(&self, p: DidSaveTextDocumentParams) {
        if let Some(text) = p.text.clone() {
            self.update(p.text_document.uri, text).await;
        }
    }

    async fn did_close(&self, p: DidCloseTextDocumentParams) {
        self.docs.remove(&p.text_document.uri);
        self.asts.remove(&p.text_document.uri);
    }

    async fn hover(&self, p: HoverParams) -> Result<Option<Hover>> {
        let uri = &p.text_document_position_params.text_document.uri;
        let pos = p.text_document_position_params.position;

        let Some(text) = self.docs.get(uri) else {
            return Ok(None);
        };
        let Some(ast) = self.asts.get(uri) else {
            return Ok(None);
        };

        let offset = lsp_pos_to_offset(&text, pos);
        let content = hover_at(&ast, offset).unwrap_or_default();
        if content.is_empty() {
            return Ok(None);
        }

        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: format!("```tinox\n{}\n```", content),
            }),
            range: None,
        }))
    }

    async fn completion(&self, p: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &p.text_document_position.text_document.uri;
        let Some(ast) = self.asts.get(uri) else {
            return Ok(None);
        };
        Ok(Some(CompletionResponse::Array(completions(&ast))))
    }

    async fn goto_definition(
        &self,
        p: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &p.text_document_position_params.text_document.uri;
        let pos = p.text_document_position_params.position;

        let Some(text) = self.docs.get(uri) else {
            return Ok(None);
        };
        let Some(ast) = self.asts.get(uri) else {
            return Ok(None);
        };

        let offset = lsp_pos_to_offset(&text, pos);
        let loc = definition_at(&ast, uri, offset);
        Ok(loc.map(GotoDefinitionResponse::Scalar))
    }

    async fn document_symbol(
        &self,
        p: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = &p.text_document.uri;
        let Some(ast) = self.asts.get(uri) else {
            return Ok(None);
        };
        Ok(Some(DocumentSymbolResponse::Nested(document_symbols(&ast))))
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(|client| Backend::new(client));
    Server::new(stdin, stdout, socket).serve(service).await;
}
