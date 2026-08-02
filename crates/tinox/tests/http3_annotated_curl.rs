//! @Http3RestController: declarative HTTP/3 REST endpoints
//! (examples/http3_rest_api/src/TaskController.tnx), compiled entirely by
//! the compiler's auto-generated `main` (tinox_typecheck's
//! Http3RestControllerInfo + tinox_codegen's emit_http3_route_code) rather
//! than the hand-written Http3Server::new()/.get()/... calls in
//! examples/http3_rest_api/src/Main.tnx.
//!
//! Same shape as http3_server_curl.rs (spawn the compiled server as a
//! background process, drive it with the system's real `curl
//! --http3-only`, gracefully SKIP if ngtcp2/nghttp3/HTTP3-curl aren't
//! available) -- this test exists to confirm the annotation-driven path
//! produces byte-identical behavior to the fluent-API version, not to
//! re-verify HTTP/3 itself (that's http3_server_curl.rs's job).

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

struct KillOnDrop(Child);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn ngtcp2_and_nghttp3_available() -> bool {
    Command::new("pkg-config")
        .args(["--exists", "libngtcp2", "libngtcp2_crypto_ossl", "libnghttp3"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn curl_supports_http3() -> bool {
    Command::new("curl")
        .arg("-V")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("HTTP3"))
        .unwrap_or(false)
}

#[test]
fn http3_rest_controller_crud_flow() {
    if !ngtcp2_and_nghttp3_available() {
        eprintln!("SKIP http3_rest_controller_crud_flow (ngtcp2/nghttp3 dev libs not installed)");
        return;
    }
    if !curl_supports_http3() {
        eprintln!("SKIP http3_rest_controller_crud_flow (system curl lacks HTTP/3 support)");
        return;
    }

    let tinox = env!("CARGO_BIN_EXE_tinox");
    let root = repo_root();
    let src = root.join("examples/http3_rest_api/src/TaskController.tnx");
    let workdir = std::env::temp_dir().join(format!(
        "tinox-http3-annotated-curl-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&workdir);
    std::fs::create_dir_all(&workdir).expect("mkdir workdir");
    let exe = workdir.join("task_controller");

    // TaskController.tnx's @Http3RestController(8843, "cert.pem", "key.pem").
    std::fs::copy(
        root.join("tests/fixtures/tls/selfsigned_cert.pem"),
        workdir.join("cert.pem"),
    )
    .expect("copy cert fixture");
    std::fs::copy(
        root.join("tests/fixtures/tls/selfsigned_key.pem"),
        workdir.join("key.pem"),
    )
    .expect("copy key fixture");

    let build = Command::new(tinox)
        .arg("build")
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .current_dir(&workdir)
        .env("TINOX_HTTP3", "1")
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

    let port = 8843;
    std::thread::sleep(Duration::from_millis(500));

    let curl_json = |args: &[&str]| -> String {
        let out = Command::new("curl")
            .args(args)
            .output()
            .expect("spawn curl");
        assert!(out.status.success(), "curl failed: {out:?}");
        String::from_utf8_lossy(&out.stdout).to_string()
    };

    // GET /tasks -- the two seeded tasks (lazily seeded by ensureInit() on
    // first request; @ApplicationComponent keeps this one TaskController
    // instance, and its `tasks` field, alive across every request below).
    let list = curl_json(&["--http3-only", "-k", "-s", &format!("https://127.0.0.1:{port}/tasks")]);
    assert_eq!(
        list,
        r#"[{"id":1,"title":"Try tinox.core.http3_server","done":false},{"id":2,"title":"curl --http3-only this API","done":false}]"#
    );

    // POST /tasks -- @StatusCode(201) + id auto-assigned via nextTaskId().
    let created = curl_json(&[
        "--http3-only", "-k", "-s",
        "-X", "POST",
        "-H", "Content-Type: application/json",
        "-d", r#"{"id":0,"title":"Declarative HTTP/3 works","done":false}"#,
        &format!("https://127.0.0.1:{port}/tasks"),
    ]);
    assert_eq!(created, r#"{"id":3,"title":"Declarative HTTP/3 works","done":false}"#);

    // PATCH /tasks/3/done -- toggles the just-created task.
    let toggled = curl_json(&[
        "--http3-only", "-k", "-s", "-X", "PATCH",
        &format!("https://127.0.0.1:{port}/tasks/3/done"),
    ]);
    assert_eq!(toggled, r#"{"id":3,"title":"Declarative HTTP/3 works","done":true}"#);

    // PUT /tasks/1 -- @Auth-free @PUT route, updates in place.
    let updated = curl_json(&[
        "--http3-only", "-k", "-s",
        "-X", "PUT",
        "-H", "Content-Type: application/json",
        "-d", r#"{"id":0,"title":"Renamed via annotation","done":true}"#,
        &format!("https://127.0.0.1:{port}/tasks/1"),
    ]);
    assert_eq!(updated, r#"{"id":1,"title":"Renamed via annotation","done":true}"#);

    // DELETE /tasks/2 -- @StatusCode(204), in-place removal.
    let delete_status = Command::new("curl")
        .args([
            "--http3-only", "-k", "-s", "-o", "/dev/null", "-w", "%{http_code}",
            "-X", "DELETE",
            &format!("https://127.0.0.1:{port}/tasks/2"),
        ])
        .output()
        .expect("spawn curl DELETE");
    assert!(delete_status.status.success());
    assert_eq!(String::from_utf8_lossy(&delete_status.stdout), "204");

    // GET /tasks (final) -- task 2 gone, tasks 1 and 3 reflect the updates
    // above -- confirms shared, persistent state across all these requests.
    let final_list = curl_json(&["--http3-only", "-k", "-s", &format!("https://127.0.0.1:{port}/tasks")]);
    assert_eq!(
        final_list,
        r#"[{"id":1,"title":"Renamed via annotation","done":true},{"id":3,"title":"Declarative HTTP/3 works","done":true}]"#
    );

    // GET /tasks/999 -- 404 for a missing id.
    let notfound = Command::new("curl")
        .args([
            "--http3-only", "-k", "-s", "-w", " [%{http_code}]",
            &format!("https://127.0.0.1:{port}/tasks/999"),
        ])
        .output()
        .expect("spawn curl 404 check");
    assert!(notfound.status.success());
    assert_eq!(
        String::from_utf8_lossy(&notfound.stdout),
        r#"{"error":"task not found"} [404]"#
    );

    let _ = std::fs::remove_dir_all(&workdir);
}
