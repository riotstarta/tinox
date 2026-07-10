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

mod common;
use common::{parse_case, repo_root, Case};
use std::fs;
use std::process::{Command, Stdio};

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
        if let Err(msg) = common::run_case(case) {
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
