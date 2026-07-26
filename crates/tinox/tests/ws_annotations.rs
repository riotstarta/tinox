//! @WebsocketEndpoint/@OnOpen/@OnMessage/@OnClose annotations.
//!
//! Compiles examples/EchoEndpoint.tnx (a generated auto-main accept/
//! message loop — never returns), runs it as a background process, drives
//! a real handshake + text-frame roundtrip over a raw TCP socket, then kills
//! it. Not part of the golden-test harness (e2e.rs): those cases must exit
//! on their own within RUN_TIMEOUT, which an auto-run WS server never does.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

/// Kills the child on drop, so a failed assertion doesn't leak a listening
/// process across test runs.
struct KillOnDrop(Child);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn read_n(stream: &mut TcpStream, n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    stream.read_exact(&mut buf).expect("read_exact");
    buf
}

#[test]
fn ws_annotated_endpoint_handshake_and_echo() {
    let tinox = env!("CARGO_BIN_EXE_tinox");
    let root = repo_root();
    let src = root.join("examples/EchoEndpoint.tnx");
    let workdir = std::env::temp_dir().join(format!(
        "tinox-ws-annotations-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&workdir);
    std::fs::create_dir_all(&workdir).expect("mkdir workdir");
    let exe = workdir.join("EchoEndpoint");

    let build = Command::new(tinox)
        .arg("build")
        .arg(&src)
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

    // Port from @WebsocketEndpoint("/echo", 8793) in the example; give the
    // server a moment to bind before connecting.
    let port = 8793;
    let mut stream = None;
    for _ in 0..50 {
        if let Ok(s) = TcpStream::connect(("127.0.0.1", port)) {
            stream = Some(s);
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let mut stream = stream.expect("connect to annotated WS server");
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();

    // RFC 6455 §1.3 example key/accept pair (same vector as ws_handshake_frames.tnx).
    let req = "GET /echo HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n";
    stream.write_all(req.as_bytes()).expect("send handshake");

    let mut resp = Vec::new();
    let mut byte = [0u8; 1];
    // Read until "\r\n\r\n".
    while !resp.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).expect("read handshake response");
        resp.push(byte[0]);
    }
    let resp_text = String::from_utf8_lossy(&resp);
    assert!(resp_text.contains("101"), "expected 101 response, got: {resp_text}");
    assert!(
        resp_text.contains("s3pPLMBiTxaQ9kYGzzhZRbK+xOo="),
        "expected RFC-6455 accept value, got: {resp_text}"
    );

    // Masked text frame "hallo" — maps to @OnMessage, echoed as "echo: hallo".
    let mask = [1u8, 2, 3, 4];
    let payload = b"hallo";
    let mut frame = vec![0x81u8, 0x80 | payload.len() as u8];
    frame.extend_from_slice(&mask);
    for (i, b) in payload.iter().enumerate() {
        frame.push(b ^ mask[i % 4]);
    }
    stream.write_all(&frame).expect("send text frame");

    let hdr = read_n(&mut stream, 2);
    assert_eq!(hdr[0], 0x81, "expected FIN+text opcode byte");
    let plen = (hdr[1] & 0x7f) as usize;
    let body = read_n(&mut stream, plen);
    assert_eq!(
        String::from_utf8_lossy(&body),
        "echo: hallo",
        "unexpected echoed payload"
    );

    let _ = std::fs::remove_dir_all(&workdir);
}
