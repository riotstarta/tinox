use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tinox_codegen::CodeGen;
use tinox_common;
use tinox_lexer::Lexer;
use tinox_parser::{DeclKind, Formatter, Parser};

mod pm;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_help();
        return;
    }

    match args[1].as_str() {
        "new"   => new_project(&args[2..]),
        "build" => build(&args[2..]),
        "run"   => run_file(&args[2..]),
        "dev"   => dev_mode(&args[2..]),
        "test"  => run_tests(&args[2..]),
        "doc"   => gen_docs(&args[2..]),
        "check"   => check(&args[2..]),
        "fmt"     => fmt(&args[2..]),
        "repl"    => repl(),
        "install" => pm::cmd_install(),
        "add"     => pm::cmd_add(&args[2..]),
        "package" => pm::cmd_package(),
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
    println!("  tinox new <name>           Create a new Tinox project");
    println!("  tinox build [file]         Compile to an executable (uses tinox.toml if no file)");
    println!("  tinox run   [file]         Compile and run (uses tinox.toml if no file)");
    println!("  tinox dev   [file]         Dev mode: hot-reload on file changes");
    println!("  tinox test  [file]         Run all @Test-annotated methods");
    println!("  tinox test --watch         Re-run tests on file changes (TDD mode)");
    println!("  tinox doc   [--open]       Generate HTML documentation in docs/");
    println!("  tinox check [file]         Type-check without compiling");
    println!("  tinox fmt   <file>         Format a Tinox file (print to stdout)");
    println!("  tinox fmt --write <file>   Format a Tinox file in place");
    println!("  tinox repl                 Start interactive REPL");
    println!("  tinox install              Download and install all dependencies");
    println!("  tinox add <g> <a> <v> <u>  Add a dependency and install it");
    println!("  tinox package              Pack src/ into <name>-<version>.tar.gz");
    println!("  tinox help                 Show this help message");
}

fn new_project(args: &[String]) {
    let name = match args.first() {
        Some(n) => n.clone(),
        None => {
            eprintln!("Error: Project name required. Usage: tinox new <name>");
            return;
        }
    };

    if name.is_empty() || name.contains('/') || name.contains('\\') {
        eprintln!("Error: Invalid project name '{}'", name);
        return;
    }

    let root = PathBuf::from(&name);
    if root.exists() {
        eprintln!("Error: '{}' already exists", name);
        return;
    }

    let src_dir = root.join("src");
    let tests_dir = root.join("tests");

    let create = |path: &PathBuf| -> bool {
        if let Err(e) = fs::create_dir_all(path) {
            eprintln!("Error creating {}: {}", path.display(), e);
            return false;
        }
        true
    };

    let write_file = |path: &PathBuf, content: &str| -> bool {
        if let Err(e) = fs::write(path, content) {
            eprintln!("Error writing {}: {}", path.display(), e);
            return false;
        }
        true
    };

    if !create(&src_dir) || !create(&tests_dir) { return; }

    let toml = format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\ndescription = \"\"\n"
    );
    let yaml = format!(
        "package:\n  name: \"{name}\"\n  version: \"0.1.0\"\n  description: \"\"\n\ndependencies: []\n"
    );
    let main_tnx = format!(
        "fn main() -> Int64\n{{\n    println(\"Hello from {name}!\");\n    return 0;\n}}\n"
    );
    let test_tnx = format!(
        "class {name}Tests\n{{\n    @Test(\"example test\")\n    fn testExample() -> Bool\n    {{\n        return 1 + 1 == 2;\n    }}\n}}\n"
    );
    let gitignore = ".tinox/\n";

    if !write_file(&root.join("tinox.toml"), &toml) { return; }
    if !write_file(&root.join("tinox.yaml"), &yaml) { return; }
    if !write_file(&root.join(".gitignore"), &gitignore) { return; }
    if !write_file(&src_dir.join("main.tnx"), &main_tnx) { return; }
    if !write_file(&tests_dir.join("main_test.tnx"), &test_tnx) { return; }

    println!("Created project '{name}'");
    println!("  {name}/tinox.toml");
    println!("  {name}/tinox.yaml");
    println!("  {name}/src/main.tnx");
    println!("  {name}/tests/main_test.tnx");
    println!();
    println!("Get started:");
    println!("  cd {name}");
    println!("  tinox run");
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

/// Returns the entry `.tnx` file for the current project.
/// If `args` has a file, use that. Otherwise read tinox.toml → src/main.tnx.
fn resolve_entry_file(args: &[String]) -> Option<String> {
    if let Some(f) = args.iter().find(|a| !a.starts_with('-')) {
        return Some(f.clone());
    }
    // Project mode: look for tinox.toml in current dir or parents
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let toml = dir.join("tinox.toml");
        if toml.exists() {
            let candidate = dir.join("src").join("main.tnx");
            if candidate.exists() {
                return Some(candidate.to_string_lossy().into_owned());
            }
            eprintln!("error: tinox.toml found but src/main.tnx is missing");
            return None;
        }
        if !dir.pop() { break; }
    }
    eprintln!("error: no input file and no tinox.toml found");
    None
}

fn build(args: &[String]) {
    let release = args.iter().any(|a| a == "--release");
    let debug   = args.iter().any(|a| a == "--debug");
    let opt = if release { OptLevel::Release } else if debug { OptLevel::Debug } else { OptLevel::Release };

    let input_file = match resolve_entry_file(args) {
        Some(f) => f,
        None => return,
    };
    let output_name = parse_output_flag(args).unwrap_or_else(|| {
        read_project_name().unwrap_or_else(|| {
            Path::new(&input_file).file_stem().unwrap_or_default().to_string_lossy().into_owned()
        })
    });

    match compile_file(&input_file, &output_name, opt) {
        Ok(_) => println!("Compiled successfully: {} ({:?})", output_name, opt),
        Err(e) => eprintln!("Compilation failed: {}", e),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum OptLevel { Release, Debug }

/// Read `[metrics]` section from the nearest `tinox.toml`, if present.
/// Returns `Some(path)` when `enabled = true` is set; `None` otherwise.
fn read_metrics_config() -> Option<String> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let toml_path = dir.join("tinox.toml");
        if toml_path.exists() {
            let content = fs::read_to_string(&toml_path).ok()?;
            let mut in_metrics = false;
            let mut enabled = false;
            let mut path = "/metrics".to_string();
            for line in content.lines() {
                let line = line.trim();
                if line.starts_with('[') {
                    in_metrics = line == "[metrics]";
                    continue;
                }
                if !in_metrics { continue; }
                if let Some(rest) = line.strip_prefix("enabled") {
                    let rest = rest.trim().strip_prefix('=').map(|s| s.trim()).unwrap_or("");
                    enabled = rest == "true";
                } else if let Some(rest) = line.strip_prefix("path") {
                    let rest = rest.trim().strip_prefix('=').map(|s| s.trim()).unwrap_or("/metrics");
                    path = rest.trim_matches('"').to_string();
                }
            }
            return if enabled { Some(path) } else { None };
        }
        if !dir.pop() { break; }
    }
    None
}

struct DbConfig {
    driver: String,
    url: String,
    #[allow(dead_code)]
    pool: usize,
}

