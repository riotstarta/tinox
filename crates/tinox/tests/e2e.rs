//! Golden-test harness (TESTPLAN Phase 1.1).
//!
//! Iterates `tests/e2e/*.tnx` at the repo root. Each file declares its
//! expectations in leading comment directives:
//!
//! ```text
//! // expect: 42            — one expected stdout line (ordered, repeatable)
//! // expect-exit: 1        — expected exit code (default 0)
//! // args: a b c           — argv passed to the compiled program
//! // db: CREATE TABLE ...  — sqlite fixture SQL (repeatable); provides
//! //                         tinox.toml + test.db in the working dir
//! // mode: test             — run `tinox test` on the file instead of build+run
//! // expect-contains: X     — substring the output must contain (repeatable)
//! ```
//!
//! Files starting with `_` are helper modules and are not run directly.
//! Each case compiles with the freshly built `tinox` binary in an isolated
//! working directory, runs with a timeout, and compares stdout+stderr.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const COMPILE_TIMEOUT: Duration = Duration::from_secs(60);
const RUN_TIMEOUT: Duration = Duration::from_secs(15);

struct Case {
    path: PathBuf,
    name: String,
    expect_lines: Vec<String>,
    expect_contains: Vec<String>,
    expect_exit: i32,
    args: Vec<String>,
    db_sql: Vec<String>,
    test_mode: bool,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn parse_case(path: &Path) -> Case {
    let src = fs::read_to_string(path).expect("read test file");
    let name = path.file_stem().unwrap().to_string_lossy().to_string();
    let mut expect_lines = Vec::new();
    let mut expect_contains = Vec::new();
    let mut expect_exit = 0;
    let mut args = Vec::new();
    let mut db_sql = Vec::new();
    let mut test_mode = false;
    for line in src.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("// expect-exit:") {
            expect_exit = rest.trim().parse().expect("expect-exit code");
        } else if let Some(rest) = t.strip_prefix("// expect-contains:") {
            expect_contains.push(rest.trim().to_string());
        } else if let Some(rest) = t.strip_prefix("// expect:") {
            expect_lines.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
        } else if let Some(rest) = t.strip_prefix("// args:") {
            args = rest.split_whitespace().map(String::from).collect();
        } else if let Some(rest) = t.strip_prefix("// db:") {
            db_sql.push(rest.trim().to_string());
        } else if t == "// mode: test" {
            test_mode = true;
        }
    }
    Case { path: path.to_path_buf(), name, expect_lines, expect_contains, expect_exit, args, db_sql, test_mode }
}

