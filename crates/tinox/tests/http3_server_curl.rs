//! tinox.core.http3_server.Http3Server (HTTP/3 over QUIC, RFC 9114).
//!
//! Compiles examples/http3_hello.tnx (an infinite listen() loop — never
//! returns, like the WebSocket example), runs it as a background process,
//! then drives it with the SYSTEM'S OWN curl over a real QUIC connection
//! (--http3-only) rather than a hand-rolled client, per this project's
//! "verify against a real, independent implementation" philosophy. Not
//! part of the golden-test harness (e2e.rs): that model requires the
//! process to exit within RUN_TIMEOUT, which a `listen()` server never does.
//!
//! Requires the runtime to be built with TINOX_HTTP3=1 (opt-in — ngtcp2/
//! nghttp3 aren't universally installed, unlike OpenSSL) and a curl build
//! linked against ngtcp2/nghttp3 (`curl --http3-only`). Gracefully SKIPs
//! (does not fail) if either precondition isn't met on the current
//! machine, mirroring e2e.rs's existing "SKIP (sqlite3 not installed)"
//! precedent for optional-dependency-gated tests — a missing optional
//! native dependency is an environment gap, not a regression.

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
fn http3_get_post_and_concurrent_requests() {
    if !ngtcp2_and_nghttp3_available() {
        eprintln!("SKIP http3_get_post_and_concurrent_requests (ngtcp2/nghttp3 dev libs not installed)");
        return;
    }
    if !curl_supports_http3() {
        eprintln!("SKIP http3_get_post_and_concurrent_requests (system curl lacks HTTP/3 support)");
        return;
    }

    let tinox = env!("CARGO_BIN_EXE_tinox");
    let root = repo_root();
    let src = root.join("examples/http3_hello.tnx");
    let workdir = std::env::temp_dir().join(format!(
        "tinox-http3-server-curl-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&workdir);
    std::fs::create_dir_all(&workdir).expect("mkdir workdir");
    let exe = workdir.join("http3_hello");

    std::fs::copy(
        root.join("tests/fixtures/tls/selfsigned_cert.pem"),
        workdir.join("tls_cert.pem"),
    )
    .expect("copy cert fixture");
    std::fs::copy(
        root.join("tests/fixtures/tls/selfsigned_key.pem"),
        workdir.join("tls_key.pem"),
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

    // Port from examples/http3_hello.tnx's Http3Server::new(8493, ...).
    let port = 8493;
    std::thread::sleep(Duration::from_millis(500));

    // GET /hello.
    let get = Command::new("curl")
        .args([
            "--http3-only",
            "-k",
            "-s",
            &format!("https://127.0.0.1:{port}/hello"),
        ])
        .output()
        .expect("spawn curl GET");
    assert!(get.status.success(), "curl GET failed: {get:?}");
    assert_eq!(
        String::from_utf8_lossy(&get.stdout),
        "hi from h3",
        "unexpected GET /hello body"
    );

    // POST /echo with a body, verifying request-body plumbing.
    let post = Command::new("curl")
        .args([
            "--http3-only",
            "-k",
            "-s",
            "--data-binary",
            "hello-quic-body",
            &format!("https://127.0.0.1:{port}/echo"),
        ])
        .output()
        .expect("spawn curl POST");
    assert!(post.status.success(), "curl POST failed: {post:?}");
    assert_eq!(
        String::from_utf8_lossy(&post.stdout),
        "echo:hello-quic-body",
        "unexpected POST /echo body"
    );

    // A several-hundred-KB body, exercising flow control across multiple
    // STREAM frames rather than a single small payload. Written to a file
    // and passed via --data-binary @file rather than as a single argv
    // entry -- large argv strings hit ArgumentListTooLong in some shells.
    let big_body = "x".repeat(300_000);
    let big_body_path = workdir.join("big_body.txt");
    std::fs::write(&big_body_path, &big_body).expect("write big body fixture");
    let big_post = Command::new("curl")
        .args([
            "--http3-only",
            "-k",
            "-s",
            "--data-binary",
            &format!("@{}", big_body_path.display()),
            &format!("https://127.0.0.1:{port}/echo"),
        ])
        .output()
        .expect("spawn curl big POST");
    assert!(big_post.status.success(), "curl big POST failed: {big_post:?}");
    assert_eq!(
        String::from_utf8_lossy(&big_post.stdout),
        format!("echo:{big_body}"),
        "unexpected large-body POST /echo response"
    );

    // 404 for an unmatched route.
    let notfound = Command::new("curl")
        .args([
            "--http3-only",
            "-k",
            "-s",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            &format!("https://127.0.0.1:{port}/does-not-exist"),
        ])
        .output()
        .expect("spawn curl 404 check");
    assert!(notfound.status.success(), "curl 404 check failed: {notfound:?}");
    assert_eq!(
        String::from_utf8_lossy(&notfound.stdout),
        "404",
        "unmatched route should respond 404"
    );

    // Several concurrent requests over HTTP/3 (multi-stream multiplexing).
    let concurrent = Command::new("curl")
        .args([
            "--http3-only",
            "-k",
            "-s",
            "-Z",
            &format!("https://127.0.0.1:{port}/hello"),
            &format!("https://127.0.0.1:{port}/hello"),
            &format!("https://127.0.0.1:{port}/hello"),
        ])
        .output()
        .expect("spawn curl concurrent");
    assert!(concurrent.status.success(), "curl concurrent requests failed: {concurrent:?}");
    assert_eq!(
        String::from_utf8_lossy(&concurrent.stdout),
        "hi from h3hi from h3hi from h3",
        "unexpected concurrent-request output"
    );

    let _ = std::fs::remove_dir_all(&workdir);
}