/// Read `[database]` section from the nearest `tinox.toml`, if present.
fn read_database_config() -> Option<DbConfig> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let toml_path = dir.join("tinox.toml");
        if toml_path.exists() {
            let content = fs::read_to_string(&toml_path).ok()?;
            let mut in_db = false;
            let mut driver = String::new();
            let mut url = String::new();
            let mut pool: usize = 1;
            let mut found = false;
            for line in content.lines() {
                let line = line.trim();
                if line.starts_with('[') {
                    in_db = line == "[database]";
                    if in_db { found = true; }
                    continue;
                }
                if !in_db { continue; }
                if let Some(rest) = line.strip_prefix("driver") {
                    let rest = rest.trim().strip_prefix('=').map(|s| s.trim()).unwrap_or("");
                    driver = rest.trim_matches('"').to_string();
                } else if let Some(rest) = line.strip_prefix("url") {
                    let rest = rest.trim().strip_prefix('=').map(|s| s.trim()).unwrap_or("");
                    url = rest.trim_matches('"').to_string();
                } else if let Some(rest) = line.strip_prefix("pool") {
                    let rest = rest.trim().strip_prefix('=').map(|s| s.trim()).unwrap_or("1");
                    pool = rest.parse().unwrap_or(1);
                }
            }
            if found && !driver.is_empty() {
                return Some(DbConfig { driver, url, pool });
            }
            return None;
        }
        if !dir.pop() { break; }
    }
    None
}

/// Read `name` from the nearest `tinox.toml`, if present.
fn read_project_name() -> Option<String> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let toml_path = dir.join("tinox.toml");
        if toml_path.exists() {
            let content = fs::read_to_string(&toml_path).ok()?;
            for line in content.lines() {
                let line = line.trim();
                if let Some(rest) = line.strip_prefix("name") {
                    let rest = rest.trim();
                    if let Some(rest) = rest.strip_prefix('=') {
                        let name = rest.trim().trim_matches('"').to_string();
                        if !name.is_empty() {
                            return Some(name);
                        }
                    }
                }
            }
            return None;
        }
        if !dir.pop() { break; }
    }
    None
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

fn repl() {
    use std::io::{self, BufRead, Write};

    println!("  Tinox REPL v{}", env!("CARGO_PKG_VERSION"));
    println!("  Type Tinox expressions or declarations. Empty line = evaluate.");
    println!("  :quit  to exit   :clear  to reset session   :help  for commands");
    println!();

    // Accumulated class/function declarations across REPL turns
    let mut session_decls = String::new();
    // Input accumulator for multi-line blocks
    let mut input_buf = String::new();
    let mut line_no: usize = 0;

    let stdin = io::stdin();
    loop {
        let prompt = if input_buf.is_empty() { ">>> " } else { "... " };
        print!("{}", prompt);
        io::stdout().flush().ok();

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(e) => { eprintln!("read error: {}", e); break; }
        }
        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');

        // REPL commands
        match trimmed {
            ":quit" | ":q" | ":exit" => break,
            ":clear" | ":reset" => {
                session_decls.clear();
                input_buf.clear();
                line_no = 0;
                println!("  Session cleared.");
                continue;
            }
            ":help" => {
                println!("  Commands:");
                println!("    :quit / :q     Exit the REPL");
                println!("    :clear         Reset session (forget all declarations)");
                println!("    :session       Show accumulated declarations");
                println!("    :help          Show this message");
                println!();
                println!("  Tinox REPL tips:");
                println!("    • Enter expressions to evaluate them");
                println!("    • Declare functions and classes; they persist across entries");
                println!("    • Multi-line: keep typing — empty line submits");
                continue;
            }
            ":session" => {
                if session_decls.is_empty() {
                    println!("  (empty session)");
                } else {
                    println!("{}", session_decls);
                }
                continue;
            }
            _ => {}
        }

        input_buf.push_str(trimmed);
        input_buf.push('\n');

        let open_braces = input_buf.chars().filter(|&c| c == '{').count();
        let close_braces = input_buf.chars().filter(|&c| c == '}').count();
        let is_empty_submit = trimmed.is_empty() && !input_buf.trim().is_empty();
        let has_open_brace = open_braces > 0;
        // For block constructs (fn, class, etc.): submit only when braces are fully balanced
        // and at least one closing brace has been seen.
        // For simple expressions (no braces): submit immediately.
        let is_complete = if has_open_brace {
            open_braces == close_braces
        } else {
            // No braces yet: submit only if the line doesn't look like it needs more input
            let first = input_buf.trim().split_whitespace().next().unwrap_or("");
            !matches!(first, "fn" | "class" | "interface" | "enum" | "trait"
                           | "if" | "while" | "for" | "loop"
                           | "let" | "var") // let/var need explicit submit (empty line)
        };

        if !is_empty_submit && !is_complete {
            continue;
        }

        let entry = input_buf.trim().to_string();
        input_buf.clear();

        if entry.is_empty() { continue; }

        line_no += 1;
        repl_eval(&entry, &mut session_decls, line_no);
    }

    println!("Bye!");
}