/// Wait with timeout; kill on expiry. Returns None on timeout.
fn wait_timeout(child: &mut std::process::Child, dur: Duration) -> Option<std::process::ExitStatus> {
    let start = Instant::now();
    loop {
        if let Some(st) = child.try_wait().expect("try_wait") {
            return Some(st);
        }
        if start.elapsed() > dur {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn run_captured(
    mut cmd: Command,
    dur: Duration,
) -> Result<(Option<std::process::ExitStatus>, String), String> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn {:?} failed: {e}", cmd.get_program()))?;
    // Read pipes on threads so a chatty child can't fill the pipe and stall.
    let mut out_pipe = child.stdout.take().unwrap();
    let mut err_pipe = child.stderr.take().unwrap();
    let out_h = std::thread::spawn(move || {
        use std::io::Read;
        let mut s = String::new();
        let _ = out_pipe.read_to_string(&mut s);
        s
    });
    let err_h = std::thread::spawn(move || {
        use std::io::Read;
        let mut s = String::new();
        let _ = err_pipe.read_to_string(&mut s);
        s
    });
    let status = wait_timeout(&mut child, dur);
    let mut output = out_h.join().unwrap_or_default();
    output.push_str(&err_h.join().unwrap_or_default());
    Ok((status, output))
}

fn run_case(case: &Case) -> Result<(), String> {
    let tinox = env!("CARGO_BIN_EXE_tinox");
    let workdir = std::env::temp_dir().join(format!(
        "tinox-e2e-{}-{}",
        case.name,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&workdir);
    fs::create_dir_all(&workdir).map_err(|e| format!("mkdir workdir: {e}"))?;

    // Optional sqlite fixture
    if !case.db_sql.is_empty() {
        let sql = case.db_sql.join("\n");
        let st = Command::new("sqlite3")
            .arg(workdir.join("test.db"))
            .arg(&sql)
            .status()
            .map_err(|e| format!("sqlite3 fixture: {e} (sqlite3 installed?)"))?;
        if !st.success() {
            return Err("sqlite3 fixture SQL failed".to_string());
        }
        fs::write(
            workdir.join("tinox.toml"),
            "[database]\ndriver = \"sqlite\"\nurl = \"test.db\"\n",
        )
        .map_err(|e| format!("write tinox.toml: {e}"))?;
    }

    let (status, output) = if case.test_mode {
        // `tinox test` compiles and runs @Test methods itself.
        let mut cmd = Command::new(tinox);
        cmd.arg("test").arg(&case.path).current_dir(&workdir);
        let (status, output) = run_captured(cmd, COMPILE_TIMEOUT)?;
        match status {
            None => return Err("tinox test TIMEOUT".to_string()),
            Some(st) => (st, output),
        }
    } else {
        // Compile
        let mut build = Command::new(tinox);
        build.arg("build").arg(&case.path).current_dir(&workdir);
        let (status, out) = run_captured(build, COMPILE_TIMEOUT)?;
        match status {
            None => return Err("compile TIMEOUT".to_string()),
            Some(st) if !st.success() => {
                return Err(format!("compile failed:\n{}", out.trim_end()));
            }
            _ => {}
        }

        // Run
        let exe = workdir.join(&case.name);
        let mut run = Command::new(&exe);
        run.args(&case.args).current_dir(&workdir);
        let (status, output) = run_captured(run, RUN_TIMEOUT)?;
        match status {
            None => return Err("run TIMEOUT".to_string()),
            Some(st) => (st, output),
        }
    };

    let _ = fs::remove_dir_all(&workdir);

    let exit = status.code().unwrap_or(-1);
    let actual = output.trim_end_matches('\n');
    let expected = case.expect_lines.join("\n");
    let mut errors = Vec::new();
    if exit != case.expect_exit {
        errors.push(format!("exit code: expected {}, got {}", case.expect_exit, exit));
    }
    if !case.expect_lines.is_empty() && actual != expected {
        errors.push(format!("output mismatch:\n--- expected ---\n{expected}\n--- actual ---\n{actual}\n---"));
    }
    for needle in &case.expect_contains {
        if !actual.contains(needle.as_str()) {
            errors.push(format!("output does not contain {needle:?}:\n{actual}"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

fn collect_cases() -> Vec<Case> {
    let dir = repo_root().join("tests/e2e");
    let mut cases: Vec<Case> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("tests/e2e missing at {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "tnx").unwrap_or(false))
        .filter(|p| {
            !p.file_name()
                .map(|n| n.to_string_lossy().starts_with('_'))
                .unwrap_or(false)
        })
        .map(|p| parse_case(&p))
        .collect();
    cases.sort_by(|a, b| a.name.cmp(&b.name));
    assert!(!cases.is_empty(), "no e2e cases found in {}", dir.display());
    for c in &cases {
        assert!(
            !c.expect_lines.is_empty() || !c.expect_contains.is_empty() || c.expect_exit != 0,
            "{}: no // expect: directives — dead test",
            c.name
        );
    }
    cases
}

fn run_shard(shard: usize, num_shards: usize) {
    let cases = collect_cases();
    let mut failures = Vec::new();
    let mut ran = 0;
    for (i, case) in cases.iter().enumerate() {
        if i % num_shards != shard {
            continue;
        }
        // DB cases need the sqlite3 CLI — skip gracefully where absent.
        if !case.db_sql.is_empty()
            && Command::new("sqlite3").arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| !s.success()).unwrap_or(true)
        {
            eprintln!("SKIP {} (sqlite3 not installed)", case.name);
            continue;
        }
        ran += 1;
        if let Err(msg) = run_case(case) {
            failures.push(format!("== {} ==\n{}", case.name, msg));
        }
    }
    assert!(
        failures.is_empty(),
        "{}/{} e2e cases failed:\n\n{}",
        failures.len(),
        ran,
        failures.join("\n\n")
    );
}

// Four shards so `cargo test` runs cases in parallel.
#[test]
fn e2e_shard_0() { run_shard(0, 4); }
#[test]
fn e2e_shard_1() { run_shard(1, 4); }
#[test]
fn e2e_shard_2() { run_shard(2, 4); }
#[test]
fn e2e_shard_3() { run_shard(3, 4); }
