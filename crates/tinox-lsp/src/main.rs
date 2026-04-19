use dashmap::DashMap;
use tinox_common::Pos;
use tinox_lexer::Lexer;
use tinox_parser::Parser;
use tinox_typecheck::typecheck;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

struct Backend {
    client: Client,
    docs: DashMap<Url, String>,
}

impl Backend {
    fn new(client: Client) -> Self {
        Self {
            client,
            docs: DashMap::new(),
        }
    }

    async fn diagnose(&self, uri: Url, text: &str) {
        let diags = compile_diagnostics(text);
        self.client.publish_diagnostics(uri, diags, None).await;
    }
}

fn pos_to_lsp(p: Pos) -> Position {
    Position {
        line: p.line.saturating_sub(1),
        character: p.column.saturating_sub(1),
    }
}

fn compile_diagnostics(src: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();

    let tokens = match Lexer::new(src).tokenize() {
        Ok(t) => t,
        Err(errs) => {
            for e in errs {
                diags.push(Diagnostic {
                    range: Range {
                        start: pos_to_lsp(e.span.start),
                        end: pos_to_lsp(e.span.end),
                    },
                    severity: Some(DiagnosticSeverity::ERROR),
                    message: e.message,
                    source: Some("tinox".into()),
                    ..Default::default()
                });
            }
            return diags;
        }
    };

    let ast = match Parser::new(tokens).parse() {
        Ok(a) => a,
        Err(bag) => {
            for e in bag.errors {
                diags.push(Diagnostic {
                    range: Range {
                        start: pos_to_lsp(e.span.start),
                        end: pos_to_lsp(e.span.end),
                    },
                    severity: Some(DiagnosticSeverity::ERROR),
                    message: e.message,
                    source: Some("tinox".into()),
                    ..Default::default()
                });
            }
            return diags;
        }
    };

    if let Err(bag) = typecheck(&ast) {
        for e in bag.errors {
            diags.push(Diagnostic {
                range: Range {
                    start: pos_to_lsp(e.span.start),
                    end: pos_to_lsp(e.span.end),
                },
                severity: Some(DiagnosticSeverity::ERROR),
                message: e.message,
                source: Some("tinox".into()),
                ..Default::default()
            });
        }
    }

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

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        self.docs.insert(uri.clone(), text.clone());
        self.diagnose(uri, &text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().last() {
            let uri = params.text_document.uri;
            let text = change.text;
            self.docs.insert(uri.clone(), text.clone());
            self.diagnose(uri, &text).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(text) = self.docs.get(&uri) {
            self.diagnose(uri, &text).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.docs.remove(&params.text_document.uri);
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(|client| Backend::new(client));
    Server::new(stdin, stdout, socket).serve(service).await;
}
