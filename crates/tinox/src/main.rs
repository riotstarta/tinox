use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tinox_codegen::CodeGen;
use tinox_common;
use tinox_lexer::Lexer;
use tinox_parser::{DeclKind, Formatter, Parser};

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_help();
        return;
    }

    match args[1].as_str() {
        "build" => build(&args[2..]),
        "run" => run_file(&args[2..]),
        "check" => check(&args[2..]),
        "fmt" => fmt(&args[2..]),
        "help" | "--help" | "-h" => print_help(),
        _ => {
            eprintln!("Unknown command: {}", args[1]);
            print_help();
        }
    }
}

fn print_help() {
    println!("Tinox Compiler v{}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("Usage:");
    println!("  tinox build <file>         Compile a Tinox file to an executable");
    println!("  tinox run <file>           Compile and run a Tinox file");
    println!("  tinox check <file>         Type-check a Tinox file without compiling");
    println!("  tinox fmt <file>           Format a Tinox file (print to stdout)");
    println!("  tinox fmt --write <file>   Format a Tinox file in place");
    println!("  tinox help                 Show this help message");
}

fn fmt(args: &[String]) {
    let (write_mode, file_arg) = if args.first().map(|s| s.as_str()) == Some("--write") {
        (true, args.get(1))
    } else {
        (false, args.first())
    };

    let input_file = match file_arg {
        Some(f) => f,
        None => {
            eprintln!("Error: No input file specified");
            return;
        }
    };

    let source = match fs::read_to_string(input_file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read '{}': {}", input_file, e);
            return;
        }
    };

    let mut lexer = Lexer::new(&source);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        Err(errors) => {
            eprintln!("Lex error: {:?}", errors);
            return;
        }
    };

    let mut parser = Parser::new(tokens);
    let ast = match parser.parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Parse error: {:?}", e);
            return;
        }
    };

    let mut formatter = Formatter::new();
    let formatted = formatter.format(&ast);

    if write_mode {
        if let Err(e) = fs::write(input_file, &formatted) {
            eprintln!("error: cannot write '{}': {}", input_file, e);
        }
    } else {
        print!("{}", formatted);
    }
}

fn build(args: &[String]) {
    if args.is_empty() {
        eprintln!("Error: No input file specified");
        return;
    }

    let input_file = &args[0];
    let output_name = parse_output_flag(&args[1..]).unwrap_or_else(|| "a.out".to_string());

    match compile_file(input_file, &output_name) {
        Ok(_) => println!("Compiled successfully: {}", output_name),
        Err(e) => eprintln!("Compilation failed: {}", e),
    }
}

fn parse_output_flag(args: &[String]) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == "-o" {
            return args.get(i + 1).cloned();
        }
        i += 1;
    }
    None
}

fn run_file(args: &[String]) {
    if args.is_empty() {
        eprintln!("Error: No input file specified");
        return;
    }

    let input_file = &args[0];
    let exe_name = format!(".tinox_tmp_{}", std::process::id());

    match compile_file(input_file, &exe_name) {
        Ok(_) => {
            let status = Command::new(&format!("./{}", exe_name))
                .status()
                .expect("Failed to run executable");

            let _ = fs::remove_file(&exe_name);

            std::process::exit(status.code().unwrap_or(1));
        }
        Err(e) => eprintln!("Compilation failed: {}", e),
    }
}

fn check(args: &[String]) {
    if args.is_empty() {
        eprintln!("Error: No input file specified");
        std::process::exit(1);
    }

    let input_file = &args[0];
    let source = match fs::read_to_string(input_file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read '{}': {}", input_file, e);
            std::process::exit(1);
        }
    };

    let lines: Vec<&str> = source.lines().collect();

    // Lex
    let mut lexer = Lexer::new(&source);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        Err(errors) => {
            for e in &errors {
                print_error(input_file, &lines, e.span, &e.message);
            }
            eprintln!("\naborting: {} error{}", errors.len(), if errors.len() == 1 { "" } else { "s" });
            std::process::exit(1);
        }
    };

    // Parse
    let mut parser = tinox_parser::Parser::new(tokens);
    let mut ast = match parser.parse() {
        Ok(a) => a,
        Err(bag) => {
            let count = bag.errors.len();
            for e in &bag.errors {
                print_error(input_file, &lines, e.span, &e.message);
            }
            eprintln!("\naborting: {} error{}", count, if count == 1 { "" } else { "s" });
            std::process::exit(1);
        }
    };

    // Resolve imports
    let base_dir = Path::new(input_file)
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let mut visited = HashSet::new();
    if let Ok(canonical) = Path::new(input_file).canonicalize() {
        visited.insert(canonical);
    }
    if let Err(e) = resolve_imports(&mut ast, &base_dir, &mut visited) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }

    // Type-check
    let mut typechecker = tinox_typecheck::TypeChecker::new();
    match typechecker.check(&ast) {
        Ok(_) => {
            // Also run annotation processing for check mode
            let ann_result = tinox_typecheck::annotations::process_annotations(&ast);
            for warning in &ann_result.deprecated_warnings {
                eprintln!("warning: {}", warning);
            }
            for route in &ann_result.route_entries {
                eprintln!("  route: {} {} -> {}.{}", route.method, route.path, route.class_name, route.method_name);
            }
            println!("{}: no errors", input_file);
            std::process::exit(0);
        }
        Err(bag) => {
            let count = bag.errors.len();
            for e in &bag.errors {
                print_error(input_file, &lines, e.span, &e.message);
            }
            eprintln!("\n{} error{} found", count, if count == 1 { "" } else { "s" });
            std::process::exit(1);
        }
    }
}

