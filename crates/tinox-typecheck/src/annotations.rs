use std::collections::{HashMap, HashSet};
use tinox_common::{Error, Span};
use tinox_parser::{Annotation, Class, DeclKind, Function, Method, Namespace};

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
}

#[derive(Debug, Clone, Default)]
pub struct AnnotationProcessingResult {
    pub route_entries: Vec<RouteInfo>,
    pub inline_functions: HashSet<String>,
    pub inline_methods: HashSet<(String, String)>,
    pub deprecated_warnings: Vec<String>,
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
                _ => {}
            }
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
            if let DeclKind::Class(c) = &inner.node {
                self.process_class_annotations(c, result);
            } else if let DeclKind::Function(f) = &inner.node {
                self.process_function_annotations(f, result);
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

pub fn validate_annotations(
    source: &tinox_parser::SourceFile,
) -> Vec<Error> {
    let processor = AnnotationProcessor::new();
    let mut errors = Vec::new();

    for decl in &source.decls {
        match &decl.node {
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
                    match &inner.node {
                        DeclKind::Class(c) => {
                            errors.extend(processor.validate(&c.annotations, AnnotationTarget::Class));
                            for field in &c.fields {
                                errors.extend(processor.validate(&field.annotations, AnnotationTarget::Field));
                            }
                            for method in &c.methods {
                                errors.extend(processor.validate(&method.annotations, AnnotationTarget::Method));
                            }
                        }
                        DeclKind::Function(f) => {
                            errors.extend(processor.validate(&f.annotations, AnnotationTarget::Function));
                        }
                        DeclKind::Interface(i) => {
                            errors.extend(processor.validate(&i.annotations, AnnotationTarget::Interface));
                        }
                        DeclKind::Enum(e) => {
                            errors.extend(processor.validate(&e.annotations, AnnotationTarget::Enum));
                        }
                        DeclKind::Trait(t) => {
                            errors.extend(processor.validate(&t.annotations, AnnotationTarget::Trait));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    errors
}