/// Evaluate one REPL entry: either a declaration (saved to session) or an expression (printed).
fn repl_eval(entry: &str, session_decls: &mut String, turn: usize) {
    // Detect declarations: starts with fn, class, interface, enum, let, var at top level
    let first_token = entry.split_whitespace().next().unwrap_or("");
    // Only top-level structural declarations go into session_decls.
    // let/var are statements (must live inside a function body).
    let is_decl = matches!(first_token,
        "fn" | "class" | "interface" | "enum" | "trait" | "import"
    ) || entry.starts_with('@'); // annotations precede class/fn

    if is_decl {
        // Try to parse and type-check the new declaration
        let combined = format!("{}\n{}", session_decls, entry);
        let tokens = match Lexer::new(&combined).tokenize() {
            Ok(t) => t,
            Err(errs) => {
                for e in &errs { eprintln!("error: {}", e.message); }
                return;
            }
        };
        match tinox_parser::Parser::new(tokens).parse() {
            Ok(_) => {
                session_decls.push_str(entry);
                session_decls.push('\n');
                println!("  defined.");
            }
            Err(bag) => {
                for e in &bag.errors { eprintln!("error: {}", e.message); }
            }
        }
        return;
    }

    // Detect if this is a statement block (contains ; or multiple lines or let/var)
    let lines: Vec<&str> = entry.lines().filter(|l| !l.trim().is_empty()).collect();
    let has_semicolons = entry.contains(';');
    let is_multi_line = lines.len() > 1;
    let starts_with_stmt = matches!(first_token, "let" | "var" | "println" | "print" | "return");

    let is_stmt_block = is_multi_line || (has_semicolons && starts_with_stmt) || starts_with_stmt;

    if is_stmt_block {
        // Ensure each statement line ends with ; (normalize)
        let body: String = entry.lines()
            .map(|l| {
                let t = l.trim_end();
                if t.is_empty() || t.ends_with(';') || t.ends_with('{') || t.ends_with('}') {
                    l.to_string()
                } else {
                    format!("{};", l)
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let src = format!(
            "{}\nfn __repl_{}() -> Int64 {{\n{}\n    return 0;\n}}\nfn main() -> Int64 {{\n    __repl_{}();\n    return 0;\n}}\n",
            session_decls, turn, body, turn
        );
        match repl_compile_and_run(&src, turn) {
            Ok(output) => {
                if !output.is_empty() {
                    print!("{}", output);
                    if !output.ends_with('\n') { println!(); }
                }
            }
            Err(msg) => eprintln!("error: {}", msg),
        }
        return;
    }

    // Single expression: try to print the value via println()
    let expr_clean = entry.trim_end_matches(';').trim();
    let src = format!(
        "{}\nfn __repl_{}() -> Int64 {{\n    println({});\n    return 0;\n}}\nfn main() -> Int64 {{\n    __repl_{}();\n    return 0;\n}}\n",
        session_decls, turn, expr_clean, turn
    );

    let result = repl_compile_and_run(&src, turn);
    match result {
        Ok(output) => {
            if !output.is_empty() {
                print!("{}", output);
                if !output.ends_with('\n') { println!(); }
            }
        }
        Err(msg) => {
            // Fallback: run as a void statement (e.g. method calls that return Nothing)
            let src2 = format!(
                "{}\nfn __repl_{}() -> Int64 {{\n    {};\n    return 0;\n}}\nfn main() -> Int64 {{\n    __repl_{}();\n    return 0;\n}}\n",
                session_decls, turn, expr_clean, turn
            );
            match repl_compile_and_run(&src2, turn) {
                Ok(output) => {
                    if !output.is_empty() {
                        print!("{}", output);
                        if !output.ends_with('\n') { println!(); }
                    }
                }
                Err(_) => eprintln!("error: {}", msg),
            }
        }
    }
}

fn repl_compile_and_run(src: &str, turn: usize) -> Result<String, String> {
    // Lex + parse + codegen
    let tokens = Lexer::new(src).tokenize()
        .map_err(|errs| errs.iter().map(|e| e.message.clone()).collect::<Vec<_>>().join("; "))?;

    let ast = tinox_parser::Parser::new(tokens).parse()
        .map_err(|bag| bag.errors.iter().map(|e| e.message.clone()).collect::<Vec<_>>().join("; "))?;

    let mut cg = CodeGen::new();
    cg.gen(&ast).map_err(|e| format!("{:?}", e))?;
    let ir = cg.into_ir();

    // Write to a temp file and compile
    let tmp_base = format!("/tmp/.tinox_repl_{}", turn);
    let ir_path = format!("{}.ll", tmp_base);
    fs::write(&ir_path, &ir)
        .map_err(|e| format!("write IR: {}", e))?;

    let runtime_obj = find_runtime_object();

    // Compile to executable
    let exe = format!("{}.out", tmp_base);
    let mut cmd = Command::new("clang");
    cmd.arg(&ir_path).arg("-o").arg(&exe).arg("-O0").arg("-lm").arg("-lgc");
    if let Some(ref rt) = runtime_obj {
        cmd.arg(rt);
    }

    let out = cmd.output()
        .map_err(|e| format!("clang: {}", e))?;
    if !out.status.success() {
        let _ = fs::remove_file(&ir_path);
        return Err(String::from_utf8_lossy(&out.stderr)
            .lines().take(3).collect::<Vec<_>>().join("; "));
    }

    // Run
    let run_out = Command::new(&exe).output()
        .map_err(|e| format!("run: {}", e))?;

    let _ = fs::remove_file(&ir_path);
    let _ = fs::remove_file(&exe);

    if !run_out.status.success() {
        return Err(format!("exited with {}", run_out.status));
    }

    Ok(String::from_utf8_lossy(&run_out.stdout).to_string())
}

fn find_runtime_object() -> Option<String> {
    // Try common locations for the precompiled runtime.o
    let candidates = [
        "runtime/runtime.o",
        "../runtime/runtime.o",
        "runtime.o",
    ];
    for c in &candidates {
        if Path::new(c).exists() { return Some(c.to_string()); }
    }
    // If runtime.c exists but not runtime.o, compile it on the fly
    let c_candidates = [
        "runtime/runtime.c",
        "../runtime/runtime.c",
        "runtime.c",
    ];
    for c in &c_candidates {
        if Path::new(c).exists() {
            let obj = "/tmp/.tinox_runtime.o";
            let status = Command::new("clang")
                .args(["-c", c, "-o", obj, "-O3"])
                .status().ok()?;
            if status.success() { return Some(obj.to_string()); }
        }
    }
    None
}

fn run_file(args: &[String]) {
    let input_file = match resolve_entry_file(args) {
        Some(f) => f,
        None => return,
    };
    let exe_name = format!(".tinox_tmp_{}", std::process::id());

    let opt = if args.iter().any(|a| a == "--debug") { OptLevel::Debug } else { OptLevel::Release };
    match compile_file(&input_file, &exe_name, opt) {
        Ok(_) => {
            let status = Command::new(&format!("./{}", exe_name))
                .status()
                .expect("Failed to run executable");

            let _ = fs::remove_file(&exe_name);
            let _ = fs::remove_file(format!("{}.ll", exe_name));

            std::process::exit(status.code().unwrap_or(1));
        }
        Err(e) => eprintln!("Compilation failed: {}", e),
    }
}

fn print_dev_banner(watching: &str) {
    eprintln!();
    eprintln!("  ████████╗██╗███╗   ██╗ ██████╗ ██╗  ██╗");
    eprintln!("     ██╔══╝██║████╗  ██║██╔═══██╗╚██╗██╔╝");
    eprintln!("     ██║   ██║██╔██╗ ██║██║   ██║ ╚███╔╝ ");
    eprintln!("     ██║   ██║██║╚██╗██║██║   ██║ ██╔██╗ ");
    eprintln!("     ██║   ██║██║ ╚████║╚██████╔╝██╔╝ ██╗");
    eprintln!("     ╚═╝   ╚═╝╚═╝  ╚═══╝ ╚═════╝ ╚═╝  ╚═╝");
    eprintln!();
    eprintln!("  :: Dev Mode ::  (v{})", env!("CARGO_PKG_VERSION"));
    eprintln!("  Watching : {}", watching);
    eprintln!("  Stop     : Ctrl+C");
    eprintln!("  ─────────────────────────────────────────");
    eprintln!();
}

fn dev_mode(args: &[String]) {
    use notify::{RecursiveMode, Watcher};
    use std::sync::mpsc;

    let input_file = match resolve_entry_file(args) {
        Some(f) => f,
        None => return,
    };
    let exe_name = format!(".tinox_dev_{}", std::process::id());

    print_dev_banner(&input_file);

    let compile_and_run = |child: &mut Option<std::process::Child>| {
        if let Some(ref mut c) = child {
            let _ = c.kill();
            let _ = c.wait();
            *child = None;
        }

        eprint!("[dev] compiling... ");
        match compile_file(&input_file, &exe_name, OptLevel::Debug) {
            Ok(_) => {
                eprintln!("ok");
                match Command::new(format!("./{}", exe_name)).spawn() {
                    Ok(c) => {
                        eprintln!("[dev] started (pid {})", c.id());
                        *child = Some(c);
                    }
                    Err(e) => eprintln!("[dev] launch failed: {}", e),
                }
            }
            Err(e) => eprintln!("[dev] compile error:\n{}", e),
        }
    };

    let mut child: Option<std::process::Child> = None;
    compile_and_run(&mut child);

    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })
    .expect("Failed to create file watcher");

    let watch_dir = Path::new(&input_file)
        .parent()
        .unwrap_or(Path::new("."));
    watcher
        .watch(watch_dir, RecursiveMode::Recursive)
        .expect("Failed to watch directory");

    loop {
        match rx.recv() {
            Ok(Ok(event)) => {
                let is_tnx = event.paths.iter().any(|p| {
                    p.extension().map(|e| e == "tnx").unwrap_or(false)
                });
                if is_tnx {
                    eprintln!("[dev] change detected — rebuilding...");
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    while rx.try_recv().is_ok() {}
                    compile_and_run(&mut child);
                }
            }
            Ok(Err(e)) => eprintln!("[dev] watcher error: {}", e),
            Err(_) => break,
        }
    }

    if let Some(ref mut c) = child {
        let _ = c.kill();
        let _ = c.wait();
    }
    let _ = fs::remove_file(&exe_name);
    let _ = fs::remove_file(format!("{}.ll", exe_name));
}

fn check(args: &[String]) {
    let input_file = match resolve_entry_file(args) {
        Some(f) => f,
        None => std::process::exit(1),
    };
    let source = match fs::read_to_string(&input_file) {
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
                print_error(&input_file, &lines, e.span, &e.message);
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
                print_error(&input_file, &lines, e.span, &e.message);
            }
            eprintln!("\naborting: {} error{}", count, if count == 1 { "" } else { "s" });
            std::process::exit(1);
        }
    };

    // Resolve imports
    let base_dir = Path::new(&input_file)
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let mut visited = HashSet::new();
    if let Ok(canonical) = Path::new(&input_file).canonicalize() {
        visited.insert(canonical);
    }
    let dep_dirs = load_dep_dirs();
    if let Err(e) = resolve_imports(&mut ast, &base_dir, &mut visited, &dep_dirs) {
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
            println!("{}: no errors", &input_file);
            std::process::exit(0);
        }
        Err(bag) => {
            let count = bag.errors.len();
            for e in &bag.errors {
                print_error(&input_file, &lines, e.span, &e.message);
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

fn run_tests(args: &[String]) {
    let watch_mode = args.iter().any(|a| a == "--watch" || a == "-w");
    let filtered_args: Vec<String> = args.iter()
        .filter(|a| *a != "--watch" && *a != "-w")
        .cloned()
        .collect();

    if watch_mode {
        run_tests_watch(&filtered_args);
        return;
    }
    run_tests_once(&filtered_args);
}

fn run_tests_watch(args: &[String]) {
    use notify::{RecursiveMode, Watcher};
    use std::sync::mpsc;

    let source_files = collect_test_files(args);
    if source_files.is_empty() { return; }

    eprintln!();
    eprintln!("  Tinox Test Watch");
    eprintln!("  ─────────────────────────────────────────");
    eprintln!("  Press Ctrl+C to stop");
    eprintln!();

    run_tests_once(args);

    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res| { let _ = tx.send(res); })
        .expect("watcher");

    // Watch every directory that contains a test or source file
    let mut watched_dirs: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for f in &source_files {
        if let Some(parent) = Path::new(f).parent() {
            let dir = parent.to_path_buf();
            if watched_dirs.insert(dir.clone()) {
                let _ = watcher.watch(&dir, RecursiveMode::Recursive);
            }
        }
    }
    // Also watch src/
    if let Ok(cwd) = std::env::current_dir() {
        let src = cwd.join("src");
        if src.is_dir() && watched_dirs.insert(src.clone()) {
            let _ = watcher.watch(&src, RecursiveMode::Recursive);
        }
    }

    loop {
        match rx.recv() {
            Ok(Ok(event)) => {
                let is_tnx = event.paths.iter().any(|p| {
                    p.extension().map(|e| e == "tnx").unwrap_or(false)
                });
                if is_tnx {
                    eprintln!("\n[watch] change detected — re-running tests...");
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    while rx.try_recv().is_ok() {}
                    run_tests_once(args);
                }
            }
            Ok(Err(e)) => eprintln!("[watch] error: {}", e),
            Err(_) => break,
        }
    }
}

fn collect_test_files(args: &[String]) -> Vec<String> {
    if let Some(f) = args.first().filter(|a| !a.starts_with('-')) {
        return vec![f.clone()];
    }
    let mut files = Vec::new();
    let mut dir = std::env::current_dir().unwrap_or_default();
    loop {
        if dir.join("tinox.toml").exists() {
            for sub in &["tests", "src"] {
                let d = dir.join(sub);
                if d.is_dir() {
                    if let Ok(entries) = fs::read_dir(&d) {
                        for entry in entries.flatten() {
                            let p = entry.path();
                            if p.extension().map(|e| e == "tnx").unwrap_or(false) {
                                files.push(p.to_string_lossy().into_owned());
                            }
                        }
                    }
                }
            }
            break;
        }
        if !dir.pop() { break; }
    }
    files
}

fn run_tests_once(args: &[String]) {
    let source_files = collect_test_files(args);
    if source_files.is_empty() {
        eprintln!("error: no test files found — run from a project directory or pass a file");
        return;
    }

    let mut total = 0usize;
    let mut passed = 0usize;
    let mut failed = 0usize;

    for source_path in &source_files {
        let test_entries = match collect_tests(source_path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("error processing {}: {}", source_path, e);
                continue;
            }
        };
        if test_entries.is_empty() { continue; }

        println!("Running {} test{} from {}",
            test_entries.len(),
            if test_entries.len() == 1 { "" } else { "s" },
            source_path);

        for t in &test_entries {
            total += 1;
            let exe = format!(".tinox_test_{}_{}", std::process::id(), total);
            let result = compile_test_exe(source_path, &t.class_name, &t.method_name, &exe);
            match result {
                Err(e) => {
                    println!("  FAIL  {} — compile error: {}", t.description, e);
                    failed += 1;
                    continue;
                }
                Ok(_) => {}
            }
            let status = Command::new(format!("./{exe}")).status();
            let _ = fs::remove_file(&exe);
            let _ = fs::remove_file(format!("{exe}.ll"));
            match status {
                Ok(s) if s.code() == Some(0) => {
                    println!("  PASS  {}", t.description);
                    passed += 1;
                }
                Ok(s) => {
                    println!("  FAIL  {} — exit code {}", t.description, s.code().unwrap_or(-1));
                    failed += 1;
                }
                Err(e) => {
                    println!("  FAIL  {} — {}", t.description, e);
                    failed += 1;
                }
            }
        }
    }

    println!();
    println!("{total} test{} — {passed} passed, {failed} failed",
        if total == 1 { "" } else { "s" });

    if failed > 0 {
        std::process::exit(1);
    }
}