fn print_error(file: &str, lines: &[&str], span: tinox_common::Span, message: &str) {
    let line = span.start.line as usize;
    let col = span.start.column as usize;
    eprintln!("{}:{}:{}: error: {}", file, line, col, message);
    if line > 0 && line <= lines.len() {
        let src_line = lines[line - 1];
        eprintln!("{:>4} | {}", line, src_line);
        let padding = " ".repeat(col.saturating_sub(1));
        eprintln!("     | {}^", padding);
    }
}

/// Returns the Tinox standard library directory.
/// Checks TINOX_PATH env var first, then falls back to the path relative to this binary's
/// source location (works for `cargo run` during development).
fn stdlib_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("TINOX_PATH") {
        let pb = PathBuf::from(p);
        if pb.is_dir() {
            return Some(pb);
        }
    }
    // Compiled-in dev path: crates/tinox/../../crates/tinox-core = crates/tinox-core
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/tinox-core");
    if dev.is_dir() {
        return dev.canonicalize().ok();
    }
    None
}

fn resolve_imports(
    ast: &mut tinox_parser::SourceFile,
    base_dir: &Path,
    visited: &mut HashSet<PathBuf>,
) -> Result<(), String> {
    let imports: Vec<_> = ast
        .decls
        .iter()
        .filter_map(|d| {
            if let DeclKind::Import(i) = &d.node {
                Some(i.clone())
            } else {
                None
            }
        })
        .collect();

    for import in imports {
        // ["foo", "bar"] → "foo/bar.tnx" relative to base_dir
        let mut rel = PathBuf::new();
        for (i, seg) in import.path.iter().enumerate() {
            if i == import.path.len() - 1 {
                rel.push(format!("{}.tnx", seg));
            } else {
                rel.push(seg);
            }
        }

        // Resolution order:
        // 1. Relative to source file directory
        // 2. tinox.core.X  →  <stdlib_dir>/X.tnx
        let full_path = if let Ok(p) = base_dir.join(&rel).canonicalize() {
            p
        } else if import.path.first().map(|s| s == "tinox").unwrap_or(false) {
            // stdlib import: take the last segment as filename
            let last = import.path.last().unwrap();
            let stdlib_file = format!("{}.tnx", last);
            stdlib_dir()
                .ok_or_else(|| {
                    format!(
                        "Cannot resolve stdlib import '{}': TINOX_PATH not set and dev path not found",
                        rel.display()
                    )
                })?
                .join(&stdlib_file)
                .canonicalize()
                .map_err(|e| format!("Cannot resolve stdlib import '{}': {}", stdlib_file, e))?
        } else {
            return Err(format!("Cannot resolve import '{}': file not found", rel.display()));
        };

        if visited.contains(&full_path) {
            continue;
        }
        visited.insert(full_path.clone());

        let source = fs::read_to_string(&full_path)
            .map_err(|e| format!("Failed to read import '{}': {}", full_path.display(), e))?;

        let mut lexer = Lexer::new(&source);
        // Keep source alive for the lexer lifetime
        let tokens = lexer
            .tokenize()
            .map_err(|e| format!("Lexer error in '{}': {:?}", full_path.display(), e))?;

        let mut parser = Parser::new(tokens);
        let mut imported = parser
            .parse()
            .map_err(|e| format!("Parse error in '{}': {:?}", full_path.display(), e))?;

        let imported_dir = full_path.parent().unwrap_or(Path::new(".")).to_path_buf();
        resolve_imports(&mut imported, &imported_dir, visited)?;

        ast.decls.extend(imported.decls);
    }

    // Drop Import and Module decls — they are resolved or informational only
    ast.decls
        .retain(|d| !matches!(&d.node, DeclKind::Import(_) | DeclKind::Module(_)));

    Ok(())
}

