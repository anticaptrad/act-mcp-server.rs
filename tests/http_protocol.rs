use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn reserve_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("reserve loopback port")
        .local_addr()
        .expect("local address")
        .port()
}

fn request(port: u16, host: &str, secret: Option<&str>, body: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to server");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("read timeout");
    let secret_header = secret
        .map(|value| format!("x-server-auth: {value}\r\n"))
        .unwrap_or_default();
    let payload = format!(
        "POST /mcp HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nAccept: application/json\r\nConnection: close\r\n{secret_header}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(payload.as_bytes()).expect("write request");
    stream.flush().expect("flush request");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    response
}

fn wait_until_ready(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) {
            let request = format!(
                "GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
            );
            let _ = stream.write_all(request.as_bytes());
            let mut response = String::new();
            let _ = stream.read_to_string(&mut response);
            if response.starts_with("HTTP/1.1 200") {
                return;
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("server did not become ready");
}

#[test]
fn real_http_process_enforces_host_auth_schema_and_security_headers() {
    let port = reserve_port();
    let host = format!("127.0.0.1:{port}");
    let secret = "synthetic-machine-secret-value";
    let mut child = Command::new(env!("CARGO_BIN_EXE_act_mcp_server"))
        .arg(format!("--port={port}"))
        .env("SERVER_AUTH_SECRET", secret)
        .env("MCP_ALLOWED_HOSTS", &host)
        .env("MCP_ALLOWED_ORIGINS", "https://console.example")
        .env("RUST_LOG", "act_mcp_server=info")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ACT MCP server");
    wait_until_ready(port);

    let initialize = request(
        port,
        &host,
        Some(secret),
        r#"{"jsonrpc":"2.0","id":"init","method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"process-test","version":"1"}}}"#,
    );
    assert!(initialize.starts_with("HTTP/1.1 200"), "{initialize}");
    let lower = initialize.to_ascii_lowercase();
    assert!(lower.contains("cache-control: no-store"));
    assert!(lower.contains("x-content-type-options: nosniff"));
    assert!(lower.contains("referrer-policy: no-referrer"));
    assert!(lower.contains("content-security-policy: default-src 'none'; frame-ancestors 'none'"));
    assert!(initialize.contains("\"protocolVersion\":\"2025-11-25\""));

    let wrong_host = request(
        port,
        "act-mcp-server.attacker",
        Some(secret),
        r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
    );
    assert!(wrong_host.starts_with("HTTP/1.1 421"), "{wrong_host}");

    let missing_secret = request(
        port,
        &host,
        None,
        r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
    );
    assert!(missing_secret.starts_with("HTTP/1.1 401"), "{missing_secret}");

    let unknown_argument = request(
        port,
        &host,
        Some(secret),
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"ping","arguments":{"message":"hello","unexpected":true}}}"#,
    );
    assert!(unknown_argument.starts_with("HTTP/1.1 200"));
    assert!(unknown_argument.contains("\"code\":-32602"));
    assert!(unknown_argument.contains("ping contains unknown arguments"));

    let unknown_envelope = request(
        port,
        &host,
        Some(secret),
        r#"{"jsonrpc":"2.0","id":4,"method":"ping","unexpected":true}"#,
    );
    assert!(
        unknown_envelope.starts_with("HTTP/1.1 422")
            || unknown_envelope.starts_with("HTTP/1.1 400"),
        "{unknown_envelope}"
    );

    let _ = child.kill();
    let _ = child.wait();
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("stderr pipe")
        .read_to_string(&mut stderr)
        .expect("read stderr");
    assert!(!stderr.contains(secret));
    assert!(!stderr.contains("x-server-auth"));
}