/// Parse a file and return all @Test entries without compiling.
// ─── tinox doc ────────────────────────────────────────────────────────────────

fn gen_docs(args: &[String]) {
    let open = args.iter().any(|a| a == "--open");

    // Collect source files from project or explicit arg
    let source_files: Vec<String> = if let Some(f) = args.first().filter(|a| !a.starts_with('-')) {
        vec![f.clone()]
    } else {
        let mut files = Vec::new();
        let mut dir = std::env::current_dir().unwrap_or_default();
        loop {
            if dir.join("tinox.toml").exists() {
                let src = dir.join("src");
                if src.is_dir() {
                    if let Ok(entries) = fs::read_dir(&src) {
                        for e in entries.flatten() {
                            let p = e.path();
                            if p.extension().map(|x| x == "tnx").unwrap_or(false) {
                                files.push(p.to_string_lossy().into_owned());
                            }
                        }
                    }
                }
                break;
            }
            if !dir.pop() { break; }
        }
        files
    };

    if source_files.is_empty() {
        eprintln!("error: no source files found");
        return;
    }

    let project_name = read_project_name().unwrap_or_else(|| "Tinox Project".to_string());
    let mut doc_items: Vec<DocItem> = Vec::new();

    for path in &source_files {
        let src = match fs::read_to_string(path) { Ok(s) => s, Err(_) => continue };
        let mut lexer = Lexer::new(&src);
        let tokens = match lexer.tokenize() { Ok(t) => t, Err(_) => continue };
        let mut parser = tinox_parser::Parser::new(tokens);
        let ast = match parser.parse() { Ok(a) => a, Err(_) => continue };

        for decl in &ast.decls {
            collect_doc_items(&decl.node, &mut doc_items);
        }
    }

    let html = render_docs_html(&project_name, &doc_items);

    // Write to docs/index.html
    let docs_dir = PathBuf::from("docs");
    if let Err(e) = fs::create_dir_all(&docs_dir) {
        eprintln!("error: cannot create docs/: {}", e);
        return;
    }
    let out_path = docs_dir.join("index.html");
    if let Err(e) = fs::write(&out_path, &html) {
        eprintln!("error: cannot write {}: {}", out_path.display(), e);
        return;
    }

    println!("Documentation written to {}", out_path.display());

    if open {
        let _ = Command::new("xdg-open").arg(&out_path).spawn()
            .or_else(|_| Command::new("open").arg(&out_path).spawn());
    }
}

