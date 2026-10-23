//! HTTP/1.1 over a real socket, for servers that hand their listener over: setup mode and a
//! `holocron serve` process.

use std::net::SocketAddr;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

pub struct Reply {
    pub status: u16,
    pub head: String,
    pub body: Value,
}

impl Reply {
    /// The `holocron_session=<token>` pair this answer sets, for a `Cookie` header.
    pub fn session(&self) -> Option<String> {
        self.head
            .lines()
            .filter_map(|line| line.split_once(": "))
            .filter(|(name, _)| name.eq_ignore_ascii_case("set-cookie"))
            .map(|(_, cookie)| cookie)
            .filter_map(|cookie| cookie.split(';').next())
            .find(|pair| pair.starts_with("holocron_session=") && pair.len() > 17)
            .map(str::to_owned)
    }

    pub fn code(&self) -> Option<&str> {
        self.body["error_code"].as_str()
    }
}

fn head(addr: SocketAddr, method: &str, path: &str, body: &str, cookie: Option<&str>) -> String {
    let cookie = cookie
        .map(|c| format!("Cookie: {c}\r\n"))
        .unwrap_or_default();
    format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nOrigin: http://{addr}\r\nX-Holocron-Request: 1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{cookie}Connection: close\r\n\r\n",
        body.len()
    )
}

/// One request with `Connection: close`, as a browser on this origin sends it.
pub async fn send(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: Option<&Value>,
    cookie: Option<&str>,
) -> std::io::Result<Reply> {
    let mut stream = tokio::net::TcpStream::connect(addr).await?;
    let body = body.map(Value::to_string).unwrap_or_default();
    stream
        .write_all(head(addr, method, path, &body, cookie).as_bytes())
        .await?;
    stream.write_all(body.as_bytes()).await?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await?;
    let raw = String::from_utf8_lossy(&raw);
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
    let status = head
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let head = head.to_owned();
    let body = if head.to_lowercase().contains("transfer-encoding: chunked") {
        dechunk(body)
    } else {
        body.to_owned()
    };
    Ok(Reply {
        status,
        head,
        body: serde_json::from_str(&body).unwrap_or(Value::Null),
    })
}

fn dechunk(mut body: &str) -> String {
    let mut out = String::new();
    while let Some((size, rest)) = body.split_once("\r\n") {
        let size = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
        if size == 0 || rest.len() < size {
            break;
        }
        out.push_str(&rest[..size]);
        body = rest[size..].trim_start_matches("\r\n");
    }
    out
}

pub async fn request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: Option<Value>,
    cookie: Option<&str>,
) -> Reply {
    send(addr, method, path, body.as_ref(), cookie)
        .await
        .unwrap_or_else(|e| panic!("{method} {path}: {e}"))
}

/// Waits until the server at `addr` answers `/health`.
pub async fn ready(addr: SocketAddr) {
    for _ in 0..600 {
        if let Ok(reply) = send(addr, "GET", "/health", None, None).await
            && reply.status == 200
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{addr} never answered /health");
}

/// Reads the server-sent events of `path` until one satisfies `until`, and returns it.
pub async fn event_until(
    addr: SocketAddr,
    path: &str,
    cookie: &str,
    until: impl Fn(&Value) -> bool,
) -> Value {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(head(addr, "GET", path, "", Some(cookie)).as_bytes())
        .await
        .unwrap();
    let mut lines = BufReader::new(stream).lines();
    while let Some(line) = lines.next_line().await.unwrap() {
        if let Some(data) = line.strip_prefix("data: ")
            && let Ok(event) = serde_json::from_str::<Value>(data)
            && until(&event)
        {
            return event;
        }
    }
    panic!("{path} ended before the awaited event");
}

/// A port nothing listens on right now.
pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
