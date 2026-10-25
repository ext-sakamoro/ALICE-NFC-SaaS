//! The gateway binary: without a usable JWT secret (unset, under 32 bytes after trimming, or a
//! placeholder such as an .env.example value) it refuses to start; with one it starts,
//! answers /health, routes /api/v1/<path> through the auth layer (401 without a token), and never
//! prints the secret. Its route literals use the path syntax of the axum it is built with (0.7:
//! `:name` and `*name`), which is what lets the router be built at all.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// a secret made inside the test (no secret is kept in a fixture)
fn test_secret() -> String {
    (0u8..40)
        .map(|i| char::from(b'a' + (i.wrapping_mul(7) % 26)))
        .collect()
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("a free port")
        .local_addr()
        .expect("its address")
        .port()
}

fn spawn(env: &[(&str, &str)], port: u16) -> Child {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_api-gateway"));
    cmd.env_clear()
        .env("GATEWAY_ADDR", format!("127.0.0.1:{port}"))
        .env("PORT", port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.spawn().expect("the gateway runs")
}

/// Wait up to `wait` for the gateway to exit; `None` if it is still running.
fn exit_within(child: &mut Child, wait: Duration) -> Option<i32> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("wait") {
            return status.code();
        }
        if start.elapsed() > wait {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn output(mut child: Child) -> String {
    let _ = child.kill();
    let out = child.wait_with_output().expect("output");
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

/// The status code of `GET path` (HTTP/1.1, no body), retried until the gateway listens.
fn status(port: u16, path: &str, child: &mut Child) -> u16 {
    let start = Instant::now();
    loop {
        if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
            s.set_read_timeout(Some(Duration::from_secs(5))).expect("timeout");
            write!(
                s,
                "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
            )
            .expect("request");
            let mut buf = Vec::new();
            let _ = s.read_to_end(&mut buf);
            let head = String::from_utf8_lossy(&buf);
            let code = head
                .split_whitespace()
                .nth(1)
                .and_then(|c| c.parse().ok())
                .unwrap_or_else(|| panic!("no status line for {path}: {head:?}"));
            return code;
        }
        assert!(
            exit_within(child, Duration::from_millis(0)).is_none(),
            "the gateway exited before listening"
        );
        assert!(start.elapsed() < Duration::from_secs(20), "the gateway never listened");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// the values a deployment can end up with by forgetting the secret or copying an example
const UNUSABLE: [&str; 9] = [
    "",
    "      ",
    "short-but-random-q8Zr",
    "dev-secret-change-me",
    "your-jwt-secret-min-32-chars",
    "a-long-value-that-says-change-me-somewhere",
    "YOUR-JWT-SECRET-AT-LEAST-THIRTY-TWO-BYTES",
    "an-example-secret-that-is-long-enough-1234",
    "placeholder-placeholder-placeholder-xyz",
];

#[test]
fn without_a_usable_jwt_secret_the_gateway_does_not_start() {
    let mut cases: Vec<Vec<(&str, &str)>> = vec![vec![]];
    cases.extend(UNUSABLE.iter().map(|v| vec![("JWT_SECRET", *v)]));
    for env in cases {
        let mut child = spawn(&env, free_port());
        let code = exit_within(&mut child, Duration::from_secs(10));
        let out = output(child);
        assert!(code.is_some(), "{env:?}: the gateway started: {out}");
        assert_ne!(code, Some(0), "{env:?}: {out}");
        assert!(out.contains("JWT_SECRET"), "{env:?}: {out}");
        if let Some((_, v)) = env.first() {
            if v.trim().len() > 3 {
                assert!(!out.contains(v.trim()), "{env:?}: the value was printed: {out}");
            }
        }
    }
}

#[test]
fn with_a_jwt_secret_the_gateway_starts_and_routes_requests() {
    let secret = test_secret();
    let port = free_port();
    let mut child = spawn(&[("JWT_SECRET", &secret)], port);
    let health = status(port, "/health", &mut child);
    let routed = status(port, "/api/v1/some/routed/path", &mut child);
    let running = exit_within(&mut child, Duration::from_millis(0)).is_none();
    let out = output(child);
    assert!(running, "the gateway exited: {out}");
    assert_eq!(health, 200, "{out}");
    assert_eq!(routed, 401, "the catch-all is not routed through auth: {out}");
    assert!(!out.contains(&secret), "{out}");
}

/// every `.route` path literal uses axum 0.7's syntax (`:name`, `*name`), none 0.8's `{name}`
#[test]
fn the_route_literals_use_the_axum_0_7_path_syntax() {
    let src = include_str!("../src/main.rs");
    let mut routes = 0;
    for part in src.split(".route(").skip(1) {
        let lit = part
            .trim_start()
            .strip_prefix('"')
            .and_then(|r| r.split('"').next())
            .expect("a string literal route");
        routes += 1;
        assert!(!lit.contains('{') && !lit.contains('}'), "{lit}");
        for seg in lit.split('/').filter(|s| !s.is_empty()) {
            if let Some(name) = seg.strip_prefix(':').or_else(|| seg.strip_prefix('*')) {
                assert!(
                    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                    "{lit}"
                );
            }
        }
    }
    assert!(routes > 0, "no .route( literal was read");
}