// ── Doc data model ────────────────────────────────────────────────────────────

struct DocParam  { name: String, ty: String }
struct DocMethod { name: String, doc: Option<String>, params: Vec<DocParam>, ret: String, annotations: Vec<String>, is_static: bool }
struct DocField  { name: String, ty: String, doc: Option<String>, annotations: Vec<String> }

enum DocItem {
    Class {
        name: String,
        doc: Option<String>,
        annotations: Vec<String>,
        fields: Vec<DocField>,
        methods: Vec<DocMethod>,
        implements: Vec<String>,
        extends: Option<String>,
    },
    Interface {
        name: String,
        doc: Option<String>,
        methods: Vec<DocMethod>,
    },
    Function {
        name: String,
        doc: Option<String>,
        params: Vec<DocParam>,
        ret: String,
        annotations: Vec<String>,
    },
}

fn collect_doc_items(decl: &tinox_parser::DeclKind, out: &mut Vec<DocItem>) {
    use tinox_parser::DeclKind;
    match decl {
        DeclKind::Class(c) if c.type_params.is_empty() => {
            let annotations = c.annotations.iter().map(|a| a.name.clone()).collect();
            let fields = c.fields.iter().map(|f| DocField {
                name: f.name.clone(),
                ty: type_str_simple(&f.field_type),
                doc: f.doc.clone(),
                annotations: f.annotations.iter().map(|a| a.name.clone()).collect(),
            }).collect();
            let methods = c.methods.iter().map(method_to_doc).collect();
            out.push(DocItem::Class {
                name: c.name.clone(),
                doc: c.doc.clone(),
                annotations,
                fields,
                methods,
                implements: c.implements.clone(),
                extends: c.extends.clone(),
            });
        }
        DeclKind::Interface(i) => {
            let methods = i.methods.iter().map(|m| {
                let params = m.params.iter().map(|p| DocParam { name: p.name.clone(), ty: type_str_simple(&p.param_type) }).collect();
                DocMethod {
                    name: m.name.clone(),
                    doc: m.doc.clone(),
                    params,
                    ret: type_str_simple(&m.ret_type),
                    annotations: m.annotations.iter().map(|a| a.name.clone()).collect(),
                    is_static: false,
                }
            }).collect();
            out.push(DocItem::Interface { name: i.name.clone(), doc: i.doc.clone(), methods });
        }
        DeclKind::Function(f) => {
            let params = f.params.iter().map(|p| DocParam { name: p.name.clone(), ty: type_str_simple(&p.param_type) }).collect();
            out.push(DocItem::Function {
                name: f.name.clone(),
                doc: f.doc.clone(),
                params,
                ret: type_str_simple(&f.ret_type),
                annotations: f.annotations.iter().map(|a| a.name.clone()).collect(),
            });
        }
        DeclKind::Namespace(ns) => {
            for inner in &ns.decls { collect_doc_items(&inner.node, out); }
        }
        _ => {}
    }
}

fn method_to_doc(m: &tinox_parser::Method) -> DocMethod {
    let params = m.params.iter().map(|p| DocParam { name: p.name.clone(), ty: type_str_simple(&p.param_type) }).collect();
    DocMethod {
        name: m.name.clone(),
        doc: m.doc.clone(),
        params,
        ret: type_str_simple(&m.ret_type),
        annotations: m.annotations.iter().map(|a| a.name.clone()).collect(),
        is_static: m.static_,
    }
}

fn type_str_simple(ty: &tinox_parser::Type) -> String {
    use tinox_parser::Type;
    match ty {
        Type::Int8 => "Int8".into(),   Type::Int16 => "Int16".into(),
        Type::Int32 => "Int32".into(), Type::Int64 => "Int64".into(),
        Type::UInt8 => "UInt8".into(), Type::UInt16 => "UInt16".into(),
        Type::UInt32 => "UInt32".into(), Type::UInt64 => "UInt64".into(),
        Type::Float32 => "Float32".into(), Type::Float64 => "Float64".into(),
        Type::Bool => "Bool".into(), Type::String => "String".into(),
        Type::Char => "Char".into(), Type::Nothing => "Nothing".into(),
        Type::Named(n) => n.clone(),
        Type::Array(t) => format!("{}[]", type_str_simple(t)),
        Type::Map(k, v) => format!("Map<{}, {}>", type_str_simple(k), type_str_simple(v)),
        Type::Tuple(ts) => format!("({})", ts.iter().map(type_str_simple).collect::<Vec<_>>().join(", ")),
        Type::Generic { name: n, args } => format!("{}<{}>", n, args.iter().map(type_str_simple).collect::<Vec<_>>().join(", ")),
        Type::Fn { params, ret } => format!("fn({}) -> {}", params.iter().map(type_str_simple).collect::<Vec<_>>().join(", "), type_str_simple(ret)),
        Type::Never => "Never".into(),
        Type::Any => "Any".into(),
        Type::Infer => "_".into(),
        Type::Mutable(t) => format!("mut {}", type_str_simple(t)),
        Type::Ref(t) => format!("&{}", type_str_simple(t)),
        Type::Nullable(t) => format!("{}?", type_str_simple(t)),
    }
}

// ── HTML renderer ─────────────────────────────────────────────────────────────

