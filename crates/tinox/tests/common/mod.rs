//! Shared helpers for the golden-test harnesses (e2e.rs, matrix.rs).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const COMPILE_TIMEOUT: Duration = Duration::from_secs(60);
const RUN_TIMEOUT: Duration = Duration::from_secs(15);

pub struct Case {
    pub path: PathBuf,
    pub name: String,
    pub expect_lines: Vec<String>,
    pub expect_contains: Vec<String>,
    pub expect_exit: i32,
    pub args: Vec<String>,
    pub db_sql: Vec<String>,
    pub test_mode: bool,
}

#[allow(dead_code)] // nur von e2e.rs genutzt — andere Test-Binaries teilen sich dieses Modul
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

pub fn parse_case(path: &Path) -> Case {
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
pub fn wait_timeout(child: &mut std::process::Child, dur: Duration) -> Option<std::process::ExitStatus> {
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

pub fn run_captured(
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

pub fn run_case(case: &Case) -> Result<(), String> {
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

