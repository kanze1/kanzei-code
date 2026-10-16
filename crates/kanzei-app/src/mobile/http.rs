//! HTTP/1 request framing and connection ownership for the mobile bridge.
//!
//! The bridge is a JSON/text remote, not a file-upload service. Its protocol
//! permits 16 KiB of headers, 256 KiB of JSON, and 4 KiB for pairing. One request
//! has ten seconds to arrive, irrespective of how often a peer sends a byte.
//! Streaming responses share a bounded set of 32 owned connections; shutdown
//! closes their sockets and joins their workers before releasing the service.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub(super) const MAX_CONNECTIONS: usize = 32;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_HEADERS: usize = 64;
pub(super) const MAX_BODY_BYTES: usize = 256 * 1024;
pub(super) const MAX_PAIR_BYTES: usize = 4 * 1024;
const REQUEST_DEADLINE: Duration = Duration::from_secs(10);
const WRITE_DEADLINE: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub(super) struct RequestError {
    status: &'static str,
    message: &'static str,
}

impl RequestError {
    fn bad_request(message: &'static str) -> Self {
        Self {
            status: "400 Bad Request",
            message,
        }
    }

    pub(super) fn respond(&self, stream: &mut TcpStream) {
        let _ = stream.set_write_timeout(Some(WRITE_DEADLINE));
        let _ = stream.write_all(&super::mobile_json_response(
            self.status,
            &serde_json::json!({"error": self.message}),
        ));
    }
}

pub(super) struct RequestHead {
    pub(super) raw: String,
    pub(super) method: String,
    pub(super) path: String,
    content_length: usize,
    received_body: Vec<u8>,
    deadline: Instant,
}

fn read_before(
    stream: &mut TcpStream,
    buffer: &mut [u8],
    deadline: Instant,
) -> Result<usize, RequestError> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
        .ok_or(RequestError {
            status: "408 Request Timeout",
            message: "request_deadline_exceeded",
        })?;
    stream
        .set_read_timeout(Some(remaining))
        .map_err(|_| RequestError::bad_request("socket_timeout_failed"))?;
    match stream.read(buffer) {
        Ok(0) => Err(RequestError::bad_request("incomplete_request")),
        Ok(count) => Ok(count),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ) =>
        {
            Err(RequestError {
                status: "408 Request Timeout",
                message: "request_deadline_exceeded",
            })
        }
        Err(_) => Err(RequestError::bad_request("request_read_failed")),
    }
}

pub(super) fn read_head(stream: &mut TcpStream) -> Result<RequestHead, RequestError> {
    read_head_until(stream, Instant::now() + REQUEST_DEADLINE)
}

fn read_head_until(stream: &mut TcpStream, deadline: Instant) -> Result<RequestHead, RequestError> {
    stream
        .set_write_timeout(Some(WRITE_DEADLINE))
        .map_err(|_| RequestError::bad_request("socket_timeout_failed"))?;
    let mut buffer = Vec::new();
    loop {
        let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
        let mut request = httparse::Request::new(&mut headers);
        match request.parse(&buffer) {
            Ok(httparse::Status::Complete(header_end)) => {
                let mut length = None;
                for header in request.headers.iter() {
                    if header.name.eq_ignore_ascii_case("transfer-encoding") {
                        return Err(RequestError::bad_request("transfer_encoding_not_supported"));
                    }
                    if header.name.eq_ignore_ascii_case("content-length") {
                        if length.is_some() {
                            return Err(RequestError::bad_request("duplicate_content_length"));
                        }
                        let value = std::str::from_utf8(header.value)
                            .ok()
                            .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
                            .and_then(|v| v.parse::<usize>().ok())
                            .ok_or_else(|| RequestError::bad_request("invalid_content_length"))?;
                        length = Some(value);
                    }
                }
                let method = request
                    .method
                    .ok_or_else(|| RequestError::bad_request("method_required"))?
                    .to_owned();
                let path = request
                    .path
                    .ok_or_else(|| RequestError::bad_request("path_required"))?
                    .to_owned();
                let raw = std::str::from_utf8(&buffer[..header_end])
                    .map_err(|_| RequestError::bad_request("invalid_header_encoding"))?
                    .to_owned();
                return Ok(RequestHead {
                    raw,
                    method,
                    path,
                    content_length: length.unwrap_or(0),
                    received_body: buffer[header_end..].to_vec(),
                    deadline,
                });
            }
            Ok(httparse::Status::Partial) => {}
            Err(httparse::Error::TooManyHeaders) => {
                return Err(RequestError {
                    status: "431 Request Header Fields Too Large",
                    message: "too_many_headers",
                })
            }
            Err(_) => return Err(RequestError::bad_request("malformed_http_request")),
        }
        if buffer.len() == MAX_HEADER_BYTES {
            return Err(RequestError {
                status: "431 Request Header Fields Too Large",
                message: "headers_too_large",
            });
        }
        let mut chunk = [0u8; 4096];
        let read_limit = chunk.len().min(MAX_HEADER_BYTES - buffer.len());
        let count = read_before(stream, &mut chunk[..read_limit], deadline)?;
        buffer.extend_from_slice(&chunk[..count]);
    }
}

impl RequestHead {
    pub(super) fn read_body(
        &mut self,
        stream: &mut TcpStream,
        limit: usize,
    ) -> Result<Vec<u8>, RequestError> {
        if self.content_length > limit {
            return Err(RequestError {
                status: "413 Payload Too Large",
                message: "body_too_large",
            });
        }
        let mut body = std::mem::take(&mut self.received_body);
        body.truncate(self.content_length);
        while body.len() < self.content_length {
            let mut chunk = [0u8; 4096];
            let read_limit = chunk.len().min(self.content_length - body.len());
            let count = read_before(stream, &mut chunk[..read_limit], self.deadline)?;
            body.extend_from_slice(&chunk[..count]);
        }
        Ok(body)
    }
}

