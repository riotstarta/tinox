//! Regression coverage for #155/#159: `tinox new` used to scaffold a
//! project that failed to compile (`src/main.tnx`'s bare top-level `fn
//! main()`, a hard error since v2.0.0's mandatory class-qualified calls)
//! and failed `tinox test` (`tests/main_test.tnx`'s `class {name}Tests`
//! violating the one-class-per-file naming rule) — i.e. the very first
//! commands in the README quick start didn't work on a fresh checkout.
//!
//! Exercises the actual built `tinox` binary end-to-end, not just the
//! template-generation unit tests in `main.rs`.

use std::process::Command;

fn tinox() -> &'static str {
    env!("CARGO_BIN_EXE_tinox")
}

#[test]
fn new_project_builds_runs_and_tests_cleanly() {
    let workdir = std::env::temp_dir().join(format!(
        "tinox-new-project-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&workdir);
    std::fs::create_dir_all(&workdir).expect("mkdir workdir");

    let new_status = Command::new(tinox())
        .arg("new")
        .arg("demo")
        .current_dir(&workdir)
        .status()
        .expect("run tinox new");
    assert!(new_status.success(), "tinox new failed");

    let project_dir = workdir.join("demo");
    assert!(project_dir.join("src/Main.tnx").is_file(), "src/Main.tnx missing");
    assert!(project_dir.join("tests/demoTests.tnx").is_file(), "tests/demoTests.tnx missing");
    assert!(!project_dir.join("tinox.yaml").exists(), "tinox.yaml should no longer be scaffolded (#154)");
    assert!(!project_dir.join("src/main.tnx").exists(), "old lowercase src/main.tnx should not exist");

    let run_output = Command::new(tinox())
        .arg("run")
        .current_dir(&project_dir)
        .output()
        .expect("run tinox run");
    assert!(
        run_output.status.success(),
        "tinox run failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&run_output.stdout),
        String::from_utf8_lossy(&run_output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&run_output.stdout).contains("Hello from demo!"),
        "unexpected tinox run output: {}",
        String::from_utf8_lossy(&run_output.stdout)
    );

    let test_output = Command::new(tinox())
        .arg("test")
        .current_dir(&project_dir)
        .output()
        .expect("run tinox test");
    assert!(
        test_output.status.success(),
        "tinox test failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&test_output.stdout),
        String::from_utf8_lossy(&test_output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&test_output.stdout).contains("1 passed"),
        "unexpected tinox test output: {}",
        String::from_utf8_lossy(&test_output.stdout)
    );

    std::fs::remove_dir_all(&workdir).ok();
}