fn render_docs_html(project_name: &str, items: &[DocItem]) -> String {
    let mut nav = String::new();
    let mut body = String::new();

    let classes: Vec<&DocItem> = items.iter().filter(|i| matches!(i, DocItem::Class {..})).collect();
    let interfaces: Vec<&DocItem> = items.iter().filter(|i| matches!(i, DocItem::Interface {..})).collect();
    let functions: Vec<&DocItem> = items.iter().filter(|i| matches!(i, DocItem::Function {..})).collect();

    if !classes.is_empty() {
        nav.push_str("<li class=\"nav-section\">Classes</li>");
        for item in &classes {
            if let DocItem::Class { name, .. } = item {
                nav.push_str(&format!("<li><a href=\"#class-{name}\">{name}</a></li>"));
            }
        }
    }
    if !interfaces.is_empty() {
        nav.push_str("<li class=\"nav-section\">Interfaces</li>");
        for item in &interfaces {
            if let DocItem::Interface { name, .. } = item {
                nav.push_str(&format!("<li><a href=\"#iface-{name}\">{name}</a></li>"));
            }
        }
    }
    if !functions.is_empty() {
        nav.push_str("<li class=\"nav-section\">Functions</li>");
        for item in &functions {
            if let DocItem::Function { name, .. } = item {
                nav.push_str(&format!("<li><a href=\"#fn-{name}\">{name}</a></li>"));
            }
        }
    }

    for item in items {
        match item {
            DocItem::Class { name, doc, annotations, fields, methods, implements, extends } => {
                let anns = render_annotations(annotations);
                let mut subtitle = String::new();
                if let Some(p) = extends { subtitle.push_str(&format!(" extends <code>{p}</code>")); }
                if !implements.is_empty() {
                    subtitle.push_str(&format!(" implements {}", implements.iter().map(|i| format!("<code>{i}</code>")).collect::<Vec<_>>().join(", ")));
                }
                body.push_str(&format!(
                    "<section id=\"class-{name}\" class=\"item\"><h2 class=\"item-name\">{anns}<span class=\"kw\">class</span> {name}{subtitle}</h2>"
                ));
                if let Some(d) = doc { body.push_str(&format!("<p class=\"doc\">{}</p>", html_escape(d))); }

                if !fields.is_empty() {
                    body.push_str("<h3>Fields</h3><table class=\"members\">");
                    for f in fields {
                        let fanns = render_annotations(&f.annotations);
                        let fdoc = f.doc.as_deref().unwrap_or("");
                        body.push_str(&format!(
                            "<tr><td class=\"member-name\">{fanns}<code>{}</code></td><td class=\"member-type\"><code>{}</code></td><td>{}</td></tr>",
                            html_escape(&f.name), html_escape(&f.ty), html_escape(fdoc)
                        ));
                    }
                    body.push_str("</table>");
                }
                if !methods.is_empty() {
                    body.push_str("<h3>Methods</h3>");
                    for m in methods {
                        render_method_html(&mut body, m);
                    }
                }
                body.push_str("</section>");
            }
            DocItem::Interface { name, doc, methods } => {
                body.push_str(&format!(
                    "<section id=\"iface-{name}\" class=\"item\"><h2 class=\"item-name\"><span class=\"kw\">interface</span> {name}</h2>"
                ));
                if let Some(d) = doc { body.push_str(&format!("<p class=\"doc\">{}</p>", html_escape(d))); }
                for m in methods { render_method_html(&mut body, m); }
                body.push_str("</section>");
            }
            DocItem::Function { name, doc, params, ret, annotations } => {
                let anns = render_annotations(annotations);
                let sig = render_sig(name, params, ret, false);
                body.push_str(&format!(
                    "<section id=\"fn-{name}\" class=\"item\"><h2 class=\"item-name\">{anns}<span class=\"kw\">fn</span> {sig}</h2>"
                ));
                if let Some(d) = doc { body.push_str(&format!("<p class=\"doc\">{}</p>", html_escape(d))); }
                body.push_str("</section>");
            }
        }
    }

    format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{project_name} — Tinox Docs</title>
<style>
*{{box-sizing:border-box;margin:0;padding:0}}
body{{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;background:#0f1117;color:#e2e8f0;display:flex;min-height:100vh}}
nav{{width:240px;min-height:100vh;background:#1a1d2e;padding:20px 0;position:sticky;top:0;overflow-y:auto;flex-shrink:0}}
nav h1{{padding:0 20px 16px;font-size:15px;color:#7c85ff;font-weight:700;border-bottom:1px solid #2a2d3e;margin-bottom:8px}}
nav ul{{list-style:none;padding:0}}
nav li a{{display:block;padding:5px 20px;color:#a0aec0;text-decoration:none;font-size:13px}}
nav li a:hover{{color:#fff;background:#252840}}
nav li.nav-section{{padding:12px 20px 4px;font-size:11px;color:#4a5568;text-transform:uppercase;letter-spacing:.08em;font-weight:600}}
main{{flex:1;padding:40px 60px;max-width:900px}}
main h1{{font-size:28px;color:#fff;margin-bottom:8px}}
main>p{{color:#718096;margin-bottom:32px;font-size:14px}}
.item{{border:1px solid #1e2235;border-radius:8px;padding:24px;margin-bottom:24px;background:#13151f}}
.item-name{{font-size:18px;font-weight:600;color:#e2e8f0;margin-bottom:12px;font-family:"SFMono-Regular",Consolas,monospace}}
.kw{{color:#7c85ff}}
.doc{{color:#a0aec0;font-size:14px;margin-bottom:16px;line-height:1.6}}
h3{{font-size:13px;color:#4a5568;text-transform:uppercase;letter-spacing:.06em;margin:16px 0 8px;font-weight:600}}
table.members{{width:100%;border-collapse:collapse;font-size:13px}}
table.members tr{{border-bottom:1px solid #1e2235}}
table.members td{{padding:6px 8px;vertical-align:top}}
.member-name{{font-family:monospace;color:#e2e8f0;width:35%}}
.member-type{{color:#63b3ed;width:20%}}
.method-sig{{background:#0d0f1a;border-radius:6px;padding:10px 14px;font-family:monospace;font-size:13px;color:#e2e8f0;margin-bottom:8px;border:1px solid #1e2235}}
.method-doc{{color:#a0aec0;font-size:13px;margin-bottom:12px;line-height:1.5}}
.ann{{color:#68d391;font-size:12px;display:inline-block;margin-right:6px}}
code{{color:#63b3ed;font-family:"SFMono-Regular",Consolas,monospace}}
</style>
</head>
<body>
<nav>
  <h1>📦 {project_name}</h1>
  <ul>{nav}</ul>
</nav>
<main>
  <h1>{project_name}</h1>
  <p>Generated by <code>tinox doc</code></p>
  {body}
</main>
</body>
</html>"#)
}

fn render_method_html(out: &mut String, m: &DocMethod) {
    let anns = render_annotations(&m.annotations);
    let kw = if m.is_static { "fnc" } else { "fn" };
    let sig = render_sig(&m.name, &m.params, &m.ret, m.is_static);
    out.push_str(&format!(
        "<div class=\"method-sig\">{anns}<span class=\"kw\">{kw}</span> {sig}</div>"
    ));
    if let Some(d) = &m.doc {
        out.push_str(&format!("<div class=\"method-doc\">{}</div>", html_escape(d)));
    }
}

fn render_sig(name: &str, params: &[DocParam], ret: &str, _static: bool) -> String {
    let ps: Vec<String> = params.iter()
        .map(|p| format!("{}: <span style=\"color:#63b3ed\">{}</span>", html_escape(&p.name), html_escape(&p.ty)))
        .collect();
    format!("{}({}) → <span style=\"color:#63b3ed\">{}</span>", html_escape(name), ps.join(", "), html_escape(ret))
}

fn render_annotations(anns: &[String]) -> String {
    anns.iter().map(|a| format!("<span class=\"ann\">@{a}</span>")).collect()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn collect_tests(path: &str) -> Result<Vec<tinox_typecheck::annotations::TestInfo>, String> {
    let source = fs::read_to_string(path)
        .map_err(|e| format!("cannot read: {e}"))?;
    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize().map_err(|e| format!("lex error: {e:?}"))?;
    let mut parser = tinox_parser::Parser::new(tokens);
    let mut ast = parser.parse().map_err(|e| format!("parse error: {e:?}"))?;
    let base = Path::new(path).parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut visited = HashSet::new();
    if let Ok(c) = Path::new(path).canonicalize() { visited.insert(c); }
    let dep_dirs = load_dep_dirs();
    resolve_imports(&mut ast, &base, &mut visited, &dep_dirs)
        .map_err(|e| format!("import error: {e}"))?;
    let result = tinox_typecheck::annotations::process_annotations(&ast);
    Ok(result.test_entries)
}

/// Compile `source` with a synthetic main that runs one test method and exits 0/1.
/// Compile a test-mode executable: the test method returns Bool; main exits 0 on true.
fn compile_test_exe(source: &str, class_name: &str, method_name: &str, exe: &str) -> Result<(), String> {
    let src = fs::read_to_string(source)
        .map_err(|e| format!("cannot read: {e}"))?;

    let mut lexer = Lexer::new(&src);
    let tokens = lexer.tokenize().map_err(|e| format!("lex: {e:?}"))?;
    let mut parser = tinox_parser::Parser::new(tokens);
    let mut ast = parser.parse().map_err(|e| format!("parse: {e:?}"))?;

    let base = Path::new(source).parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut visited = HashSet::new();
    if let Ok(c) = Path::new(source).canonicalize() { visited.insert(c); }
    let dep_dirs = load_dep_dirs();
    resolve_imports(&mut ast, &base, &mut visited, &dep_dirs)?;

    let mut tc = tinox_typecheck::TypeChecker::new();
    tc.check(&ast).map_err(|e| format!("type error:\n{e}"))?;
    let (iface, impls) = tc.interface_info();

    let ann = tinox_typecheck::annotations::process_annotations(&ast);

    let route_entries = ann.route_entries.iter().map(|r| tinox_codegen::RouteEntry {
        http_method: r.method.clone(), path: r.path.clone(),
        class_name: r.class_name.clone(), method_name: r.method_name.clone(),
        status_code: r.status_code, produces: r.produces.clone(),
        consumes: r.consumes.clone(), auth_type: r.auth_type.clone(),
        is_static: r.is_static,
    }).collect();
    let di_components = ann.di_components.iter().map(|c| tinox_codegen::DiComponentInfo {
        class_name: c.class_name.clone(),
        scope: match c.scope {
            tinox_typecheck::annotations::DiScope::Application => tinox_codegen::DiScope::Application,
            tinox_typecheck::annotations::DiScope::Startup => tinox_codegen::DiScope::Startup,
            tinox_typecheck::annotations::DiScope::HttpRequest => tinox_codegen::DiScope::HttpRequest,
        },
        inject_fields: c.inject_fields.iter().map(|f| tinox_codegen::DiInjectField {
            field_name: f.field_name.clone(), field_type: f.field_type.clone(),
        }).collect(),
    }).collect();
    let config_fields = ann.config_fields.iter().map(|f| tinox_codegen::ConfigFieldInfo {
        class_name: f.class_name.clone(), field_name: f.field_name.clone(),
        config_key: f.config_key.clone(), field_llvm_type: f.field_llvm_type.clone(),
    }).collect();
    let cli_commands = ann.cli_commands.iter().map(|c| tinox_codegen::CliCommandInfo {
        class_name: c.class_name.clone(), cmd_name: c.cmd_name.clone(),
        description: c.description.clone(), version: c.version.clone(),
        options: c.options.iter().map(|o| tinox_codegen::CliOptionInfo {
            field_name: o.field_name.clone(), names: o.names.clone(),
            description: o.description.clone(), required: o.required,
            field_type: o.field_type.clone(),
        }).collect(),
        arguments: c.arguments.iter().map(|a| tinox_codegen::CliArgumentInfo {
            field_name: a.field_name.clone(), index: a.index,
            description: a.description.clone(), required: a.required,
            field_type: a.field_type.clone(),
        }).collect(),
    }).collect();

    let sensitive_fields = ann.sensitive_fields.iter().map(|f| tinox_codegen::LogMaskFieldInfo {
        class_name: f.class_name.clone(), field_name: f.field_name.clone(),
    }).collect();
    let masked_fields = ann.masked_fields.iter().map(|f| tinox_codegen::LogMaskFieldInfo {
        class_name: f.class_name.clone(), field_name: f.field_name.clone(),
    }).collect();

    let mut cg = CodeGen::new();
    cg.set_interface_info(iface, impls);
    let do_not_serialize_fields: Vec<tinox_codegen::LogMaskFieldInfo> = ann.do_not_serialize_fields
        .iter()
        .map(|f| tinox_codegen::LogMaskFieldInfo {
            class_name: f.class_name.clone(),
            field_name: f.field_name.clone(),
        })
        .collect();
    cg.set_annotation_info(ann.inline_functions, ann.inline_methods, route_entries,
        di_components, ann.log_classes, config_fields, cli_commands, sensitive_fields, masked_fields,
        do_not_serialize_fields, ann.json_serializable_classes, vec![]);
    let entity_entries_test: Vec<tinox_codegen::EntityEntry> = ann.entity_entries
        .iter()
        .map(|e| tinox_codegen::EntityEntry {
            class_name: e.class_name.clone(),
            table_name: e.table_name.clone(),
            fields: e.fields.iter().map(|f| tinox_codegen::EntityFieldEntry {
                field_name: f.field_name.clone(),
                column_name: f.column_name.clone(),
                is_id: f.is_id,
                is_generated: f.is_generated,
                not_null: f.not_null,
                field_llvm_type: f.field_llvm_type.clone(),
            }).collect(),
        })
        .collect();
    cg.set_entity_entries(entity_entries_test);
    cg.set_test_entry(class_name.to_string(), method_name.to_string());
    cg.gen(&ast).map_err(|e| format!("codegen: {e:?}"))?;

    let ir = cg.into_ir();
    let ir_path = format!("{exe}.ll");
    fs::write(&ir_path, ir).map_err(|e| format!("write IR: {e}"))?;
    compile_ll_to_exe(&ir_path, exe, OptLevel::Debug)
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

fn load_dep_dirs() -> Vec<PathBuf> {
    pm::find_project_root()
        .and_then(|root| pm::read_manifest(&root).ok().map(|m| (root, m)))
        .map(|(root, m)| pm::installed_dep_dirs(&root, &m))
        .unwrap_or_default()
}

fn resolve_imports(
    ast: &mut tinox_parser::SourceFile,
    base_dir: &Path,
    visited: &mut HashSet<PathBuf>,
    dep_dirs: &[PathBuf],
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
        // 2. Installed package dependencies (.tinox/deps/...)
        // 3. tinox.core.X  →  <stdlib_dir>/X.tnx
        let full_path = if let Ok(p) = base_dir.join(&rel).canonicalize() {
            p
        } else if let Some(p) = dep_dirs.iter().find_map(|d| d.join(&rel).canonicalize().ok()) {
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
        resolve_imports(&mut imported, &imported_dir, visited, dep_dirs)?;

        ast.decls.extend(imported.decls);
    }

    // Drop Import and Module decls — they are resolved or informational only
    ast.decls
        .retain(|d| !matches!(&d.node, DeclKind::Import(_) | DeclKind::Module(_)));

    Ok(())
}

fn compile_file(input_path: &str, output_name: &str, opt: OptLevel) -> Result<(), String> {
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
    let dep_dirs = load_dep_dirs();
    resolve_imports(&mut ast, &base_dir, &mut visited, &dep_dirs)
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
    let config_fields: Vec<tinox_codegen::ConfigFieldInfo> = ann_result.config_fields
        .iter()
        .map(|f| tinox_codegen::ConfigFieldInfo {
            class_name: f.class_name.clone(),
            field_name: f.field_name.clone(),
            config_key: f.config_key.clone(),
            field_llvm_type: f.field_llvm_type.clone(),
        })
        .collect();
    let cli_commands: Vec<tinox_codegen::CliCommandInfo> = ann_result.cli_commands
        .iter()
        .map(|c| tinox_codegen::CliCommandInfo {
            class_name: c.class_name.clone(),
            cmd_name: c.cmd_name.clone(),
            description: c.description.clone(),
            version: c.version.clone(),
            options: c.options.iter().map(|o| tinox_codegen::CliOptionInfo {
                field_name: o.field_name.clone(),
                names: o.names.clone(),
                description: o.description.clone(),
                required: o.required,
                field_type: o.field_type.clone(),
            }).collect(),
            arguments: c.arguments.iter().map(|a| tinox_codegen::CliArgumentInfo {
                field_name: a.field_name.clone(),
                index: a.index,
                description: a.description.clone(),
                required: a.required,
                field_type: a.field_type.clone(),
            }).collect(),
        })
        .collect();
    let sensitive_fields: Vec<tinox_codegen::LogMaskFieldInfo> = ann_result.sensitive_fields
        .iter()
        .map(|f| tinox_codegen::LogMaskFieldInfo {
            class_name: f.class_name.clone(),
            field_name: f.field_name.clone(),
        })
        .collect();
    let masked_fields: Vec<tinox_codegen::LogMaskFieldInfo> = ann_result.masked_fields
        .iter()
        .map(|f| tinox_codegen::LogMaskFieldInfo {
            class_name: f.class_name.clone(),
            field_name: f.field_name.clone(),
        })
        .collect();
    let do_not_serialize_fields: Vec<tinox_codegen::LogMaskFieldInfo> = ann_result.do_not_serialize_fields
        .iter()
        .map(|f| tinox_codegen::LogMaskFieldInfo {
            class_name: f.class_name.clone(),
            field_name: f.field_name.clone(),
        })
        .collect();
    let metric_entries: Vec<tinox_codegen::MetricEntry> = ann_result.metric_entries
        .iter()
        .map(|m| tinox_codegen::MetricEntry {
            kind: match m.kind {
                tinox_typecheck::annotations::MetricKind::Timed   => tinox_codegen::MetricKind::Timed,
                tinox_typecheck::annotations::MetricKind::Counted => tinox_codegen::MetricKind::Counted,
                tinox_typecheck::annotations::MetricKind::Gauge   => tinox_codegen::MetricKind::Counted, // gauge on fields, handled separately
            },
            metric_name: m.metric_name.clone(),
            class_name:  m.class_name.clone(),
            fn_name:     m.fn_name.clone(),
        })
        .collect();
    codegen.set_annotation_info(ann_result.inline_functions, ann_result.inline_methods, route_entries, di_components, ann_result.log_classes, config_fields, cli_commands, sensitive_fields, masked_fields, do_not_serialize_fields, ann_result.json_serializable_classes, metric_entries);
    codegen.set_metrics_config(read_metrics_config());
    let entity_entries: Vec<tinox_codegen::EntityEntry> = ann_result.entity_entries
        .iter()
        .map(|e| tinox_codegen::EntityEntry {
            class_name: e.class_name.clone(),
            table_name: e.table_name.clone(),
            fields: e.fields.iter().map(|f| tinox_codegen::EntityFieldEntry {
                field_name: f.field_name.clone(),
                column_name: f.column_name.clone(),
                is_id: f.is_id,
                is_generated: f.is_generated,
                not_null: f.not_null,
                field_llvm_type: f.field_llvm_type.clone(),
            }).collect(),
        })
        .collect();
    codegen.set_entity_entries(entity_entries);
    codegen.set_db_url(read_database_config().map(|c| c.url));
    codegen
        .gen(&ast)
        .map_err(|e| format!("Codegen error: {:?}", e))?;

    let ir = codegen.into_ir();
    let ir_path = format!("{}.ll", output_name);
    fs::write(&ir_path, ir).map_err(|e| format!("Failed to write IR: {}", e))?;

    compile_ll_to_exe(&ir_path, output_name, opt)
}

fn compile_ll_to_exe(ir_path: &str, output_name: &str, opt: OptLevel) -> Result<(), String> {
    let obj_path = format!("{}.o", output_name);

    let (llc_opt_flag, opt_flag) = match opt {
        OptLevel::Release => ("-O3", "-O3"),
        OptLevel::Debug   => ("-O0", "-O0"),
    };

    // In Release mode, try opt for mid-level optimizations before llc.
    // In Debug mode skip opt entirely for faster compile times.
    let opt_available = opt == OptLevel::Release && Command::new("opt")
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
            .args(&[opt_flag, "-o", &bc_path, ir_path])
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
            llc_opt_flag,
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

    let db_cfg = read_database_config();
    let db_driver = db_cfg.as_ref().map(|c| c.driver.as_str()).unwrap_or("");

    let mut cc_args = vec!["-c", &runtime_src, "-o", &runtime_obj, "-O3"];
    if db_driver == "postgres" {
        cc_args.push("-DTINOX_DB_POSTGRES");
    } else if db_driver == "mysql" {
        cc_args.push("-DTINOX_DB_MYSQL");
    } else if db_driver == "sqlite" {
        cc_args.push("-DTINOX_DB_SQLITE");
    }
    let cc_status = Command::new("cc")
        .args(&cc_args)
        .status()
        .map_err(|e| format!("Failed to compile runtime: {}", e))?;

    if !cc_status.success() {
        return Err("Runtime compilation failed".to_string());
    }

    let mut link_args = vec![obj_path.as_str(), runtime_obj.as_str(), "-o", output_name, "-lm", "-lpthread", "-lgc", "-no-pie"];
    if db_driver == "postgres" {
        link_args.push("-lpq");
    } else if db_driver == "mysql" {
        link_args.push("-lmysqlclient");
    } else if db_driver == "sqlite" {
        link_args.push("-lsqlite3");
    }
    let link_status = Command::new("cc")
        .args(&link_args)
        .status()
        .map_err(|e| format!("Failed to link: {}", e))?;

    if !link_status.success() {
        return Err("Linking failed".to_string());
    }

    let _ = fs::remove_file(&obj_path);
    let _ = fs::remove_file(&runtime_obj);

    Ok(())
}