fn compile_file(input_path: &str, output_name: &str) -> Result<(), String> {
    let source =
        fs::read_to_string(input_path).map_err(|e| format!("Failed to read file: {}", e))?;

    let mut lexer = Lexer::new(&source);
    let tokens = lexer
        .tokenize()
        .map_err(|e| format!("Lexer error: {:?}", e))?;

    let mut parser = Parser::new(tokens);
    let mut ast = parser
        .parse()
        .map_err(|e| format!("Parse error: {:?}", e))?;

    let base_dir = Path::new(input_path)
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let mut visited = HashSet::new();
    if let Ok(canonical) = Path::new(input_path).canonicalize() {
        visited.insert(canonical);
    }
    resolve_imports(&mut ast, &base_dir, &mut visited)
        .map_err(|e| format!("Import error: {}", e))?;

    let mut typechecker = tinox_typecheck::TypeChecker::new();
    typechecker
        .check(&ast)
        .map_err(|e| format!("Type error:\n{}", e))?;

    let (iface_methods, class_implements) = typechecker.interface_info();

    // Annotation processing pass
    let ann_result = tinox_typecheck::annotations::process_annotations(&ast);
    for warning in &ann_result.deprecated_warnings {
        eprintln!("warning: {}", warning);
    }
    for route in &ann_result.route_entries {
        eprintln!("  route: {} {} -> {}.{}", route.method, route.path, route.class_name, route.method_name);
    }

    let route_entries: Vec<tinox_codegen::RouteEntry> = ann_result
        .route_entries
        .iter()
        .map(|r| tinox_codegen::RouteEntry {
            http_method: r.method.clone(),
            path: r.path.clone(),
            class_name: r.class_name.clone(),
            method_name: r.method_name.clone(),
            status_code: r.status_code,
            produces: r.produces.clone(),
            consumes: r.consumes.clone(),
            auth_type: r.auth_type.clone(),
            is_static: r.is_static,
        })
        .collect();

    let di_components: Vec<tinox_codegen::DiComponentInfo> = ann_result.di_components
        .iter()
        .map(|c| tinox_codegen::DiComponentInfo {
            class_name: c.class_name.clone(),
            scope: match c.scope {
                tinox_typecheck::annotations::DiScope::Application => tinox_codegen::DiScope::Application,
                tinox_typecheck::annotations::DiScope::Startup => tinox_codegen::DiScope::Startup,
                tinox_typecheck::annotations::DiScope::HttpRequest => tinox_codegen::DiScope::HttpRequest,
            },
            inject_fields: c.inject_fields.iter().map(|f| tinox_codegen::DiInjectField {
                field_name: f.field_name.clone(),
                field_type: f.field_type.clone(),
            }).collect(),
        })
        .collect();

    let mut codegen = CodeGen::new();
    codegen.set_interface_info(iface_methods, class_implements);
    codegen.set_annotation_info(ann_result.inline_functions, ann_result.inline_methods, route_entries, di_components, ann_result.log_classes);
    codegen
        .gen(&ast)
        .map_err(|e| format!("Codegen error: {:?}", e))?;

    let ir = codegen.into_ir();
    let ir_path = format!("{}.ll", output_name);
    fs::write(&ir_path, ir).map_err(|e| format!("Failed to write IR: {}", e))?;

    compile_ll_to_exe(&ir_path, output_name)
}

fn compile_ll_to_exe(ir_path: &str, output_name: &str) -> Result<(), String> {
    let obj_path = format!("{}.o", output_name);

    // Try opt -O3 first (full mid-level + backend optimizations via mem2reg, vectorize, etc.)
    // Fall back to direct llc if opt is not available.
    let opt_available = Command::new("opt")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    let llc_input: String;
    let bc_path_opt: Option<String>;

    if opt_available {
        let bc_path = format!("{}.opt.bc", output_name);
        let opt_status = Command::new("opt")
            .args(&["-O3", "-o", &bc_path, ir_path])
            .status()
            .map_err(|e| format!("opt failed: {}", e))?;
        if !opt_status.success() {
            return Err("opt failed".to_string());
        }
        llc_input = bc_path.clone();
        bc_path_opt = Some(bc_path);
    } else {
        llc_input = ir_path.to_string();
        bc_path_opt = None;
    }

    let llc_status = Command::new("llc")
        .args(&[
            "-O3",
            "-march=x86-64",
            "-filetype=obj",
            "-o",
            &obj_path,
            &llc_input,
        ])
        .status()
        .map_err(|e| format!("llc failed: {}", e))?;

    if !llc_status.success() {
        return Err("llc failed".to_string());
    }

    if let Some(bc_path) = bc_path_opt {
        let _ = fs::remove_file(&bc_path);
    }

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let runtime_src = format!("{}/../../runtime/runtime.c", manifest_dir);
    let runtime_obj = format!("{}_runtime.o", output_name);

    let cc_status = Command::new("cc")
        .args(&["-c", &runtime_src, "-o", &runtime_obj])
        .status()
        .map_err(|e| format!("Failed to compile runtime: {}", e))?;

    if !cc_status.success() {
        return Err("Runtime compilation failed".to_string());
    }

    let link_status = Command::new("cc")
        .args(&[&obj_path, &runtime_obj, "-o", output_name, "-lm", "-lpthread", "-no-pie"])
        .status()
        .map_err(|e| format!("Failed to link: {}", e))?;

    if !link_status.success() {
        return Err("Linking failed".to_string());
    }

    let _ = fs::remove_file(&obj_path);
    let _ = fs::remove_file(&runtime_obj);

    Ok(())
}
