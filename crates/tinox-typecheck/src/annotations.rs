use std::collections::{HashMap, HashSet};
use tinox_common::{Error, Span};
use tinox_parser::{Annotation, Class, DeclKind, FieldDef, Function, Method, Namespace, Type};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AnnotationTarget {
    Function,
    Method,
    Class,
    Field,
    Interface,
    Enum,
    Trait,
    Namespace,
}

#[derive(Debug, Clone)]
pub struct AnnotationInfo {
    pub name: String,
    pub valid_targets: Vec<AnnotationTarget>,
    pub min_args: usize,
    pub max_args: usize,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct ProcessedAnnotation {
    pub name: String,
    pub args: Vec<tinox_parser::Literal>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct RouteInfo {
    pub method: String,
    pub path: String,
    pub class_name: String,
    pub method_name: String,
    pub status_code: Option<i64>,
    pub produces: Option<String>,
    pub consumes: Option<String>,
    pub auth_type: Option<String>,
    pub is_static: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DiScope {
    Application,
    Startup,
    HttpRequest,
}

#[derive(Debug, Clone)]
pub struct DiInjectField {
    pub field_name: String,
    pub field_type: String,
}

#[derive(Debug, Clone)]
pub struct DiComponentInfo {
    pub class_name: String,
    pub scope: DiScope,
    pub inject_fields: Vec<DiInjectField>,
}

#[derive(Debug, Clone, Default)]
pub struct AnnotationProcessingResult {
    pub route_entries: Vec<RouteInfo>,
    pub inline_functions: HashSet<String>,
    pub inline_methods: HashSet<(String, String)>,
    pub deprecated_warnings: Vec<String>,
    pub custom_annotation_names: Vec<String>,
    pub di_components: Vec<DiComponentInfo>,
    pub log_classes: HashSet<String>,
}

pub struct AnnotationProcessor {
    registry: HashMap<String, AnnotationInfo>,
}

impl AnnotationProcessor {
    pub fn new() -> Self {
        let mut registry: HashMap<String, AnnotationInfo> = HashMap::new();

        // HTTP method annotations
        registry.insert(
            "GET".to_string(),
            AnnotationInfo {
                name: "GET".to_string(),
                valid_targets: vec![AnnotationTarget::Method],
                min_args: 0,
                max_args: 1,
                description: "Marks a method as a GET endpoint. Optional path arg, or use @Path.".to_string(),
            },
        );
        registry.insert(
            "POST".to_string(),
            AnnotationInfo {
                name: "POST".to_string(),
                valid_targets: vec![AnnotationTarget::Method],
                min_args: 0,
                max_args: 1,
                description: "Marks a method as a POST endpoint. Optional path arg, or use @Path.".to_string(),
            },
        );
        registry.insert(
            "PUT".to_string(),
            AnnotationInfo {
                name: "PUT".to_string(),
                valid_targets: vec![AnnotationTarget::Method],
                min_args: 0,
                max_args: 1,
                description: "Marks a method as a PUT endpoint. Optional path arg, or use @Path.".to_string(),
            },
        );
        registry.insert(
            "PATCH".to_string(),
            AnnotationInfo {
                name: "PATCH".to_string(),
                valid_targets: vec![AnnotationTarget::Method],
                min_args: 0,
                max_args: 1,
                description: "Marks a method as a PATCH endpoint. Optional path arg, or use @Path.".to_string(),
            },
        );
        registry.insert(
            "DELETE".to_string(),
            AnnotationInfo {
                name: "DELETE".to_string(),
                valid_targets: vec![AnnotationTarget::Method],
                min_args: 0,
                max_args: 1,
                description: "Marks a method as a DELETE endpoint. Optional path arg, or use @Path.".to_string(),
            },
        );

        // REST framework annotations
        registry.insert(
            "Path".to_string(),
            AnnotationInfo {
                name: "Path".to_string(),
                valid_targets: vec![AnnotationTarget::Class, AnnotationTarget::Method],
                min_args: 1,
                max_args: 1,
                description: "Sets the URL path prefix for a controller or route".to_string(),
            },
        );
        registry.insert(
            "Produces".to_string(),
            AnnotationInfo {
                name: "Produces".to_string(),
                valid_targets: vec![AnnotationTarget::Method],
                min_args: 1,
                max_args: 1,
                description: "Specifies the response content type".to_string(),
            },
        );
        registry.insert(
            "Consumes".to_string(),
            AnnotationInfo {
                name: "Consumes".to_string(),
                valid_targets: vec![AnnotationTarget::Method],
                min_args: 1,
                max_args: 1,
                description: "Specifies the accepted request content type".to_string(),
            },
        );
        registry.insert(
            "StatusCode".to_string(),
            AnnotationInfo {
                name: "StatusCode".to_string(),
                valid_targets: vec![AnnotationTarget::Method],
                min_args: 1,
                max_args: 1,
                description: "Sets the default HTTP status code for the response".to_string(),
            },
        );
        registry.insert(
            "Auth".to_string(),
            AnnotationInfo {
                name: "Auth".to_string(),
                valid_targets: vec![AnnotationTarget::Method, AnnotationTarget::Class],
                min_args: 1,
                max_args: 1,
                description: "Requires authentication (\"bearer\" or \"basic\")".to_string(),
            },
        );
        registry.insert(
            "annotation".to_string(),
            AnnotationInfo {
                name: "annotation".to_string(),
                valid_targets: vec![AnnotationTarget::Class],
                min_args: 0,
                max_args: 0,
                description: "Marks a class as an annotation definition".to_string(),
            },
        );

        // Logging annotation
        registry.insert(
            "Log".to_string(),
            AnnotationInfo {
                name: "Log".to_string(),
                valid_targets: vec![AnnotationTarget::Class],
                min_args: 0,
                max_args: 0,
                description: "Injects a 'log: Logger' field initialized with Logger::new(ClassName)".to_string(),
            },
        );

        // DI scope annotations
        registry.insert(
            "ApplicationComponent".to_string(),
            AnnotationInfo {
                name: "ApplicationComponent".to_string(),
                valid_targets: vec![AnnotationTarget::Class],
                min_args: 0,
                max_args: 0,
                description: "Lazy singleton — one instance for the lifetime of the application".to_string(),
            },
        );
        registry.insert(
            "Startup".to_string(),
            AnnotationInfo {
                name: "Startup".to_string(),
                valid_targets: vec![AnnotationTarget::Class],
                min_args: 0,
                max_args: 0,
                description: "Eager singleton — created immediately at application startup".to_string(),
            },
        );
        registry.insert(
            "HttpRequestScoped".to_string(),
            AnnotationInfo {
                name: "HttpRequestScoped".to_string(),
                valid_targets: vec![AnnotationTarget::Class],
                min_args: 0,
                max_args: 0,
                description: "One instance per HTTP request, lives as long as the request".to_string(),
            },
        );
        registry.insert(
            "Inject".to_string(),
            AnnotationInfo {
                name: "Inject".to_string(),
                valid_targets: vec![AnnotationTarget::Field],
                min_args: 0,
                max_args: 0,
                description: "Marks a field for compile-time dependency injection".to_string(),
            },
        );

        // Compiler annotations
        registry.insert(
            "inline".to_string(),
            AnnotationInfo {
                name: "inline".to_string(),
                valid_targets: vec![AnnotationTarget::Function, AnnotationTarget::Method],
                min_args: 0,
                max_args: 1,
                description: "Hints that the function should be inlined".to_string(),
            },
        );
        registry.insert(
            "deprecated".to_string(),
            AnnotationInfo {
                name: "deprecated".to_string(),
                valid_targets: vec![
                    AnnotationTarget::Function,
                    AnnotationTarget::Method,
                    AnnotationTarget::Class,
                ],
                min_args: 0,
                max_args: 1,
                description: "Marks the declaration as deprecated".to_string(),
            },
        );

        Self { registry }
    }

    pub fn register_custom_annotation(&mut self, name: &str) {
        self.registry.insert(
            name.to_string(),
            AnnotationInfo {
                name: name.to_string(),
                valid_targets: vec![
                    AnnotationTarget::Function,
                    AnnotationTarget::Method,
                    AnnotationTarget::Class,
                    AnnotationTarget::Field,
                    AnnotationTarget::Interface,
                    AnnotationTarget::Enum,
                    AnnotationTarget::Trait,
                    AnnotationTarget::Namespace,
                ],
                min_args: 0,
                max_args: usize::MAX,
                description: format!("User-defined annotation @{}", name),
            },
        );
    }

    pub fn validate(&self, annotations: &[Annotation], target: AnnotationTarget) -> Vec<Error> {
        let mut errors = Vec::new();
        for ann in annotations {
            match self.registry.get(&ann.name) {
                Some(info) => {
                    if !info.valid_targets.contains(&target) {
                        errors.push(Error::new(
                            ann.span,
                            format!(
                                "@{} cannot be applied to {:?} (valid targets: {})",
                                ann.name,
                                target,
                                info.valid_targets
                                    .iter()
                                    .map(|t| format!("{:?}", t))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        ));
                    }
                    if ann.args.len() < info.min_args {
                        errors.push(Error::new(
                            ann.span,
                            format!(
                                "@{} requires at least {} argument(s), found {}",
                                ann.name,
                                info.min_args,
                                ann.args.len()
                            ),
                        ));
                    }
                    if ann.args.len() > info.max_args {
                        errors.push(Error::new(
                            ann.span,
                            format!(
                                "@{} accepts at most {} argument(s), found {}",
                                ann.name,
                                info.max_args,
                                ann.args.len()
                            ),
                        ));
                    }
                }
                None => {
                    errors.push(Error::new(
                        ann.span,
                        format!("unknown annotation: @{}", ann.name),
                    ));
                }
            }
        }
        errors
    }

    pub fn process_source(
        &self,
        source: &tinox_parser::SourceFile,
    ) -> AnnotationProcessingResult {
        let mut result = AnnotationProcessingResult::default();

        for decl in &source.decls {
            match &decl.node {
                DeclKind::Class(c) => {
                    self.process_class_annotations(c, &mut result);
                }
                DeclKind::Function(f) => {
                    self.process_function_annotations(f, &mut result);
                }
                DeclKind::Namespace(ns) => {
                    self.process_namespace_annotations(ns, &mut result);
                }
                _ => {}
            }
        }

        result
    }

    fn process_class_annotations(
        &self,
        class: &Class,
        result: &mut AnnotationProcessingResult,
    ) {
        let mut class_base_path: Option<String> = None;
        let mut class_auth: Option<String> = None;
        let mut di_scope: Option<DiScope> = None;

        for ann in &class.annotations {
            match ann.name.as_str() {
                "Path" => {
                    if let Some(tinox_parser::Literal::String(s)) = ann.args.first() {
                        class_base_path = Some(s.clone());
                    }
                }
                "Auth" => {
                    if let Some(tinox_parser::Literal::String(s)) = ann.args.first() {
                        class_auth = Some(s.clone());
                    }
                }
                "deprecated" => {
                    let msg = if let Some(tinox_parser::Literal::String(s)) = ann.args.first() {
                        format!("class '{}' is deprecated: {}", class.name, s)
                    } else {
                        format!("class '{}' is deprecated", class.name)
                    };
                    result.deprecated_warnings.push(msg);
                }
                "annotation" => {
                    result.custom_annotation_names.push(class.name.clone());
                }
                "ApplicationComponent" => di_scope = Some(DiScope::Application),
                "Startup" => di_scope = Some(DiScope::Startup),
                "HttpRequestScoped" => di_scope = Some(DiScope::HttpRequest),
                "Log" => {
                    result.log_classes.insert(class.name.clone());
                }
                _ => {}
            }
        }

        if let Some(scope) = di_scope {
            let inject_fields = collect_inject_fields(&class.fields);
            result.di_components.push(DiComponentInfo {
                class_name: class.name.clone(),
                scope,
                inject_fields,
            });
        }

        for method in &class.methods {
            let route = self.extract_route_from_method(
                method,
                &class.name,
                class_base_path.as_deref(),
                class_auth.as_deref(),
            );
            if let Some(route) = route {
                result.route_entries.push(route);
            }

            for ann in &method.annotations {
                match ann.name.as_str() {
                    "inline" => {
                        result
                            .inline_methods
                            .insert((class.name.clone(), method.name.clone()));
                    }
                    "deprecated" => {
                        let msg = if let Some(tinox_parser::Literal::String(s)) = ann.args.first() {
                            format!("method '{}.{}' is deprecated: {}", class.name, method.name, s)
                        } else {
                            format!("method '{}.{}' is deprecated", class.name, method.name)
                        };
                        result.deprecated_warnings.push(msg);
                    }
                    _ => {}
                }
            }
        }
    }

    fn process_function_annotations(
        &self,
        f: &Function,
        result: &mut AnnotationProcessingResult,
    ) {
        for ann in &f.annotations {
            if ann.name == "inline" {
                result.inline_functions.insert(f.name.clone());
            }
            if ann.name == "deprecated" {
                let msg = if let Some(tinox_parser::Literal::String(s)) = ann.args.first() {
                    format!("function '{}' is deprecated: {}", f.name, s)
                } else {
                    format!("function '{}' is deprecated", f.name)
                };
                result.deprecated_warnings.push(msg);
            }
        }
    }

    fn process_namespace_annotations(
        &self,
        ns: &Namespace,
        result: &mut AnnotationProcessingResult,
    ) {
        for inner in &ns.decls {
            match &inner.node {
                DeclKind::Class(c) => self.process_class_annotations(c, result),
                DeclKind::Function(f) => self.process_function_annotations(f, result),
                DeclKind::Namespace(nested) => self.process_namespace_annotations(nested, result),
                _ => {}
            }
        }
    }

    fn extract_route_from_method(
        &self,
        method: &Method,
        class_name: &str,
        class_base_path: Option<&str>,
        class_auth: Option<&str>,
    ) -> Option<RouteInfo> {
        let mut http_method: Option<String> = None;
        let mut method_path: Option<String> = None;
        let mut status_code: Option<i64> = None;
        let mut produces: Option<String> = None;
        let mut consumes: Option<String> = None;
        let mut auth: Option<String> = class_auth.map(|s| s.to_string());

        for ann in &method.annotations {
            match ann.name.as_str() {
                "GET" | "POST" | "PUT" | "PATCH" | "DELETE" => {
                    http_method = Some(ann.name.clone());
                    if let Some(tinox_parser::Literal::String(s)) = ann.args.first() {
                        method_path = Some(s.clone());
                    }
                }
                "Path" => {
                    if let Some(tinox_parser::Literal::String(s)) = ann.args.first() {
                        method_path = Some(s.clone());
                    }
                }
                "StatusCode" => {
                    if let Some(tinox_parser::Literal::Integer(n)) = ann.args.first() {
                        status_code = Some(*n);
                    }
                }
                "Produces" => {
                    if let Some(tinox_parser::Literal::String(s)) = ann.args.first() {
                        produces = Some(s.clone());
                    }
                }
                "Consumes" => {
                    if let Some(tinox_parser::Literal::String(s)) = ann.args.first() {
                        consumes = Some(s.clone());
                    }
                }
                "Auth" => {
                    if let Some(tinox_parser::Literal::String(s)) = ann.args.first() {
                        auth = Some(s.clone());
                    }
                }
                _ => {}
            }
        }

        let m = http_method?;
        let p = method_path.unwrap_or_default();
        let full_path = match class_base_path {
            Some(base) => {
                if p.is_empty() {
                    base.to_string()
                } else if base.ends_with('/') && p.starts_with('/') {
                    format!("{}{}", &base[..base.len() - 1], p)
                } else if base.ends_with('/') || p.starts_with('/') {
                    format!("{}{}", base, p)
                } else {
                    format!("{}/{}", base, p)
                }
            }
            None => p,
        };

        Some(RouteInfo {
            method: m,
            path: full_path,
            class_name: class_name.to_string(),
            method_name: method.name.clone(),
            status_code,
            produces,
            consumes,
            auth_type: auth,
            is_static: method.static_,
        })
    }
}

impl Default for AnnotationProcessor {
    fn default() -> Self {
        Self::new()
    }
}

pub fn process_annotations(
    source: &tinox_parser::SourceFile,
) -> AnnotationProcessingResult {
    let processor = AnnotationProcessor::new();
    processor.process_source(source)
}

fn collect_inject_fields(fields: &[FieldDef]) -> Vec<DiInjectField> {
    fields
        .iter()
        .filter(|f| f.annotations.iter().any(|a| a.name == "Inject"))
        .filter_map(|f| {
            if let Type::Named(type_name) = &f.field_type {
                Some(DiInjectField {
                    field_name: f.name.clone(),
                    field_type: type_name.clone(),
                })
            } else {
                None
            }
        })
        .collect()
}

fn collect_custom_annotation_classes(decl: &DeclKind, processor: &mut AnnotationProcessor) {
    match decl {
        DeclKind::Class(c) => {
            if c.annotations.iter().any(|a| a.name == "annotation") {
                processor.register_custom_annotation(&c.name);
            }
        }
        DeclKind::Namespace(ns) => {
            for inner in &ns.decls {
                collect_custom_annotation_classes(&inner.node, processor);
            }
        }
        _ => {}
    }
}

fn validate_decl(processor: &AnnotationProcessor, decl: &DeclKind, errors: &mut Vec<Error>) {
    match decl {
        DeclKind::Function(f) => {
            errors.extend(processor.validate(&f.annotations, AnnotationTarget::Function));
        }
        DeclKind::Class(c) => {
            errors.extend(processor.validate(&c.annotations, AnnotationTarget::Class));
            for field in &c.fields {
                errors.extend(processor.validate(&field.annotations, AnnotationTarget::Field));
            }
            for method in &c.methods {
                errors.extend(processor.validate(&method.annotations, AnnotationTarget::Method));
            }
        }
        DeclKind::Interface(i) => {
            errors.extend(processor.validate(&i.annotations, AnnotationTarget::Interface));
            for method in &i.methods {
                errors.extend(processor.validate(&method.annotations, AnnotationTarget::Method));
            }
        }
        DeclKind::Enum(e) => {
            errors.extend(processor.validate(&e.annotations, AnnotationTarget::Enum));
        }
        DeclKind::Trait(t) => {
            errors.extend(processor.validate(&t.annotations, AnnotationTarget::Trait));
            for method in &t.methods {
                errors.extend(processor.validate(&method.annotations, AnnotationTarget::Method));
            }
        }
        DeclKind::Namespace(ns) => {
            errors.extend(processor.validate(&ns.annotations, AnnotationTarget::Namespace));
            for inner in &ns.decls {
                validate_decl(processor, &inner.node, errors);
            }
        }
        _ => {}
    }
}

pub fn validate_annotations(
    source: &tinox_parser::SourceFile,
) -> Vec<Error> {
    let mut processor = AnnotationProcessor::new();

    // First pass: register all @annotation-class definitions so they are valid in the second pass
    for decl in &source.decls {
        collect_custom_annotation_classes(&decl.node, &mut processor);
    }

    let mut errors = Vec::new();
    for decl in &source.decls {
        validate_decl(&processor, &decl.node, &mut errors);
    }
    errors
}