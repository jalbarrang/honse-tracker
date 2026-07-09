//! Minimal blocking HTTP/1.1 POST over `std::net::TcpStream`.
//!
//! We only ever talk to a localhost ingest webhook with a fixed request shape, so
//! a hand-rolled POST avoids pulling in an HTTP/TLS client (and its API churn).
//! `Connection: close` keeps it stateless; connecting to localhost is sub-ms.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::config::Endpoint;

const CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
const IO_TIMEOUT: Duration = Duration::from_millis(500);

/// POST `body` (protobuf) to the endpoint. Returns `Ok(())` on a 2xx status.
pub fn post(endpoint: &Endpoint, content_type: &str, body: &[u8]) -> Result<(), String> {
    let addr = format!("{}:{}", endpoint.host, endpoint.port);
    let sock_addr = addr
        .to_socket_addrs_first()
        .ok_or_else(|| format!("resolve failed: {addr}"))?;

    let mut stream = TcpStream::connect_timeout(&sock_addr, CONNECT_TIMEOUT).map_err(|e| format!("connect: {e}"))?;
    stream.set_write_timeout(Some(IO_TIMEOUT)).ok();
    stream.set_read_timeout(Some(IO_TIMEOUT)).ok();
    stream.set_nodelay(true).ok();

    let header = format!(
        "POST {} HTTP/1.1\r\n\
         Host: {}\r\n\
         Content-Type: {}\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n",
        endpoint.path,
        endpoint.host,
        content_type,
        body.len()
    );

    stream
        .write_all(header.as_bytes())
        .map_err(|e| format!("write head: {e}"))?;
    stream.write_all(body).map_err(|e| format!("write body: {e}"))?;
    stream.flush().ok();

    // Read just enough to learn the status line; ignore the rest.
    let mut buf = [0u8; 64];
    let n = stream.read(&mut buf).map_err(|e| format!("read: {e}"))?;
    parse_status_ok(&buf[..n])
}

/// Validate that the response begins with `HTTP/1.x 2xx`.
fn parse_status_ok(head: &[u8]) -> Result<(), String> {
    let text = String::from_utf8_lossy(head);
    let mut parts = text.split_whitespace();
    let _version = parts.next();
    match parts.next().and_then(|c| c.parse::<u16>().ok()) {
        Some(code) if (200..300).contains(&code) => Ok(()),
        Some(code) => Err(format!("http status {code}")),
        None => Err("malformed response".to_string()),
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

    #[test]
    fn status_2xx_is_ok() {
        assert!(parse_status_ok(b"HTTP/1.1 200 OK\r\n").is_ok());
        assert!(parse_status_ok(b"HTTP/1.1 204 No Content\r\n").is_ok());
    }

    #[test]
    fn status_non_2xx_is_err() {
        assert!(parse_status_ok(b"HTTP/1.1 404 Not Found\r\n").is_err());
        assert!(parse_status_ok(b"HTTP/1.1 500 Internal\r\n").is_err());
        assert!(parse_status_ok(b"garbage").is_err());
    }
}
