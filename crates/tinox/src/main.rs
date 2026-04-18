use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tinox_codegen::CodeGen;
use tinox_lexer::Lexer;
use tinox_parser::{DeclKind, Parser};

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
    println!("  tinox build <file>    Compile a Tinox file to an executable");
    println!("  tinox run <file>      Compile and run a Tinox file");
    println!("  tinox check <file>    Type-check a Tinox file without compiling");
    println!("  tinox help            Show this help message");
}

fn build(args: &[String]) {
    if args.is_empty() {
        eprintln!("Error: No input file specified");
        return;
    }

    let input_file = &args[0];
    let output_name = if args.len() > 1 {
        args[1].clone()
    } else {
        "a.out".to_string()
    };

    match compile_file(input_file, &output_name) {
        Ok(_) => println!("Compiled successfully: {}", output_name),
        Err(e) => eprintln!("Compilation failed: {}", e),
    }
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
        return;
    }

    let input_file = &args[0];
    let source = match fs::read_to_string(input_file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to read file: {}", e);
            return;
        }
    };

    println!("=== Tinox Check ===");
    println!("File: {}", input_file);
    println!("Source length: {} characters", source.len());
    println!("(Full parsing not yet implemented - V1 minimal)");
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
        // ["foo", "bar"] → "foo/bar.tnx"
        let mut rel = PathBuf::new();
        for (i, seg) in import.path.iter().enumerate() {
            if i == import.path.len() - 1 {
                rel.push(format!("{}.tnx", seg));
            } else {
                rel.push(seg);
            }
        }
        let full_path = base_dir
            .join(&rel)
            .canonicalize()
            .map_err(|e| format!("Cannot resolve import '{}': {}", rel.display(), e))?;

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

    let mut codegen = CodeGen::new();
    codegen.set_interface_info(iface_methods, class_implements);
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

    let llc_status = Command::new("llc")
        .args(&[
            "-O3",
            "-march=x86-64",
            "-filetype=obj",
            "-o",
            &obj_path,
            ir_path,
        ])
        .status()
        .map_err(|e| format!("llc failed: {}", e))?;

    if !llc_status.success() {
        return Err("llc failed".to_string());
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
        .args(&[&obj_path, &runtime_obj, "-o", output_name, "-lm", "-no-pie"])
        .status()
        .map_err(|e| format!("Failed to link: {}", e))?;

    if !link_status.success() {
        return Err("Linking failed".to_string());
    }

    let _ = fs::remove_file(&obj_path);
    let _ = fs::remove_file(&runtime_obj);

    Ok(())
}
