use std::env;
use std::fs;
use std::process::Command;

use tinox_codegen::CodeGen;
use tinox_lexer::Lexer;
use tinox_parser::Parser;

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

fn compile_file(input_path: &str, output_name: &str) -> Result<(), String> {
    let source =
        fs::read_to_string(input_path).map_err(|e| format!("Failed to read file: {}", e))?;

    let mut lexer = Lexer::new(&source);
    let tokens = lexer
        .tokenize()
        .map_err(|e| format!("Lexer error: {:?}", e))?;

    let mut parser = Parser::new(tokens);
    let ast = parser
        .parse()
        .map_err(|e| format!("Parse error: {:?}", e))?;

    let mut typechecker = tinox_typecheck::TypeChecker::new();
    typechecker
        .check(&ast)
        .map_err(|e| format!("Type error:\n{}", e))?;

    let mut codegen = CodeGen::new();
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
