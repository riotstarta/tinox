use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tinox_codegen::CodeGen;
use tinox_lexer::Lexer;
use tinox_parser::{DeclKind, Formatter, Parser};

mod pm;

fn main() {
    // Run the compiler on a thread with a large stack. The parser, type checker
    // and code generator all recurse over the AST, so deeply nested (or maliciously
    // deep) input can overflow the default 8 MB main-thread stack. A 512 MB stack
    // pushes the safe nesting depth far beyond any real program; the parser's own
    // MAX_RECURSION_DEPTH guard rejects the truly pathological case with a clean
    // error before even this stack is exhausted.
    let child = std::thread::Builder::new()
        .stack_size(512 * 1024 * 1024)
        .spawn(run)
        .expect("failed to spawn compiler thread");
    // Propagate a panic in the worker as a non-zero exit (no double-panic noise).
    if child.join().is_err() {
        std::process::exit(101);
    }
}

fn run() {
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
        "install" => pm::cmd_install(&args[2..]),
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
    println!("  tinox install --update     Re-pin tinox.lock instead of verifying against it");
    println!("  tinox add <g> <a> <v> <u>  Add a dependency and install it");
    println!("  tinox package              Pack src/ into <name>-<version>.tar.gz");
    println!("  tinox help                 Show this help message");
}

/// The scaffolded project's file contents — `(tinox.toml, src/Main.tnx,
/// test class name, tests/{test class name}.tnx)`. Pure/pathless so it's
/// unit-testable without touching the filesystem or CWD (`new_project`
/// below writes these to disk relative to CWD, which isn't safely
/// testable in a parallel test binary).
///
/// Both `src/Main.tnx` (`class Main { fnc main() -> Int32 { ... } }`) and
/// the entry point (`class Main` in a file literally named `Main.tnx`)
/// follow the one-class-per-file rule and the mandatory class-qualified
/// entry point (#149) — a bare top-level `fn main()` (this scaffold's
/// pre-v2.0.0 shape) is now a hard compile error (#155). The test
/// scaffold's file name likewise has to match its `class {name}Tests`
/// declaration (#159).
fn new_project_files(name: &str) -> (String, String, String, String) {
    let toml = format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\ndescription = \"\"\nentry = \"src/Main.tnx\"\n"
    );
    let main_tnx = format!(
        "class Main\n{{\n    fnc main() -> Int32\n    {{\n        println(\"Hello from {name}!\");\n        return 0;\n    }}\n}}\n"
    );
    let test_class = format!("{name}Tests");
    let test_tnx = format!(
        "class {test_class}\n{{\n    @Test(\"example test\")\n    fn testExample() -> Bool\n    {{\n        return 1 + 1 == 2;\n    }}\n}}\n"
    );
    (toml, main_tnx, test_class, test_tnx)
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

    let (toml, main_tnx, test_class, test_tnx) = new_project_files(&name);
    let gitignore = ".tinox/\n";

    if !write_file(&root.join("tinox.toml"), &toml) { return; }
    if !write_file(&root.join(".gitignore"), gitignore) { return; }
    if !write_file(&src_dir.join("Main.tnx"), &main_tnx) { return; }
    if !write_file(&tests_dir.join(format!("{test_class}.tnx")), &test_tnx) { return; }

    println!("Created project '{name}'");
    println!("  {name}/tinox.toml");
    println!("  {name}/src/Main.tnx");
    println!("  {name}/tests/{test_class}.tnx");
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
/// Read `entry` from the nearest `tinox.toml`'s `[package]` section, if present.
fn read_project_entry(content: &str) -> Option<String> {
    let mut in_package = false;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if !in_package { continue; }
        if let Some(rest) = line.strip_prefix("entry") {
            let rest = rest.trim();
            if let Some(rest) = rest.strip_prefix('=') {
                let entry = rest.trim().trim_matches('"').to_string();
                if !entry.is_empty() {
                    return Some(entry);
                }
            }
        }
    }
    None
}

/// If `args` has a file, use that. Otherwise read tinox.toml → its
/// `[package] entry` field (defaulting to `src/main.tnx` if unset).
fn resolve_entry_file(args: &[String]) -> Option<String> {
    if let Some(f) = args.iter().find(|a| !a.starts_with('-')) {
        return Some(f.clone());
    }
    // Project mode: look for tinox.toml in current dir or parents
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let toml = dir.join("tinox.toml");
        if toml.exists() {
            let content = fs::read_to_string(&toml).ok()?;
            let entry = read_project_entry(&content).unwrap_or_else(|| "src/main.tnx".to_string());
            let candidate = dir.join(&entry);
            if candidate.exists() {
                return Some(candidate.to_string_lossy().into_owned());
            }
            eprintln!("error: tinox.toml found but {entry} is missing");
            return None;
        }
        if !dir.pop() { break; }
    }
    eprintln!("error: no input file and no tinox.toml found");
    None
}

/// `--checked` (TESTPLAN Phase 4): Runtime mit Heap-Kind-Registry bauen —
/// Array-/Map-Funktionen prüfen ihre Pointer und brechen bei
/// Dispatch-Bugs laut ab, statt still Müll zu lesen. Implementiert über
/// TINOX_CFLAGS, das compile_ll_to_exe an beide cc-Aufrufe durchreicht.
fn apply_checked_flag(args: &[String]) {
    if args.iter().any(|a| a == "--checked") {
        let mut flags = std::env::var("TINOX_CFLAGS").unwrap_or_default();
        if !flags.contains("-DTINOX_CHECKED") {
            if !flags.is_empty() {
                flags.push(' ');
            }
            flags.push_str("-DTINOX_CHECKED");
            std::env::set_var("TINOX_CFLAGS", flags);
        }
    }
}