struct Connection {
    socket: std::sync::Arc<TcpStream>,
    worker: JoinHandle<()>,
}

struct CloseConnection(std::sync::Arc<TcpStream>);
impl Drop for CloseConnection {
    fn drop(&mut self) {
        // The owner's cancellation handle must not keep a completed HTTP
        // response open until another connection happens to trigger reaping.
        let _ = self.0.shutdown(Shutdown::Both);
    }
}

#[derive(Default)]
pub(super) struct Connections(Vec<Connection>);

impl Connections {
    pub(super) fn admit(
        &mut self,
        mut stream: TcpStream,
        handler: impl FnOnce(TcpStream) + Send + 'static,
    ) -> std::io::Result<bool> {
        let mut index = 0;
        while index < self.0.len() {
            if self.0[index].worker.is_finished() {
                let finished = self.0.swap_remove(index);
                if finished.worker.join().is_err() {
                    tracing::error!("mobile connection worker panicked");
                }
            } else {
                index += 1;
            }
        }
        if self.0.len() >= MAX_CONNECTIONS {
            RequestError {
                status: "503 Service Unavailable",
                message: "connection_limit_reached",
            }
            .respond(&mut stream);
            return Ok(false);
        }
        stream.set_read_timeout(Some(REQUEST_DEADLINE))?;
        stream.set_write_timeout(Some(WRITE_DEADLINE))?;
        let socket = std::sync::Arc::new(stream.try_clone()?);
        let close = CloseConnection(socket.clone());
        let worker = std::thread::Builder::new()
            .name("mobile-http".into())
            .spawn(move || {
                let _close = close;
                handler(stream);
            })?;
        self.0.push(Connection { socket, worker });
        Ok(true)
    }
}

impl Drop for Connections {
    fn drop(&mut self) {
        for connection in &self.0 {
            let _ = connection.socket.shutdown(Shutdown::Both);
        }
        for connection in self.0.drain(..) {
            if connection.worker.join().is_err() {
                tracing::error!("mobile connection worker panicked during shutdown");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn sockets() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        (client, listener.accept().unwrap().0)
    }

    #[test]
    fn framing_rejects_ambiguous_and_oversized_requests_before_body_read() {
        for (headers, error) in [
            (
                "Content-Length: 1\r\nContent-Length: 2\r\n",
                "duplicate_content_length",
            ),
            (
                "Transfer-Encoding: chunked\r\n",
                "transfer_encoding_not_supported",
            ),
            ("Content-Length: -1\r\n", "invalid_content_length"),
        ] {
            let (mut client, mut server) = sockets();
            write!(client, "POST /v1/messages HTTP/1.1\r\n{headers}\r\n").unwrap();
            assert_eq!(read_head(&mut server).err().unwrap().message, error);
        }
        let (mut client, mut server) = sockets();
        write!(
            client,
            "POST /v1/pair HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_PAIR_BYTES + 1
        )
        .unwrap();
        let mut head = read_head(&mut server).unwrap();
        assert_eq!(
            head.read_body(&mut server, MAX_PAIR_BYTES)
                .unwrap_err()
                .status,
            "413 Payload Too Large"
        );
    }

    #[test]
    fn header_and_body_share_one_deadline() {
        let (mut client, mut server) = sockets();
        client
            .write_all(b"POST /v1/messages HTTP/1.1\r\nContent-Length: 2\r\n\r\nx")
            .unwrap();
        let mut head =
            read_head_until(&mut server, Instant::now() + Duration::from_millis(40)).unwrap();
        head.deadline = Instant::now();
        assert_eq!(
            head.read_body(&mut server, MAX_BODY_BYTES)
                .unwrap_err()
                .status,
            "408 Request Timeout"
        );
    }

    #[test]
    fn connection_owner_bounds_admission_and_joins_blocked_readers() {
        let mut connections = Connections::default();
        let mut clients = Vec::new();
        let completed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let admitted = std::sync::Arc::new(std::sync::Barrier::new(MAX_CONNECTIONS + 1));
        for _ in 0..MAX_CONNECTIONS {
            let (client, server) = sockets();
            clients.push(client);
            let completed = completed.clone();
            let admitted = admitted.clone();
            assert!(connections
                .admit(server, move |mut stream| {
                    admitted.wait();
                    stream
                        .set_read_timeout(Some(Duration::from_millis(100)))
                        .unwrap();
                    let _ = stream.read(&mut [0]);
                    completed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                })
                .unwrap());
        }
        let (mut extra_client, extra_server) = sockets();
        assert!(!connections
            .admit(extra_server, |_| panic!("must not admit"))
            .unwrap());
        let mut response = String::new();
        extra_client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 503"));
        admitted.wait();
        drop(connections);
        assert_eq!(
            completed.load(std::sync::atomic::Ordering::SeqCst),
            MAX_CONNECTIONS
        );
    }

    #[test]
    fn finished_response_closes_without_waiting_for_the_next_accept() {
        let (mut client, server) = sockets();
        client
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut connections = Connections::default();
        assert!(connections
            .admit(server, |mut stream| {
                stream.write_all(b"response").unwrap();
            })
            .unwrap());
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert_eq!(response, "response");
        drop(connections);
    }
}
