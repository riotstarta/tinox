//! Regression test for issue #140: `tinox_HttpServer_listen`'s epoll worker
//! threads used to crash inside GC-managed memory after a handful of
//! requests to an allocation-heavy `@GET` route (root cause: several
//! `static __thread` buffers hold pointers to GC-managed memory, but Boehm
//! GC does not automatically scan `__thread`/TLS storage as roots -- fixed
//! by explicitly `GC_add_roots`-registering them once per thread, see
//! `tinox_gc_register_thread_roots` in runtime.c).
//!
//! Drives a real compiled server with the SYSTEM'S OWN curl over many
//! sequential real HTTP requests (not a simulated in-process client), per
//! this project's "verify against a real, independent implementation"
//! philosophy -- this exact bug was previously invisible to any
//! self-consistent/simulated test and was only found via live curl load.

use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct KillOnDrop(Child);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn http_server_survives_heavy_allocation_load() {
    let tinox = env!("CARGO_BIN_EXE_tinox");
    let workdir = std::env::temp_dir().join(format!(
        "tinox-http-gc-stress-curl-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&workdir);
    std::fs::create_dir_all(&workdir).expect("mkdir workdir");

    // A route that allocates heavily (many small String concatenations,
    // each a fresh GC allocation) before ever touching ctx.response --
    // reproduces the crash without needing the Map-based header-setting
    // path, isolating the GC-root issue itself.
    std::fs::write(
        workdir.join("Ctrl.tnx"),
        r#"import tinox.core.http_server;

class Ctrl
{
    @GET("/heavy")
    fnc heavy(ctx: HttpContext) -> Nothing
    {
        var s: String = "";
        var i: Int64 = 0;
        while i < 3000
        {
            s = s + fromCharCode(65 + (i % 26));
            i = i + 1;
        }
        ctx.response.status(200).json("{\"len\":" + s.len().toString() + "}");
    }
}
"#,
    )
    .expect("write Ctrl.tnx");

    std::fs::write(
        workdir.join("Main.tnx"),
        r#"import Ctrl;

class Main
{
    fnc main() -> Int32
    {
        return 0;
    }
}
"#,
    )
    .expect("write Main.tnx");

    // Core/extended stdlib split: Ctrl.tnx imports tinox.core.http_server
    // (extended-tier), so it needs a declared+installed dependency now.
    std::fs::write(
        workdir.join("tinox.toml"),
        "[package]\nname = \"heavy_server\"\nversion = \"0.0.0\"\ndescription = \"\"\n\n[[dependencies]]\ngroup = \"tinox.core\"\nartifactId = \"http_server\"\nversion = \"1.0.0\"\n",
    )
    .expect("write tinox.toml");
    let install = Command::new(tinox)
        .arg("install")
        .current_dir(&workdir)
        .output()
        .expect("spawn install");
    assert!(
        install.status.success(),
        "tinox install failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&install.stdout),
        String::from_utf8_lossy(&install.stderr)
    );

    let exe = workdir.join("heavy_server");
    let build = Command::new(tinox)
        .arg("build")
        .arg(workdir.join("Main.tnx"))
        .arg("-o")
        .arg(&exe)
        .current_dir(&workdir)
        .output()
        .expect("spawn build");
    assert!(
        build.status.success(),
        "build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );

    let child = Command::new(&exe)
        .current_dir(&workdir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn server");
    let _guard = KillOnDrop(child);
    std::thread::sleep(Duration::from_millis(500));

    // tinox_HttpServer_listen always binds port 8080 (no port-configuration
    // knob on this path -- matches the annotation-driven server's actual
    // behavior, not a test-specific choice).
    let port = 8080;

    let mut success = 0u32;
    for _ in 0..300 {
        let out = Command::new("curl")
            .args([
                "-s",
                "-o",
                "/dev/null",
                "-w",
                "%{http_code}",
                &format!("http://127.0.0.1:{port}/heavy"),
            ])
            .output()
            .expect("spawn curl");
        if String::from_utf8_lossy(&out.stdout) == "200" {
            success += 1;
        }
    }

    // A handful of transient connection hiccups from a tight sequential
    // curl loop are tolerated; a crash (the actual bug) collapses this to
    // near-zero successes for the remainder of the run, not a handful of
    // isolated misses.
    assert!(
        success >= 290,
        "expected almost all 300 requests to succeed, got {success} -- server likely crashed (issue #140)"
    );

    // The process must still be alive and responsive after the load, not
    // just "some requests happened to land before it died".
    let final_check = Command::new("curl")
        .args(["-s", &format!("http://127.0.0.1:{port}/heavy")])
        .output()
        .expect("spawn final curl");
    assert!(
        String::from_utf8_lossy(&final_check.stdout).contains("\"len\":3000"),
        "server not responsive after load: {final_check:?}"
    );

    let _ = std::fs::remove_dir_all(&workdir);
}