fn build(args: &[String]) {
    let release = args.iter().any(|a| a == "--release");
    let debug   = args.iter().any(|a| a == "--debug");
    apply_checked_flag(args);
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
        Err(e) => {
            eprintln!("Compilation failed: {}", e);
            std::process::exit(1);
        }
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
            let first = input_buf.split_whitespace().next().unwrap_or("");
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

    let is_stmt_block = is_multi_line || has_semicolons || starts_with_stmt;

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
    cmd.arg(&ir_path).arg("-o").arg(&exe).arg("-O0").arg("-lm").arg("-lgc").arg("-lz");
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
    // Same dev/system resolution used by the main build path (compile_ll_to_exe).
    if let Some(c) = runtime_c_path() {
        let obj = "/tmp/.tinox_runtime.o";
        let status = Command::new("clang")
            .args(["-c", &c.to_string_lossy(), "-o", obj, "-O3"])
            .status().ok()?;
        if status.success() { return Some(obj.to_string()); }
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
    apply_checked_flag(args);
    match compile_file(&input_file, &exe_name, opt) {
        Ok(_) => {
            let status = Command::new(format!("./{}", exe_name))
                .status()
                .expect("Failed to run executable");

            let _ = fs::remove_file(&exe_name);
            let _ = fs::remove_file(format!("{}.ll", exe_name));

            std::process::exit(status.code().unwrap_or(1));
        }
        Err(e) => {
            eprintln!("Compilation failed: {}", e);
            std::process::exit(1);
        }
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
    if let Err(e) = check_one_type_per_file(&ast.decls, Path::new(&input_file)) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
    if let Err(e) = check_no_top_level_fn(&ast.decls, Path::new(&input_file)) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }

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

    // Assign node ids before type-checking so infer_type's memoization is active
    // (Bug 50) — without ids every sub-expression is re-inferred, making deep
    // method chains exponential. The build path already does this before check.
    tinox_parser::assign_node_ids(&mut ast);

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
            if let Err(e) = result {
                println!("  FAIL  {} — compile error: {}", t.description, e);
                failed += 1;
                continue;
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

// ─── tinox doc ────────────────────────────────────────────────────────────────

/// Recursively collects every .tnx file under `dir` — a multi-file module
/// (e.g. tinox-core's `websocket/` with Ws.tnx/WsClient.tnx/WsFrame.tnx/
/// WsServer.tnx as siblings) needs all of them merged into one doc page,
/// not just whichever file happens to sort first.
fn collect_tnx_files_for_docs(dir: &Path, out: &mut Vec<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect_tnx_files_for_docs(&path, out);
        } else if path.extension().map(|x| x == "tnx").unwrap_or(false) {
            out.push(path.to_string_lossy().into_owned());
        }
    }
}

fn gen_docs(args: &[String]) {
    let open = args.iter().any(|a| a == "--open");
    let out_override: Option<&String> = args.iter()
        .position(|a| a == "--out")
        .and_then(|i| args.get(i + 1));

    // Collect source files from project or explicit arg, and remember the
    // project root along the way — description/dependencies/examples below
    // all read from files relative to it, same as tinox.toml itself.
    let mut project_root: Option<PathBuf> = None;
    let source_files: Vec<String> = if let Some(f) = args.first().filter(|a| !a.starts_with('-')) {
        vec![f.clone()]
    } else {
        let mut files = Vec::new();
        let mut dir = std::env::current_dir().unwrap_or_default();
        loop {
            if dir.join("tinox.toml").exists() {
                let src = dir.join("src");
                if src.is_dir() {
                    collect_tnx_files_for_docs(&src, &mut files);
                }
                project_root = Some(dir);
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

    // Description + declared dependencies come straight from tinox.toml —
    // real project metadata, not re-derived/guessed. Examples are read from
    // an `examples/*.tnx` directory next to `src/`, one file per example,
    // sorted by filename so an author can order them (01_basic.tnx, ...).
    let (description, dependencies) = match &project_root {
        Some(root) => match pm::read_manifest(root) {
            Ok(m) => (
                m.package.as_ref().map(|p| p.description.clone()).filter(|d| !d.is_empty()),
                m.dependencies,
            ),
            Err(_) => (None, Vec::new()),
        },
        None => (None, Vec::new()),
    };
    let examples: Vec<(String, String)> = project_root.as_ref()
        .map(|root| root.join("examples"))
        .filter(|dir| dir.is_dir())
        .map(|dir| {
            let mut files: Vec<PathBuf> = fs::read_dir(&dir)
                .map(|entries| entries.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().map(|x| x == "tnx").unwrap_or(false))
                    .collect())
                .unwrap_or_default();
            files.sort();
            files.into_iter()
                .filter_map(|p| {
                    let src = fs::read_to_string(&p).ok()?;
                    let stem = p.file_stem()?.to_string_lossy().into_owned();
                    Some((humanize_example_name(&stem), src))
                })
                .collect()
        })
        .unwrap_or_default();

    let html = render_docs_html(&project_name, description.as_deref(), &dependencies, &examples, &doc_items);

    let out_path = match out_override {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from("docs").join("index.html"),
    };
    if let Some(parent) = out_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        if let Err(e) = fs::create_dir_all(parent) {
            eprintln!("error: cannot create {}: {}", parent.display(), e);
            return;
        }
    }
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

fn render_docs_html(
    project_name: &str,
    description: Option<&str>,
    dependencies: &[pm::Dependency],
    examples: &[(String, String)],
    items: &[DocItem],
) -> String {
    let mut nav = String::new();
    let mut body = String::new();

    // 1) Description, 2) Dependencies, 3) Examples, 4) the existing
    // class/interface/function reference — in that fixed order, matching
    // "what it is → what it needs → how to use it → full API".
    if let Some(desc) = description {
        nav.push_str("<li class=\"nav-section\">Overview</li><li><a href=\"#overview\">Description</a></li>");
        body.push_str(&format!(
            "<section id=\"overview\" class=\"item\"><p class=\"doc\" style=\"margin-bottom:0\">{}</p></section>",
            html_escape(desc)
        ));
    }

    if !dependencies.is_empty() {
        nav.push_str("<li class=\"nav-section\">Dependencies</li><li><a href=\"#dependencies\">Dependencies</a></li>");
        let rows: String = dependencies.iter().map(|d| {
            // Sibling docs.html, one directory per artifactId — matches
            // how these pages are actually laid out (docs/tinox-core/<mod>/docs.html).
            format!(
                "<tr><td class=\"member-name\"><a href=\"../{}/docs.html\"><code>{}</code></a></td><td class=\"member-type\"><code>{}</code></td><td>{}</td></tr>",
                html_escape(&d.artifact_id), html_escape(&d.artifact_id), html_escape(&d.version), html_escape(&d.group)
            )
        }).collect();
        body.push_str(&format!(
            "<section id=\"dependencies\" class=\"item\"><table class=\"members\"><tr><th style=\"text-align:left;color:var(--text3);font-size:0.75rem;text-transform:uppercase;padding-bottom:6px\">Module</th><th style=\"text-align:left;color:var(--text3);font-size:0.75rem;text-transform:uppercase;padding-bottom:6px\">Version</th><th style=\"text-align:left;color:var(--text3);font-size:0.75rem;text-transform:uppercase;padding-bottom:6px\">Group</th></tr>{}</table></section>",
            rows
        ));
    }

    if !examples.is_empty() {
        nav.push_str("<li class=\"nav-section\">Examples</li>");
        let mut ex_body = String::new();
        for (title, src) in examples {
            let slug: String = title.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
            nav.push_str(&format!("<li><a href=\"#ex-{slug}\">{}</a></li>", html_escape(title)));
            ex_body.push_str(&format!(
                "<section id=\"ex-{slug}\" class=\"item\"><h3 style=\"margin-top:0\">{}</h3><pre><code>{}</code></pre></section>",
                html_escape(title), highlight_tinox_source(src)
            ));
        }
        body.push_str(&ex_body);
    }

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

    // Same palette/layout conventions as docs_en.html (the hand-written
    // language reference) so auto-generated per-module doc pages read as
    // one consistent site rather than a visibly different tool's output.
    format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{project_name} — Tinox Docs</title>
<style>
:root {{
  --bg:        #0f1117;
  --bg2:       #171b26;
  --bg3:       #1e2333;
  --sidebar:   #13161f;
  --border:    #2a2f42;
  --accent:    #5b8ff9;
  --accent2:   #7c5bf9;
  --green:     #4ecb71;
  --text:      #dde1f0;
  --text2:     #8a91aa;
  --text3:     #5a6080;
  --code-bg:   #0b0e18;
  --tag-bg:    #1a2540;
}}
* {{ box-sizing: border-box; margin: 0; padding: 0; }}
body {{ font-family: 'Segoe UI', system-ui, sans-serif; background: var(--bg); color: var(--text); line-height: 1.7; display: flex; min-height: 100vh; }}
nav {{ width: 270px; min-width: 270px; background: var(--sidebar); border-right: 1px solid var(--border); padding: 32px 0; position: sticky; top: 0; height: 100vh; overflow-y: auto; flex-shrink: 0; }}
nav h1 {{ padding: 0 24px 20px; border-bottom: 1px solid var(--border); margin-bottom: 12px; font-size: 1.2rem; font-weight: 700; letter-spacing: -0.5px; color: #fff; }}
nav ul {{ list-style: none; }}
nav li.nav-section {{ font-size: 0.65rem; font-weight: 700; text-transform: uppercase; letter-spacing: 1.2px; color: var(--text3); padding: 16px 24px 6px; }}
nav li a {{ display: block; padding: 7px 24px; color: var(--text2); text-decoration: none; font-size: 0.88rem; border-left: 2px solid transparent; transition: all 0.15s; }}
nav li a:hover {{ color: var(--text); background: var(--bg3); border-left-color: var(--accent); }}
main {{ flex: 1; max-width: 920px; padding: 56px 64px; }}
main h1 {{ font-size: 1.9rem; font-weight: 700; color: #fff; margin-bottom: 8px; letter-spacing: -0.4px; }}
main > p {{ color: var(--text2); margin-bottom: 32px; font-size: 0.88rem; }}
.item {{ border: 1px solid var(--border); border-radius: 10px; padding: 24px; margin-bottom: 24px; background: var(--bg2); }}
.item-name {{ font-size: 1.1rem; font-weight: 600; color: var(--text); margin-bottom: 12px; font-family: 'Fira Code', 'Cascadia Code', monospace; }}
.kw {{ color: #c792ea; }}
.doc {{ color: var(--text2); font-size: 0.88rem; margin-bottom: 16px; line-height: 1.6; }}
h3 {{ font-size: 0.8rem; color: var(--text3); text-transform: uppercase; letter-spacing: 0.06em; margin: 20px 0 8px; font-weight: 700; }}
table.members {{ width: 100%; border-collapse: collapse; font-size: 0.85rem; }}
table.members tr {{ border-bottom: 1px solid var(--border); }}
table.members td {{ padding: 8px 10px; vertical-align: top; }}
.member-name {{ font-family: 'Fira Code', 'Cascadia Code', monospace; color: var(--text); width: 35%; }}
.member-type {{ color: #ffcb6b; width: 20%; }}
.method-sig {{ background: var(--code-bg); border-radius: 6px; padding: 10px 14px; font-family: 'Fira Code', 'Cascadia Code', monospace; font-size: 0.85rem; color: var(--text); margin-bottom: 8px; border: 1px solid var(--border); }}
.method-doc {{ color: var(--text2); font-size: 0.85rem; margin-bottom: 12px; line-height: 1.5; }}
.ann {{ color: var(--green); font-size: 0.78rem; display: inline-block; margin-right: 6px; }}
code {{ font-family: 'Fira Code', 'Cascadia Code', monospace; font-size: 0.85em; background: var(--code-bg); border: 1px solid var(--border); border-radius: 4px; padding: 2px 6px; color: #a8d0ff; }}
a {{ color: var(--accent); text-decoration: none; }}
a:hover {{ text-decoration: underline; }}
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
        .map(|p| format!("{}: <span style=\"color:#ffcb6b\">{}</span>", html_escape(&p.name), html_escape(&p.ty)))
        .collect();
    format!("{}({}) → <span style=\"color:#ffcb6b\">{}</span>", html_escape(name), ps.join(", "), html_escape(ret))
}

fn render_annotations(anns: &[String]) -> String {
    anns.iter().map(|a| format!("<span class=\"ann\">@{a}</span>")).collect()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// `01_basic_publish.tnx` → `Basic publish`; `-`/`_` become spaces, a
/// leading numeric ordering prefix (`01_`, `2-`) is dropped, first letter
/// capitalized. Falls back to the stem itself if that leaves nothing.
fn humanize_example_name(stem: &str) -> String {
    let no_prefix = stem.trim_start_matches(|c: char| c.is_ascii_digit())
        .trim_start_matches(['_', '-']);
    let words: Vec<&str> = if no_prefix.is_empty() { stem } else { no_prefix }
        .split(['_', '-'])
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() {
        return stem.to_string();
    }
    let mut out = String::new();
    for (i, w) in words.iter().enumerate() {
        if i > 0 { out.push(' '); }
        if i == 0 {
            let mut chars = w.chars();
            if let Some(c) = chars.next() {
                out.extend(c.to_uppercase());
                out.push_str(chars.as_str());
            }
        } else {
            out.push_str(w);
        }
    }
    out
}

/// Real-lexer-based syntax highlighting for example code blocks — reuses
/// `tinox_lexer::Lexer` (the same tokenizer the compiler itself runs)
/// rather than a regex approximation, so keywords/strings/comments/numbers
/// are colored exactly per the real grammar, not guessed. Falls back to
/// plain escaped text if the example doesn't lex cleanly. Types aren't a
/// distinct token kind in this lexer, so a capitalized identifier is
/// treated as one — matches this codebase's own PascalCase-for-types,
/// camelCase-for-values convention throughout tinox-core.
fn highlight_tinox_source(src: &str) -> String {
    use tinox_lexer::TokenKind;

    let mut lexer = Lexer::new(src);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        Err(_) => return html_escape(src),
    };

    let mut out = String::new();
    let mut pos = 0usize;
    for tok in &tokens {
        let start = tok.span.start.offset as usize;
        let end = tok.span.end.offset as usize;
        if start > pos && start <= src.len() {
            out.push_str(&html_escape(&src[pos..start]));
        }
        if start > end || end > src.len() {
            continue;
        }
        let text = &src[start..end];
        let escaped = html_escape(text);
        let class = match &tok.kind {
            TokenKind::Keyword(_) | TokenKind::Bool(_) => Some("kw"),
            TokenKind::String(_) | TokenKind::RawString(_) | TokenKind::InterpString(_) | TokenKind::Char(_) => Some("str"),
            TokenKind::Integer(_) | TokenKind::Float(_) | TokenKind::IntegerSuffix(_) | TokenKind::FloatSuffix(_) => Some("num"),
            TokenKind::Comment(_) | TokenKind::DocComment(_) => Some("cmt"),
            TokenKind::Ident(name) if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) => Some("type"),
            _ => None,
        };
        match class {
            Some(c) => out.push_str(&format!("<span class=\"{}\">{}</span>", c, escaped)),
            None => out.push_str(&escaped),
        }
        pos = end.max(pos);
    }
    if pos < src.len() {
        out.push_str(&html_escape(&src[pos..]));
    }
    out
}

/// Parse a file and return all @Test entries without compiling.
fn collect_tests(path: &str) -> Result<Vec<tinox_typecheck::annotations::TestInfo>, String> {
    let source = fs::read_to_string(path)
        .map_err(|e| format!("cannot read: {e}"))?;
    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize().map_err(|e| format!("lex error: {e:?}"))?;
    let mut parser = tinox_parser::Parser::new(tokens);
    let mut ast = parser.parse().map_err(|e| format!("parse error: {e:?}"))?;
    check_one_type_per_file(&ast.decls, Path::new(path))?;
    check_no_top_level_fn(&ast.decls, Path::new(path))?;
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
    check_one_type_per_file(&ast.decls, Path::new(source))?;
    check_no_top_level_fn(&ast.decls, Path::new(source))?;

    let base = Path::new(source).parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut visited = HashSet::new();
    if let Ok(c) = Path::new(source).canonicalize() { visited.insert(c); }
    let dep_dirs = load_dep_dirs();
    resolve_imports(&mut ast, &base, &mut visited, &dep_dirs)?;
    tinox_parser::assign_node_ids(&mut ast);

    let mut tc = tinox_typecheck::TypeChecker::new();
    tc.check(&ast).map_err(|e| format!("type error:\n{e}"))?;
    let (iface, impls) = tc.interface_info();

    let ann = tinox_typecheck::annotations::process_annotations(&ast);

    let route_entries = ann.route_entries.iter().map(|r| tinox_codegen::RouteEntry {
        http_method: r.method.clone(), path: r.path.clone(),
        class_name: r.class_name.clone(), method_name: r.method_name.clone(),
        status_code: r.status_code, produces: r.produces.clone(),
        consumes: r.consumes.clone(), auth_type: r.auth_type.clone(),
        oidc_roles: r.oidc_roles.clone(),
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
    cg.set_expr_value_types(tc.expr_value_types());
    cg.set_interface_info(iface, impls);
    let do_not_serialize_fields: Vec<tinox_codegen::LogMaskFieldInfo> = ann.do_not_serialize_fields
        .iter()
        .map(|f| tinox_codegen::LogMaskFieldInfo {
            class_name: f.class_name.clone(),
            field_name: f.field_name.clone(),
        })
        .collect();
    cg.set_annotation_info(tinox_codegen::AnnotationInfo {
        inline_fns: ann.inline_functions,
        inline_meths: ann.inline_methods,
        routes: route_entries,
        di_components,
        log_classes: ann.log_classes,
        config_fields,
        cli_commands,
        sensitive_fields,
        masked_fields,
        do_not_serialize_fields,
        json_serializable_classes: ann.json_serializable_classes,
        metric_entries: vec![],
    });
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
/// Checks TINOX_PATH env var first, then the path relative to this binary's
/// source location (works for `cargo run` during development), then the
/// fixed system install path used by distro packages (e.g. the AUR
/// `tinox-bin` package installs tinox-core to /usr/share/tinox/core).
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
    let system = PathBuf::from("/usr/share/tinox/core");
    if system.is_dir() {
        return Some(system);
    }
    None
}

/// Returns the path to runtime.c: the dev-checkout path relative to this
/// binary's compiled-in source location (works for `cargo run` during
/// development), then the fixed system install path used by distro
/// packages. Unlike `stdlib_dir`, there is no env var override — runtime.c
/// is an implementation detail, not something a user is expected to point
/// at directly.
fn runtime_c_path() -> Option<PathBuf> {
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../runtime/runtime.c");
    if dev.is_file() {
        return Some(dev);
    }
    let system = PathBuf::from("/usr/share/tinox/runtime.c");
    if system.is_file() {
        return Some(system);
    }
    None
}

fn load_dep_dirs() -> Vec<PathBuf> {
    pm::find_project_root()
        .and_then(|root| pm::read_manifest(&root).ok().map(|m| (root, m)))
        .map(|(root, m)| pm::installed_dep_dirs(&root, &m))
        .unwrap_or_default()
}

/// Collects the names of every top-level `class`/`interface`/`enum` in a
/// single file's own decls, descending into `namespace { ... }` wrappers
/// (the stdlib's `namespace tinox.core.X { class Y { ... } }` shape) since
/// those are organizational, not a second nesting level from the user's
/// perspective. Order matches declaration order.
fn collect_type_decl_names(decls: &[tinox_parser::Decl]) -> Vec<&str> {
    let mut names = Vec::new();
    for d in decls {
        match &d.node {
            DeclKind::Class(c) => names.push(c.name.as_str()),
            DeclKind::Interface(i) => names.push(i.name.as_str()),
            DeclKind::Enum(e) => names.push(e.name.as_str()),
            DeclKind::Namespace(ns) => names.extend(collect_type_decl_names(&ns.decls)),
            _ => {}
        }
    }
    names
}

/// Enforces "at most one top-level class/interface/enum per file, and if
/// there is one, the file must be named exactly after it" (case-sensitive).
/// Must run on a SINGLE file's own (pre-merge) decls, before those decls are
/// merged into the importer — once merged, a decl's originating file can no
/// longer be determined (`Spanned<T>` carries no filename). Wired into
/// `resolve_imports` (for every imported file) and `check`/`compile_file`
/// (for the entry file).
fn check_one_type_per_file(decls: &[tinox_parser::Decl], path: &Path) -> Result<(), String> {
    let names = collect_type_decl_names(decls);
    match names.as_slice() {
        [] => Ok(()),
        [only] => {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if *only == stem {
                Ok(())
            } else {
                Err(format!(
                    "'{}' declares '{}', but the file must be named '{}.tnx' (one type per file, filename must match exactly)",
                    path.display(),
                    only,
                    only
                ))
            }
        }
        many => Err(format!(
            "'{}' declares {} types ({}), but only one class/interface/enum is allowed per file — split it into separate files",
            path.display(),
            many.len(),
            many.join(", ")
        )),
    }
}

/// Collects the names of every top-level free `fn` WITH A BODY in a single
/// file's own decls (descending into `namespace { ... }` like
/// `collect_type_decl_names` does). `extern fn` declarations are excluded
/// (`StmtKind::Empty` is the parser's marker for a body-less
/// declare-only signature, confirmed in `tinox-codegen`'s `gen_fn`) —
/// those are FFI bindings to `runtime.c`, not free functions in the
/// issue #149 sense, and stay legal.
fn collect_top_level_fn_names(decls: &[tinox_parser::Decl]) -> Vec<&str> {
    let mut names = Vec::new();
    for d in decls {
        match &d.node {
            DeclKind::Function(f) if !matches!(f.body.node, tinox_parser::StmtKind::Empty) => {
                names.push(f.name.as_str())
            }
            DeclKind::Namespace(ns) => names.extend(collect_top_level_fn_names(&ns.decls)),
            _ => {}
        }
    }
    names
}

/// Issue #149 stage 3: hard-enforces "no top-level `fn` with a body" — the
/// language has no implicit global function namespace anymore, every
/// function must be a class method (`fn`/`fnc`). Mirrors
/// `check_one_type_per_file` exactly: must run on a SINGLE file's own
/// (pre-merge) decls for the same reason (a decl's originating file can't
/// be recovered after `resolve_imports` merges everything), and is wired
/// into the identical call sites (`resolve_imports` for every imported
/// file, `check`/`compile_file`/test-mode entry points for the entry
/// file).
fn check_no_top_level_fn(decls: &[tinox_parser::Decl], path: &Path) -> Result<(), String> {
    let names = collect_top_level_fn_names(decls);
    if names.is_empty() {
        return Ok(());
    }
    Err(format!(
        "'{}' declares {} top-level function{} ({}) — Tinox no longer allows free functions outside a class; move {} into a class as a `fnc` (static) or `fn` (instance) method",
        path.display(),
        names.len(),
        if names.len() == 1 { "" } else { "s" },
        names.join(", "),
        if names.len() == 1 { "it" } else { "them" }
    ))
}

/// Stamps every `Function`/`Class` method's `file` field with `path`
/// (issue #114): the parser has no notion of a filename (`Parser::new`
/// only sees a token stream), so it always leaves `file` at
/// `tinox_parser::UNKNOWN_FILE`. This is the one place — called right
/// after each individual file is parsed, both for the entry file
/// (`compile_file`) and every imported file (`resolve_imports`), BEFORE
/// `resolve_imports` merges everything into one flat decl list — where
/// the real path is actually known. Recurses into `Namespace` decls
/// (matches `CodeGen::gen`'s own decl-walking for `gen_fn`/
/// `gen_class_method`, the only two codegen sites that read `file`).
/// Uses the canonicalized absolute path so DWARF's `!DIFile` directory/
/// filename split (`tinox-codegen`) is well-defined regardless of the
/// cwd `tinox build` was invoked from.
fn stamp_file_identity(decls: &mut [tinox_parser::Decl], path: &Path) {
    let file: std::sync::Arc<str> = std::sync::Arc::from(
        path.canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .into_owned(),
    );
    stamp_file_identity_with(decls, &file);
}

fn stamp_file_identity_with(decls: &mut [tinox_parser::Decl], file: &std::sync::Arc<str>) {
    for decl in decls {
        match &mut decl.node {
            DeclKind::Function(f) => f.file = file.clone(),
            DeclKind::Class(c) => {
                for m in &mut c.methods {
                    m.file = file.clone();
                }
            }
            DeclKind::Namespace(ns) => stamp_file_identity_with(&mut ns.decls, file),
            _ => {}
        }
    }
}

/// Resolves a module reference to a list of source files: prefers a single
/// `<name>.tnx` file (legacy / not-yet-migrated modules); if that doesn't
/// exist, falls back to a `<name>/` directory containing one `.tnx` file per
/// top-level type (one-type-per-file convention, Issue: filename must match
/// its type). Returns `Ok(None)` if neither a matching file nor directory
/// exists under `base`; `Err` if the directory exists but is empty/unreadable.
fn resolve_module_paths(
    base: &Path,
    rel_file: &Path,
    rel_dir: &Path,
) -> Result<Option<Vec<PathBuf>>, String> {
    if let Ok(p) = base.join(rel_file).canonicalize() {
        return Ok(Some(vec![p]));
    }
    let dir = base.join(rel_dir);
    if dir.is_dir() {
        let mut files: Vec<PathBuf> = fs::read_dir(&dir)
            .map_err(|e| format!("Cannot read module directory '{}': {}", dir.display(), e))?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|p| p.extension().map(|e| e == "tnx").unwrap_or(false))
            .collect();
        if files.is_empty() {
            return Err(format!("Module directory '{}' contains no .tnx files", dir.display()));
        }
        files.sort();
        let canon: Result<Vec<PathBuf>, String> = files
            .iter()
            .map(|p| p.canonicalize().map_err(|e| format!("Cannot resolve '{}': {}", p.display(), e)))
            .collect();
        return canon.map(Some);
    }
    Ok(None)
}

/// Best-effort `group:artifactId:version` label for an installed
/// dependency directory (`.tinox/deps/<group>/<artifactId>/<version>/`,
/// see `pm::dep_install_dir`), for the ambiguous-import diagnostic below.
/// Falls back to the raw path if it doesn't have that shape (defensive
/// only — every entry `installed_dep_dirs` produces does).
fn dep_dir_coordinate(dep_dir: &Path) -> String {
    let parts: Vec<&str> = dep_dir
        .components()
        .rev()
        .take(3)
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    if parts.len() == 3 {
        format!("{}:{}:{}", parts[2], parts[1], parts[0])
    } else {
        dep_dir.display().to_string()
    }
}

/// Resolves `rel_file`/`rel_dir` against every installed dependency
/// directory, requiring **at most one** to match. Two dependencies
/// shipping a module at the same relative path used to resolve via
/// `.find_map` — first (manifest-declaration-order) match silently wins,
/// the other is shadowed with no diagnostic of any kind. That's exactly
/// the shape of bug this project's own "no silent garbage" principle
/// (CLAUDE.md) exists to prevent, so an ambiguity is now a hard error
/// instead (#156) — a per-dependency resolution error (e.g. an empty
/// module directory) still doesn't count as a match and doesn't block
/// resolution via a different dependency, unchanged from before.
fn resolve_in_dep_dirs(
    dep_dirs: &[PathBuf],
    rel_file: &Path,
    rel_dir: &Path,
) -> Result<Option<Vec<PathBuf>>, String> {
    let matches: Vec<(&PathBuf, Vec<PathBuf>)> = dep_dirs
        .iter()
        .filter_map(|d| resolve_module_paths(d, rel_file, rel_dir).ok().flatten().map(|p| (d, p)))
        .collect();
    match matches.len() {
        0 => Ok(None),
        1 => Ok(Some(matches.into_iter().next().unwrap().1)),
        _ => {
            let coords: Vec<String> = matches.iter().map(|(d, _)| dep_dir_coordinate(d)).collect();
            Err(format!(
                "Ambiguous import '{}': resolves in more than one installed dependency ({}). \
                 Remove or rename one of them so their module paths don't collide.",
                rel_file.display(),
                coords.join(", "),
            ))
        }
    }
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

    // Collected separately and prepended (not appended) below: the
    // typechecker does a single linear pass over decls with no forward-
    // declaration/hoisting pass for interface-implementation records
    // (`interface_implementations` is populated lazily inside `check_class`
    // as each class is visited, see tinox-typecheck/src/lib.rs) — a `main()`
    // that upcasts an imported class to an imported interface it implements
    // must see both of those decls EARLIER in the list than `main` itself,
    // otherwise `types_compatible` sees an empty implements-record and
    // rejects the assignment. Single-file programs always satisfied this by
    // convention (types declared above `main`); merging via simple `extend`
    // put every import AFTER the importing file's own decls instead, which
    // broke exactly this pattern once one-type-per-file split types and
    // their `main()` driver across separate files.
    let mut imported_decls: Vec<tinox_parser::Decl> = Vec::new();

    for import in imports {
        // ["foo", "bar"] → "foo/bar.tnx" (single-file module) or "foo/bar/"
        // (directory module, one .tnx per top-level type) relative to base_dir.
        let mut rel_file = PathBuf::new();
        let mut rel_dir = PathBuf::new();
        for (i, seg) in import.path.iter().enumerate() {
            if i == import.path.len() - 1 {
                rel_file.push(format!("{}.tnx", seg));
                rel_dir.push(seg);
            } else {
                rel_file.push(seg);
                rel_dir.push(seg);
            }
        }

        // Resolution order:
        // 1. Relative to source file directory
        // 2. Installed package dependencies (.tinox/deps/...)
        // 3. tinox.core.X  →  <stdlib_dir>/X.tnx or <stdlib_dir>/X/*.tnx
        //    tinox.core.X.Y  →  <stdlib_dir>/X/Y.tnx or <stdlib_dir>/X/Y/*.tnx
        //    (everything after "tinox.core" nests as a subdirectory of the
        //    stdlib dir, same rule as the relative-import case above; when
        //    there's no "core" segment to anchor on, falls back to just the
        //    last segment, unchanged from before this nesting support)
        let full_paths: Vec<PathBuf> = if let Some(p) = resolve_module_paths(base_dir, &rel_file, &rel_dir)? {
            p
        } else if let Some(p) = resolve_in_dep_dirs(dep_dirs, &rel_file, &rel_dir)? {
            p
        } else if import.path.first().map(|s| s == "tinox").unwrap_or(false) {
            let tail: Vec<&String> = if import.path.len() >= 3 && import.path[1] == "core" {
                import.path[2..].iter().collect()
            } else {
                import.path.last().into_iter().collect()
            };
            let mut stdlib_rel_file = PathBuf::new();
            let mut stdlib_rel_dir = PathBuf::new();
            for (i, seg) in tail.iter().enumerate() {
                if i == tail.len() - 1 {
                    stdlib_rel_file.push(format!("{}.tnx", seg));
                    stdlib_rel_dir.push(seg);
                } else {
                    stdlib_rel_file.push(seg);
                    stdlib_rel_dir.push(seg);
                }
            }
            let dir = stdlib_dir().ok_or_else(|| {
                format!(
                    "Cannot resolve stdlib import '{}': TINOX_PATH not set and dev path not found",
                    rel_file.display()
                )
            })?;
            resolve_module_paths(&dir, &stdlib_rel_file, &stdlib_rel_dir)?.ok_or_else(|| {
                format!("Cannot resolve stdlib import '{}': no such file or directory", stdlib_rel_file.display())
            })?
        } else {
            return Err(format!("Cannot resolve import '{}': file not found", rel_file.display()));
        };

        for full_path in full_paths {
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
            check_one_type_per_file(&imported.decls, &full_path)?;
            check_no_top_level_fn(&imported.decls, &full_path)?;
            stamp_file_identity(&mut imported.decls, &full_path);

            let imported_dir = full_path.parent().unwrap_or(Path::new(".")).to_path_buf();
            resolve_imports(&mut imported, &imported_dir, visited, dep_dirs)?;

            imported_decls.extend(imported.decls);
        }
    }

    // Drop Import and Module decls — they are resolved or informational only
    ast.decls
        .retain(|d| !matches!(&d.node, DeclKind::Import(_) | DeclKind::Module(_)));

    imported_decls.append(&mut ast.decls);
    ast.decls = imported_decls;

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
    check_one_type_per_file(&ast.decls, Path::new(input_path))?;
    check_no_top_level_fn(&ast.decls, Path::new(input_path))?;
    stamp_file_identity(&mut ast.decls, Path::new(input_path));

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
    // NodeIds für die Typ-Tabelle (Typecheck → Codegen, TESTPLAN Phase 4)
    tinox_parser::assign_node_ids(&mut ast);

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
            oidc_roles: r.oidc_roles.clone(),
            is_static: r.is_static,
        })
        .collect();

    if ann_result.ws_endpoints.len() > 1 {
        return Err(format!(
            "found {} @WebsocketEndpoint classes ({}); v1 supports exactly one auto-run WebSocket endpoint per program",
            ann_result.ws_endpoints.len(),
            ann_result.ws_endpoints.iter().map(|e| e.class_name.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }
    let ws_endpoints: Vec<tinox_codegen::WsEndpointEntry> = ann_result
        .ws_endpoints
        .iter()
        .map(|e| tinox_codegen::WsEndpointEntry {
            class_name: e.class_name.clone(),
            path: e.path.clone(),
            port: e.port,
            on_open: e.on_open.clone(),
            on_message: e.on_message.clone(),
            on_close: e.on_close.clone(),
        })
        .collect();

    if ann_result.amqp10_consumers.len() > 1 {
        return Err(format!(
            "found {} @Amqp10Consumer classes ({}); v1 supports exactly one auto-run AMQP-1.0 consumer per program",
            ann_result.amqp10_consumers.len(),
            ann_result.amqp10_consumers.iter().map(|e| e.class_name.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }
    let amqp10_consumers: Vec<tinox_codegen::Amqp10ConsumerEntry> = ann_result
        .amqp10_consumers
        .iter()
        .map(|e| tinox_codegen::Amqp10ConsumerEntry {
            class_name: e.class_name.clone(),
            host: e.host.clone(),
            port: e.port,
            user: e.user.clone(),
            pass: e.pass.clone(),
            address: e.address.clone(),
            on_message: e.on_message.clone(),
        })
        .collect();

    if ann_result.amqp091_consumers.len() > 1 {
        return Err(format!(
            "found {} @Amqp091Consumer classes ({}); v1 supports exactly one auto-run AMQP-0-9-1 consumer per program",
            ann_result.amqp091_consumers.len(),
            ann_result.amqp091_consumers.iter().map(|e| e.class_name.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }
    let amqp091_consumers: Vec<tinox_codegen::Amqp091ConsumerEntry> = ann_result
        .amqp091_consumers
        .iter()
        .map(|e| tinox_codegen::Amqp091ConsumerEntry {
            class_name: e.class_name.clone(),
            host: e.host.clone(),
            port: e.port,
            vhost: e.vhost.clone(),
            user: e.user.clone(),
            pass: e.pass.clone(),
            queue: e.queue.clone(),
            on_message: e.on_message.clone(),
        })
        .collect();

    if ann_result.http3_rest_controllers.len() > 1 {
        return Err(format!(
            "found {} @Http3RestController classes ({}); v1 supports exactly one auto-run HTTP/3 REST controller per program",
            ann_result.http3_rest_controllers.len(),
            ann_result.http3_rest_controllers.iter().map(|e| e.class_name.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }
    if !ann_result.http3_rest_controllers.is_empty()
        && (!ann_result.ws_endpoints.is_empty()
            || !ann_result.amqp10_consumers.is_empty()
            || !ann_result.amqp091_consumers.is_empty())
    {
        return Err(
            "@Http3RestController cannot be combined with @WebsocketEndpoint/@Amqp10Consumer/@Amqp091Consumer in the same program (each generates its own auto-run `main`)".to_string(),
        );
    }
    let http3_rest_controller: Option<tinox_codegen::Http3RestControllerEntry> = ann_result
        .http3_rest_controllers
        .first()
        .map(|e| tinox_codegen::Http3RestControllerEntry {
            class_name: e.class_name.clone(),
            port: e.port,
            cert_path: e.cert_path.clone(),
            key_path: e.key_path.clone(),
        });

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
    codegen.set_expr_value_types(typechecker.expr_value_types());
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
    codegen.set_annotation_info(tinox_codegen::AnnotationInfo {
        inline_fns: ann_result.inline_functions,
        inline_meths: ann_result.inline_methods,
        routes: route_entries,
        di_components,
        log_classes: ann_result.log_classes,
        config_fields,
        cli_commands,
        sensitive_fields,
        masked_fields,
        do_not_serialize_fields,
        json_serializable_classes: ann_result.json_serializable_classes,
        metric_entries,
    });
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
    codegen.set_ws_endpoints(ws_endpoints);
    codegen.set_amqp10_consumers(amqp10_consumers);
    codegen.set_amqp091_consumers(amqp091_consumers);
    codegen.set_http3_rest_controller(http3_rest_controller);
    codegen.set_db_url(read_database_config().map(|c| c.url));
    codegen
        .gen(&ast)
        .map_err(|e| format!("Codegen error: {:?}", e))?;

    let ir = codegen.into_ir();
    let ir_path = format!("{}.ll", output_name);
    fs::write(&ir_path, ir).map_err(|e| format!("Failed to write IR: {}", e))?;

    compile_ll_to_exe(&ir_path, output_name, opt)
}

/// IR verifier gate: run the LLVM verifier on the generated .ll so invalid IR
/// fails immediately with a real diagnostic (instead of a bare "opt failed"/
/// "llc failed" later — or a silent miscompile in Debug mode, where opt is
/// skipped entirely). Invalid IR is always a codegen bug, never a user error.
fn verify_ir(ir_path: &str) -> Result<(), String> {
    let out = Command::new("opt")
        .args(["-passes=verify", "-disable-output", ir_path])
        .output();
    match out {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            let excerpt: Vec<&str> = stderr.lines().take(20).collect();
            Err(format!(
                "internal compiler error: generated invalid LLVM IR ({})\n{}\n\
                 This is a Tinox codegen bug — please report it with the source file.",
                ir_path,
                excerpt.join("\n")
            ))
        }
        // opt not installed — skip the gate, the normal pipeline will complain.
        Err(_) => Ok(()),
    }
}

fn compile_ll_to_exe(ir_path: &str, output_name: &str, opt: OptLevel) -> Result<(), String> {
    let obj_path = format!("{}.o", output_name);

    verify_ir(ir_path)?;

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
            .args([opt_flag, "-o", &bc_path, ir_path])
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
        .args([
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

    let runtime_src = runtime_c_path().ok_or_else(|| {
        "Cannot find runtime.c (checked the dev checkout path and /usr/share/tinox/runtime.c)".to_string()
    })?;
    let runtime_src = runtime_src.to_string_lossy().into_owned();
    let runtime_obj = format!("{}_runtime.o", output_name);

    let db_cfg = read_database_config();
    let db_driver = db_cfg.as_ref().map(|c| c.driver.as_str()).unwrap_or("");

    // Zusätzliche C-Flags aus der Umgebung, z. B. für Sanitizer-Läufe:
    // TINOX_CFLAGS="-fsanitize=address -g -DTINOX_NO_GC" (siehe make asan)
    let extra_cflags: Vec<String> = std::env::var("TINOX_CFLAGS")
        .map(|v| v.split_whitespace().map(String::from).collect())
        .unwrap_or_default();

    // HTTPS/TLS-Server: standardmäßig an. Aktiviert den TLS-Code in runtime.c
    // (-DTINOX_TLS) und linkt OpenSSL (-lssl -lcrypto). Opt-out per
    // TINOX_TLS=0, falls z.B. kein OpenSSL zum Bauen verfügbar ist.
    let tls_enabled = std::env::var("TINOX_TLS").map(|v| v != "0" && v != "false").unwrap_or(true);

    // HTTP/3 (QUIC) server: opt-in, default OFF -- unlike TLS (OpenSSL is
    // near-universally installed), ngtcp2/nghttp3 are far less common on a
    // typical build machine, so defaulting this on would break `tinox
    // build` with a compile error on any system lacking them, rather than
    // the graceful runtime -1 the rest of this file's opt-out flags give.
    // Also gated on tls_enabled: ngtcp2_crypto_ossl needs OpenSSL underneath,
    // so TINOX_TLS=0 implies HTTP/3 support is unavailable regardless of
    // TINOX_HTTP3.
    let http3_enabled = tls_enabled
        && std::env::var("TINOX_HTTP3")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

    let mut cc_args = vec!["-c", &runtime_src, "-o", &runtime_obj, "-O3"];
    if db_driver == "postgres" {
        cc_args.push("-DTINOX_DB_POSTGRES");
    } else if db_driver == "mysql" {
        cc_args.push("-DTINOX_DB_MYSQL");
    } else if db_driver == "sqlite" {
        cc_args.push("-DTINOX_DB_SQLITE");
    }
    if tls_enabled {
        cc_args.push("-DTINOX_TLS");
    }
    if http3_enabled {
        cc_args.push("-DTINOX_HTTP3");
    }
    cc_args.extend(extra_cflags.iter().map(|s| s.as_str()));
    let cc_status = Command::new("cc")
        .args(&cc_args)
        .status()
        .map_err(|e| format!("Failed to compile runtime: {}", e))?;

    if !cc_status.success() {
        return Err("Runtime compilation failed".to_string());
    }

    // -lz: WebSocket permessage-deflate (issue #122, RFC 7692) raw-deflate
    // wrappers in runtime.c. Unlike -lssl/-lcrypto (opt-out via TINOX_TLS,
    // since OpenSSL isn't always available in minimal build environments),
    // zlib is assumed always present — same tier as -lm/-lpthread/-lgc, no
    // opt-out needed.
    let mut link_args = vec![obj_path.as_str(), runtime_obj.as_str(), "-o", output_name, "-lm", "-lpthread", "-lgc", "-lz", "-no-pie"];
    if db_driver == "postgres" {
        link_args.push("-lpq");
    } else if db_driver == "mysql" {
        link_args.push("-lmysqlclient");
    } else if db_driver == "sqlite" {
        link_args.push("-lsqlite3");
    }
    if tls_enabled {
        link_args.push("-lssl");
        link_args.push("-lcrypto");
    }
    if http3_enabled {
        link_args.push("-lngtcp2");
        link_args.push("-lngtcp2_crypto_ossl");
        link_args.push("-lnghttp3");
    }
    link_args.extend(extra_cflags.iter().map(|s| s.as_str()));
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

#[cfg(test)]
mod one_type_per_file_tests {
    use super::*;

    fn parse_decls(src: &str) -> Vec<tinox_parser::Decl> {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().expect("tokenize");
        let mut parser = Parser::new(tokens);
        parser.parse().expect("parse").decls
    }

    #[test]
    fn zero_types_ok() {
        let decls = parse_decls("fn main() -> Int32 { return 0; }");
        assert!(check_one_type_per_file(&decls, Path::new("script.tnx")).is_ok());
    }

    #[test]
    fn one_type_matching_name_ok() {
        let decls = parse_decls("class Player { var hp: Int64; }");
        assert!(check_one_type_per_file(&decls, Path::new("Player.tnx")).is_ok());
    }

    #[test]
    fn one_type_mismatched_name_err() {
        let decls = parse_decls("class Player { var hp: Int64; }");
        let err = check_one_type_per_file(&decls, Path::new("player.tnx")).unwrap_err();
        assert!(err.contains("Player"), "error should name the type: {err}");
        assert!(err.contains("Player.tnx"), "error should name the required filename: {err}");
    }

    #[test]
    fn two_types_err() {
        let decls = parse_decls("class A { var x: Int64; } class B { var y: Int64; }");
        let err = check_one_type_per_file(&decls, Path::new("AB.tnx")).unwrap_err();
        assert!(err.contains('A') && err.contains('B'), "error should list both types: {err}");
    }

    #[test]
    fn namespace_wrapped_type_matching_name_ok() {
        let decls = parse_decls("namespace tinox.core.base64 { class Base64 { var x: Int64; } }");
        assert!(check_one_type_per_file(&decls, Path::new("Base64.tnx")).is_ok());
    }

    #[test]
    fn namespace_wrapped_type_mismatched_name_err() {
        let decls = parse_decls("namespace tinox.core.base64 { class Base64 { var x: Int64; } }");
        assert!(check_one_type_per_file(&decls, Path::new("base64.tnx")).is_err());
    }

    #[test]
    fn interface_and_enum_count_too() {
        let decls = parse_decls("interface Shape { fn area() -> Int64; } enum Color { Red, Blue }");
        let err = check_one_type_per_file(&decls, Path::new("x.tnx")).unwrap_err();
        assert!(err.contains("Shape") && err.contains("Color"), "error should list both: {err}");
    }
}

#[cfg(test)]
mod new_project_files_tests {
    use super::*;

    // #155/#159: the scaffold must produce a project that compiles and
    // tests cleanly under the one-class-per-file + mandatory
    // class-qualified-entry-point rules (#149) — not the pre-v2.0.0 bare
    // `fn main()` shape this used to generate.

    #[test]
    fn main_tnx_is_class_qualified_not_bare_fn() {
        let (_, main_tnx, _, _) = new_project_files("demo");
        assert!(main_tnx.contains("class Main"), "{main_tnx}");
        assert!(main_tnx.contains("fnc main() -> Int32"), "{main_tnx}");
        assert!(!main_tnx.trim_start().starts_with("fn main"), "{main_tnx}");
    }

    #[test]
    fn toml_declares_entry_matching_the_scaffolded_file_name() {
        let (toml, _, _, _) = new_project_files("demo");
        assert_eq!(read_project_entry(&toml), Some("src/Main.tnx".to_string()));
    }

    #[test]
    fn test_class_name_matches_its_own_scaffolded_file_name() {
        let (_, _, test_class, test_tnx) = new_project_files("demo");
        assert_eq!(test_class, "demoTests");
        assert!(test_tnx.contains(&format!("class {test_class}")), "{test_tnx}");
        // The file this content is written to (new_project) is named
        // "{test_class}.tnx" — the whole point being that the class name
        // inside the content and the file name it's written under match.
    }
}

#[cfg(test)]
mod read_project_entry_tests {
    use super::*;

    #[test]
    fn entry_field_found() {
        let toml = "[package]\nname = \"foo\"\nentry = \"src/Main.tnx\"\noutput = \"foo\"\n";
        assert_eq!(read_project_entry(toml), Some("src/Main.tnx".to_string()));
    }

    #[test]
    fn no_entry_field_returns_none() {
        let toml = "[package]\nname = \"foo\"\noutput = \"foo\"\n";
        assert_eq!(read_project_entry(toml), None);
    }

    #[test]
    fn entry_outside_package_section_ignored() {
        let toml = "[build]\nentry = \"not/this/one.tnx\"\n[package]\nname = \"foo\"\n";
        assert_eq!(read_project_entry(toml), None);
    }

    #[test]
    fn entry_field_whitespace_tolerant() {
        let toml = "[package]\nentry=\"src/Main.tnx\"\n";
        assert_eq!(read_project_entry(toml), Some("src/Main.tnx".to_string()));
    }
}

#[cfg(test)]
mod no_top_level_fn_tests {
    use super::*;

    fn parse_decls(src: &str) -> Vec<tinox_parser::Decl> {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().expect("tokenize");
        let mut parser = Parser::new(tokens);
        parser.parse().expect("parse").decls
    }

    #[test]
    fn class_only_ok() {
        let decls = parse_decls("class Main { fnc main() -> Int32 { return 0; } }");
        assert!(check_no_top_level_fn(&decls, Path::new("Main.tnx")).is_ok());
    }

    #[test]
    fn top_level_fn_err() {
        let decls = parse_decls("fn main() -> Int32 { return 0; }");
        let err = check_no_top_level_fn(&decls, Path::new("main.tnx")).unwrap_err();
        assert!(err.contains("main"), "error should name the function: {err}");
    }

    #[test]
    fn multiple_top_level_fns_err() {
        let decls = parse_decls("fn helper() -> Int64 { return 1; } fn main() -> Int32 { return 0; }");
        let err = check_no_top_level_fn(&decls, Path::new("x.tnx")).unwrap_err();
        assert!(err.contains("helper") && err.contains("main"), "error should list both: {err}");
    }

    #[test]
    fn extern_fn_stays_legal() {
        // `extern fn` (StmtKind::Empty body) is an FFI binding, not a free
        // function in the issue #149 sense -- must not trip the check.
        let decls = parse_decls("extern fn tinoxSomeRuntimeFn(x: Int64) -> Int64;");
        assert!(check_no_top_level_fn(&decls, Path::new("x.tnx")).is_ok());
    }

    #[test]
    fn namespace_wrapped_fn_err() {
        let decls = parse_decls("namespace tinox.core.demo { fn helper() -> Int64 { return 1; } }");
        let err = check_no_top_level_fn(&decls, Path::new("x.tnx")).unwrap_err();
        assert!(err.contains("helper"), "error should name the function: {err}");
    }
}
