//! Minimal blocking HTTP/1.1 POST over `std::net::TcpStream`.
//!
//! We only ever talk to the localhost sidecar ingest endpoint with a fixed
//! request shape, so a hand-rolled POST avoids pulling in an HTTP/TLS client
//! (and its API churn). `Connection: close` keeps it stateless; connecting to
//! localhost is sub-ms. Requests carry the per-install bearer token.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::config::{BearerToken, Endpoint};

const CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
const IO_TIMEOUT: Duration = Duration::from_millis(500);

/// Delivery failure classification driving the retry policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostError {
    /// Connect/IO failure or a 5xx — the sidecar may come back; retry.
    Transient(String),
    /// The sidecar answered with a 4xx (bad token, malformed payload, ...).
    /// Retrying the identical body can never succeed; drop it.
    Rejected(u16),
}

impl std::fmt::Display for PostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transient(msg) => write!(f, "transient: {msg}"),
            Self::Rejected(code) => write!(f, "rejected: http status {code}"),
        }
    }
}

/// POST `body` (protobuf) to the endpoint. Returns `Ok(())` on a 2xx status.
pub fn post(
    endpoint: &Endpoint,
    token: Option<&BearerToken>,
    content_type: &str,
    body: &[u8],
) -> Result<(), PostError> {
    let addr = format!("{}:{}", endpoint.host, endpoint.port);
    let sock_addr = addr
        .to_socket_addrs_first()
        .ok_or_else(|| PostError::Transient(format!("resolve failed: {addr}")))?;

    let mut stream = TcpStream::connect_timeout(&sock_addr, CONNECT_TIMEOUT)
        .map_err(|e| PostError::Transient(format!("connect: {e}")))?;
    stream.set_write_timeout(Some(IO_TIMEOUT)).ok();
    stream.set_read_timeout(Some(IO_TIMEOUT)).ok();
    stream.set_nodelay(true).ok();

    let header = request_head(endpoint, token, content_type, body.len());

    stream
        .write_all(header.as_bytes())
        .map_err(|e| PostError::Transient(format!("write head: {e}")))?;
    stream
        .write_all(body)
        .map_err(|e| PostError::Transient(format!("write body: {e}")))?;
    stream.flush().ok();

    // Read just enough to learn the status line; ignore the rest.
    let mut buf = [0u8; 64];
    let n = stream
        .read(&mut buf)
        .map_err(|e| PostError::Transient(format!("read: {e}")))?;
    parse_status(&buf[..n])
}

/// Build the request head. Pure so header shape (incl. the Authorization
/// bearer line) is unit-testable without a socket.
fn request_head(endpoint: &Endpoint, token: Option<&BearerToken>, content_type: &str, body_len: usize) -> String {
    let auth_line = token.map_or_else(String::new, |t| format!("Authorization: Bearer {}\r\n", t.expose()));
    format!(
        "POST {} HTTP/1.1\r\n\
         Host: {}\r\n\
         Content-Type: {}\r\n\
         Content-Length: {}\r\n\
         {auth_line}Connection: close\r\n\
         \r\n",
        endpoint.path, endpoint.host, content_type, body_len
    )
}

/// Classify the response status line: 2xx succeeds, 4xx is a permanent
/// rejection of this body, anything else (5xx, garbage) is transient.
fn parse_status(head: &[u8]) -> Result<(), PostError> {
    let text = String::from_utf8_lossy(head);
    let mut parts = text.split_whitespace();
    let _version = parts.next();
    match parts.next().and_then(|c| c.parse::<u16>().ok()) {
        Some(code) if (200..300).contains(&code) => Ok(()),
        Some(code) if (400..500).contains(&code) => Err(PostError::Rejected(code)),
        Some(code) => Err(PostError::Transient(format!("http status {code}"))),
        None => Err(PostError::Transient("malformed response".to_string())),
    }
}

/// Tiny helper so we don't pull `ToSocketAddrs` ceremony into the hot path.
trait FirstSocketAddr {
    fn to_socket_addrs_first(&self) -> Option<std::net::SocketAddr>;
}

impl FirstSocketAddr for str {
    fn to_socket_addrs_first(&self) -> Option<std::net::SocketAddr> {
        use std::net::ToSocketAddrs;
        self.to_socket_addrs().ok()?.next()
    }
}

impl FirstSocketAddr for String {
    fn to_socket_addrs_first(&self) -> Option<std::net::SocketAddr> {
        self.as_str().to_socket_addrs_first()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint() -> Endpoint {
        Endpoint::parse("http://127.0.0.1:8716/v1/turns").expect("parse")
    }

    #[test]
    fn status_2xx_is_ok() {
        assert!(parse_status(b"HTTP/1.1 200 OK\r\n").is_ok());
        assert!(parse_status(b"HTTP/1.1 201 Created\r\n").is_ok());
        assert!(parse_status(b"HTTP/1.1 204 No Content\r\n").is_ok());
    }

    #[test]
    fn status_4xx_is_permanent_rejection() {
        assert_eq!(
            parse_status(b"HTTP/1.1 401 Unauthorized\r\n"),
            Err(PostError::Rejected(401))
        );
        assert_eq!(
            parse_status(b"HTTP/1.1 400 Bad Request\r\n"),
            Err(PostError::Rejected(400))
        );
        assert_eq!(
            parse_status(b"HTTP/1.1 413 Too Large\r\n"),
            Err(PostError::Rejected(413))
        );
    }

    #[test]
    fn status_5xx_and_garbage_are_transient() {
        assert!(matches!(
            parse_status(b"HTTP/1.1 500 Internal\r\n"),
            Err(PostError::Transient(_))
        ));
        assert!(matches!(parse_status(b"garbage"), Err(PostError::Transient(_))));
    }

    #[test]
    fn request_head_carries_bearer_token() {
        let token = BearerToken::new("tok-123".to_string());
        let head = request_head(&endpoint(), Some(&token), "application/x-protobuf", 42);
        assert!(head.starts_with("POST /v1/turns HTTP/1.1\r\n"));
        assert!(head.contains("Host: 127.0.0.1\r\n"));
        assert!(head.contains("Content-Type: application/x-protobuf\r\n"));
        assert!(head.contains("Content-Length: 42\r\n"));
        assert!(head.contains("Authorization: Bearer tok-123\r\n"));
        assert!(head.ends_with("\r\n\r\n"));
    }

    #[test]
    fn request_head_without_token_has_no_auth_line() {
        let head = request_head(&endpoint(), None, "application/x-protobuf", 7);
        assert!(!head.contains("Authorization"));
    }
}
