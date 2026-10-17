//! Minimal HTTP/1.1 request parsing and response writing for the product web surface.
//!
//! The frozen product serves its browser interface from ASP.NET Core. The port keeps the
//! same user-visible routes and workflows but implements only the HTTP subset a browser
//! needs to drive a local, single-user interface: one request per connection, `GET`/`POST`/
//! `HEAD`/`PUT`, a query string, form-urlencoded bodies with `Content-Length`, and no
//! chunked transfer coding, compression, or keep-alive.
//!
//! Every input is bounded before it is allocated: the request line, the header block, the
//! header count and the body each have an explicit limit, and a request that exceeds one is
//! rejected with a diagnostic status instead of being read into memory.

use std::fmt;
use std::io::{self, BufRead, Write};

/// Maximum bytes accepted for the request line (`METHOD TARGET VERSION`).
pub const MAX_REQUEST_LINE_BYTES: usize = 8 * 1024;
/// Maximum number of header fields accepted per request.
pub const MAX_HEADER_COUNT: usize = 100;
/// Maximum bytes accepted across all header fields.
pub const MAX_HEADER_BYTES: usize = 32 * 1024;
/// Maximum body bytes accepted. Form posts carry a filesystem path and a flag.
pub const MAX_BODY_BYTES: usize = 1024 * 1024;

/// A parsed request head plus decoded body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Request {
    method: String,
    target: String,
    path: String,
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    /// Read one request from `reader`.
    ///
    /// Returns `Ok(None)` when the peer closed the connection cleanly before sending any
    /// byte, so a server loop can distinguish an idle disconnect from a malformed request.
    pub fn read(reader: &mut impl BufRead) -> Result<Option<Self>, HttpError> {
        let mut request_line = String::new();
        let read = read_line_bounded(reader, &mut request_line, MAX_REQUEST_LINE_BYTES)?;
        if read == 0 {
            return Ok(None);
        }
        let mut parts = request_line.split(' ');
        let method = parts.next().unwrap_or_default();
        let target = parts.next().unwrap_or_default();
        let version = parts.next().unwrap_or_default();
        if method.is_empty() || target.is_empty() || version.is_empty() || parts.next().is_some() {
            return Err(HttpError::Malformed {
                reason: "the request line must be `METHOD TARGET VERSION`",
            });
        }
        if !(version == "HTTP/1.1" || version == "HTTP/1.0") {
            return Err(HttpError::Malformed {
                reason: "only HTTP/1.1 and HTTP/1.0 are supported",
            });
        }
        let method = method.to_ascii_uppercase();
        if !matches!(
            method.as_str(),
            "GET" | "POST" | "HEAD" | "PUT" | "DELETE" | "OPTIONS"
        ) {
            return Err(HttpError::Malformed {
                reason: "unsupported request method",
            });
        }

        let mut headers = Vec::new();
        let mut header_bytes = 0usize;
        loop {
            let mut line = String::new();
            let read = read_line_bounded(reader, &mut line, MAX_HEADER_BYTES)?;
            if read == 0 {
                return Err(HttpError::Malformed {
                    reason: "the header block is unterminated",
                });
            }
            if line.is_empty() {
                break;
            }
            header_bytes += line.len();
            if header_bytes > MAX_HEADER_BYTES {
                return Err(HttpError::TooLarge {
                    what: "the header block",
                    limit: MAX_HEADER_BYTES,
                });
            }
            if headers.len() == MAX_HEADER_COUNT {
                return Err(HttpError::TooLarge {
                    what: "the header count",
                    limit: MAX_HEADER_COUNT,
                });
            }
            let (name, value) = line.split_once(':').ok_or(HttpError::Malformed {
                reason: "a header field has no colon",
            })?;
            if name.is_empty() || name.contains(' ') || name.contains('\t') {
                return Err(HttpError::Malformed {
                    reason: "a header field name is empty or contains whitespace",
                });
            }
            headers.push((name.to_ascii_lowercase(), value.trim().to_owned()));
        }

        if header(&headers, "transfer-encoding").is_some() {
            return Err(HttpError::Malformed {
                reason: "chunked transfer coding is not supported",
            });
        }

        let body = match header(&headers, "content-length") {
            None => Vec::new(),
            Some(text) => {
                let length: usize = text.trim().parse().map_err(|_| HttpError::Malformed {
                    reason: "the content length is not a valid number",
                })?;
                if length > MAX_BODY_BYTES {
                    return Err(HttpError::TooLarge {
                        what: "the request body",
                        limit: MAX_BODY_BYTES,
                    });
                }
                let mut body = vec![0u8; length];
                reader.read_exact(&mut body).map_err(HttpError::Io)?;
                body
            }
        };

        let (path, query) = split_target(target);
        Ok(Some(Self {
            method,
            target: target.to_owned(),
            path,
            query: parse_query(query),
            headers,
            body,
        }))
    }

    /// The upper-case method.
    pub fn method(&self) -> &str {
        &self.method
    }

    /// The request target exactly as sent.
    pub fn target(&self) -> &str {
        &self.target
    }

    /// The decoded path portion of the request target (no query string).
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The first query-string value named `name`, percent-decoded.
    pub fn query_value(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// The first query-string value named `name` parsed as a boolean flag.
    ///
    /// Mirrors the baseline's form handling: only the exact text `true` is true.
    #[cfg(test)]
    pub fn query_flag(&self, name: &str) -> bool {
        self.query_value(name) == Some("true")
    }

    /// The first header named `name` (case-insensitive).
    #[cfg(test)]
    pub fn header(&self, name: &str) -> Option<&str> {
        header(&self.headers, name)
    }

    /// The raw request body.
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// The first form value named `name` from a `application/x-www-form-urlencoded` body.
    pub fn form_value(&self, name: &str) -> Option<String> {
        parse_form(&self.body)
            .into_iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    /// Every form value named `name`, in body order.
    ///
    /// The baseline reads repeated fields through `TryGetStringArray` - the configuration page's
    /// list add posts one `Content` field per file - so the port needs the same view of the body
    /// rather than only the first value.
    pub fn form_values(&self, name: &str) -> Vec<String> {
        parse_form(&self.body)
            .into_iter()
            .filter(|(key, _)| key == name)
            .map(|(_, value)| value)
            .collect()
    }

    /// The boolean value of a form field, mirroring the baseline's `CreateSubfolder` rule:
    /// only the exact text `true` is true.
    pub fn form_flag(&self, name: &str) -> bool {
        self.form_value(name).as_deref() == Some("true")
    }
}

/// A response ready to be written.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Response {
    status: u16,
    content_type: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Response {
    /// A response with an explicit status, content type and body.
    pub fn new(status: u16, content_type: impl Into<String>, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            content_type: content_type.into(),
            headers: Vec::new(),
            body: body.into(),
        }
    }

    /// A UTF-8 `text/html` response.
    pub fn html(body: impl Into<String>) -> Self {
        Self::new(200, "text/html; charset=utf-8", body.into())
    }

    /// A UTF-8 `text/plain` response.
    pub fn text(body: impl Into<String>) -> Self {
        Self::new(200, "text/plain; charset=utf-8", body.into())
    }

    /// An `application/json` response.
    pub fn json(body: impl Into<String>) -> Self {
        Self::new(200, "application/json; charset=utf-8", body.into())
    }

    /// A binary response with an explicit content type.
    pub fn bytes(content_type: impl Into<String>, body: Vec<u8>) -> Self {
        Self::new(200, content_type, body)
    }

    /// A `302 Found` redirect, the status the baseline's `Results.Redirect` and
    /// `Response.Redirect` produce.
    pub fn redirect(location: &str) -> Self {
        Self::new(302, "text/plain; charset=utf-8", Vec::new()).with_header("Location", location)
    }

    /// A `400` response carrying a diagnostic message.
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::text(message)
            .with_status(400)
            .with_content_type("text/plain; charset=utf-8")
    }

    /// A `404` response.
    pub fn not_found() -> Self {
        Self::text("Not Found").with_status(404)
    }

    /// A `405` response naming the allowed methods.
    pub fn method_not_allowed(allow: &str) -> Self {
        Self::text("Method Not Allowed")
            .with_status(405)
            .with_header("Allow", allow)
    }

    /// A `500` response carrying a diagnostic message.
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::text(message).with_status(500)
    }

    /// A `501` response carrying a diagnostic message.
    #[cfg(test)]
    pub fn not_implemented(message: impl Into<String>) -> Self {
        Self::text(message).with_status(501)
    }

    /// A `204` response with no body.
    #[cfg(test)]
    pub fn no_content() -> Self {
        Self::new(204, "text/plain; charset=utf-8", Vec::new())
    }

    /// Replace the status code.
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    /// Replace the content type.
    pub fn with_content_type(mut self, content_type: impl Into<String>) -> Self {
        self.content_type = content_type.into();
        self
    }

    /// Append a header field.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// The status code.
    pub fn status(&self) -> u16 {
        self.status
    }

    /// The content type.
    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    /// The body bytes.
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// A header value previously set on this response (case-insensitive).
    #[cfg(test)]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// Write the response head and body.
    pub fn write_to(&self, writer: &mut impl Write) -> io::Result<()> {
        self.write_with_body(writer, true)
    }

    /// Write the response head for a `HEAD` request, omitting the body.
    pub fn write_head(&self, writer: &mut impl Write) -> io::Result<()> {
        self.write_with_body(writer, false)
    }

    fn write_with_body(&self, writer: &mut impl Write, include_body: bool) -> io::Result<()> {
        write!(
            writer,
            "HTTP/1.1 {} {}\r\n",
            self.status,
            reason_phrase(self.status)
        )?;
        write!(writer, "Content-Type: {}\r\n", self.content_type)?;
        write!(writer, "Content-Length: {}\r\n", self.body.len())?;
        write!(writer, "Cache-Control: no-store\r\n")?;
        for (name, value) in &self.headers {
            write!(writer, "{name}: {value}\r\n")?;
        }
        write!(writer, "Connection: close\r\n\r\n")?;
        if include_body {
            writer.write_all(&self.body)?;
        }
        writer.flush()
    }
}

/// Reasons a request could not be read.
#[derive(Debug)]
pub enum HttpError {
    /// The underlying stream failed.
    Io(io::Error),
    /// The request is syntactically invalid.
    Malformed { reason: &'static str },
    /// The request exceeds a configured bound.
    TooLarge { what: &'static str, limit: usize },
}

impl fmt::Display for HttpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "the connection failed: {error}"),
            Self::Malformed { reason } => write!(formatter, "the request is invalid: {reason}"),
            Self::TooLarge { what, limit } => {
                write!(formatter, "{what} exceeds the {limit}-byte limit")
            }
        }
    }
}

impl std::error::Error for HttpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

/// The response that answers a rejected request, so a client sees a defined status.
pub fn error_response(error: &HttpError) -> Response {
    match error {
        HttpError::TooLarge { .. } => Response::text(error.to_string()).with_status(413),
        HttpError::Malformed { .. } => Response::bad_request(error.to_string()),
        HttpError::Io(_) => Response::internal_error(error.to_string()),
    }
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        302 => "Found",
        303 => "See Other",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        _ => "OK",
    }
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// Read one CRLF (or bare LF) terminated line into `line`, without the terminator.
///
/// Returns the number of bytes read, including the terminator. `line` is cleared first, and
/// a line longer than `limit` fails instead of growing without bound.
fn read_line_bounded(
    reader: &mut impl BufRead,
    line: &mut String,
    limit: usize,
) -> Result<usize, HttpError> {
    line.clear();
    let mut total = 0usize;
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        let read = reader
            .read_until(b'\n', &mut buffer)
            .map_err(HttpError::Io)?;
        if read == 0 {
            if total == 0 {
                return Ok(0);
            }
            return Err(HttpError::Malformed {
                reason: "a header line is unterminated",
            });
        }
        total += read;
        if total > limit {
            return Err(HttpError::TooLarge {
                what: "a request line",
                limit,
            });
        }
        let text = core::str::from_utf8(&buffer).map_err(|_| HttpError::Malformed {
            reason: "the request head is not valid UTF-8",
        })?;
        line.push_str(text.trim_end_matches(['\r', '\n']));
        if line.ends_with('\r') {
            line.pop();
        }
        if buffer.last() == Some(&b'\n') {
            return Ok(total);
        }
    }
}

fn split_target(target: &str) -> (String, &str) {
    // Absolute-form targets (`http://host/path`) are tolerated by taking the path portion.
    let path_and_query = match target.find("://") {
        Some(index) => match target[index + 3..].find('/') {
            Some(slash) => &target[index + 3 + slash..],
            None => "/",
        },
        None => target,
    };
    match path_and_query.split_once('?') {
        Some((path, query)) => (
            percent_decode(path).unwrap_or_else(|| path.to_owned()),
            query,
        ),
        None => (
            percent_decode(path_and_query).unwrap_or_else(|| path_and_query.to_owned()),
            "",
        ),
    }
}

fn parse_query(query: &str) -> Vec<(String, String)> {
    if query.is_empty() {
        return Vec::new();
    }
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((key, value)) => (
                percent_decode_form(key).unwrap_or_else(|| key.to_owned()),
                percent_decode_form(value).unwrap_or_else(|| value.to_owned()),
            ),
            None => (
                percent_decode_form(pair).unwrap_or_else(|| pair.to_owned()),
                String::new(),
            ),
        })
        .collect()
}

/// Parse an `application/x-www-form-urlencoded` body.
pub fn parse_form(body: &[u8]) -> Vec<(String, String)> {
    let text = match core::str::from_utf8(body) {
        Ok(text) => text,
        Err(_) => return Vec::new(),
    };
    text.split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((key, value)) => (
                percent_decode_form(key).unwrap_or_else(|| key.to_owned()),
                percent_decode_form(value).unwrap_or_else(|| value.to_owned()),
            ),
            None => (
                percent_decode_form(pair).unwrap_or_else(|| pair.to_owned()),
                String::new(),
            ),
        })
        .collect()
}

/// Percent-decode a component, rejecting malformed escapes.
pub fn percent_decode(text: &str) -> Option<String> {
    decode(text, false)
}

/// Percent-decode a form component, where `+` means a space.
pub fn percent_decode_form(text: &str) -> Option<String> {
    decode(text, true)
}

fn decode(text: &str, plus_is_space: bool) -> Option<String> {
    let bytes = text.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let high = hex_value(*bytes.get(index + 1)?)?;
                let low = hex_value(*bytes.get(index + 2)?)?;
                output.push((high << 4) | low);
                index += 3;
            }
            b'+' if plus_is_space => {
                output.push(b' ');
                index += 1;
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(output).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        error_response, parse_form, percent_decode, percent_decode_form, HttpError, Request,
        Response, MAX_BODY_BYTES,
    };
    use std::io::{BufReader, Cursor};

    fn read(text: &str) -> Result<Option<Request>, HttpError> {
        let mut reader = BufReader::new(Cursor::new(text.as_bytes().to_vec()));
        Request::read(&mut reader)
    }

    #[test]
    fn reader_parses_a_get_with_a_query_string() {
        let request = read(
            "GET /Assets/View?path=Assets%2FScene.unity&name=a+b HTTP/1.1\r\n\
             Host: 127.0.0.1:5000\r\n\
             User-Agent: test\r\n\r\n",
        )
        .unwrap()
        .unwrap();
        assert_eq!(request.method(), "GET");
        assert_eq!(request.path(), "/Assets/View");
        assert_eq!(request.query_value("path"), Some("Assets/Scene.unity"));
        // Query values use form decoding, so `+` is a space.
        assert_eq!(request.query_value("name"), Some("a b"));
        assert_eq!(request.query_value("missing"), None);
        assert_eq!(request.header("host"), Some("127.0.0.1:5000"));
        assert_eq!(request.header("HOST"), Some("127.0.0.1:5000"));
        assert!(request.body().is_empty());
        assert_eq!(
            request.target(),
            "/Assets/View?path=Assets%2FScene.unity&name=a+b"
        );
    }

    #[test]
    fn reader_parses_a_form_post_body() {
        let body = "Path=C%3A%5Crip%2Csample&CreateSubfolder=true";
        let request = read(&format!(
            "POST /Export/UnityProject HTTP/1.1\r\nContent-Length: {}\r\n\
             Content-Type: application/x-www-form-urlencoded\r\n\r\n{body}",
            body.len()
        ))
        .unwrap()
        .unwrap();
        assert_eq!(
            request.form_value("Path"),
            Some("C:\\rip,sample".to_owned())
        );
        assert!(request.form_flag("CreateSubfolder"));
        assert!(!request.form_flag("Missing"));
        assert_eq!(request.form_value("missing"), None);
    }

    #[test]
    fn reader_reports_a_clean_disconnect_separately() {
        assert!(read("").unwrap().is_none());
    }

    #[test]
    fn reader_rejects_malformed_and_oversized_inputs() {
        for (text, expected) in [
            (
                "GET / HTTP/2.0\r\n\r\n",
                "only HTTP/1.1 and HTTP/1.0 are supported",
            ),
            (
                "GET /foo\r\n\r\n",
                "the request line must be `METHOD TARGET VERSION`",
            ),
            (
                "GET / HTTP/1.1\r\nBroken\r\n\r\n",
                "a header field has no colon",
            ),
            (
                "GET / HTTP/1.1\r\nBad Name: x\r\n\r\n",
                "a header field name is empty or contains whitespace",
            ),
            (
                "GET / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n",
                "chunked transfer coding is not supported",
            ),
            (
                "GET / HTTP/1.1\r\nContent-Length: abc\r\n\r\n",
                "the content length is not a valid number",
            ),
            ("GET / HTTP/1.1\r\n", "the header block is unterminated"),
            ("TRACE / HTTP/1.1\r\n\r\n", "unsupported request method"),
            (
                "GET /\r\n\r\n",
                "the request line must be `METHOD TARGET VERSION`",
            ),
        ] {
            match read(text).unwrap_err() {
                HttpError::Malformed { reason } => assert_eq!(reason, expected, "{text:?}"),
                other => panic!("expected malformed for {text:?}, got {other:?}"),
            }
        }

        let long_line = format!("GET /{} HTTP/1.1\r\n\r\n", "a".repeat(9 * 1024));
        assert!(matches!(
            read(&long_line).unwrap_err(),
            HttpError::TooLarge { limit, .. } if limit == super::MAX_REQUEST_LINE_BYTES
        ));

        let oversized = format!(
            "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY_BYTES + 1
        );
        assert!(matches!(
            read(&oversized).unwrap_err(),
            HttpError::TooLarge {
                what: "the request body",
                ..
            }
        ));

        let many_headers = format!(
            "GET / HTTP/1.1\r\n{}\r\n",
            (0..super::MAX_HEADER_COUNT + 1)
                .map(|index| format!("X-{index}: 1\r\n"))
                .collect::<String>()
        );
        assert!(matches!(
            read(&many_headers).unwrap_err(),
            HttpError::TooLarge {
                what: "the header count",
                ..
            }
        ));
        // A 413 answers an oversized body, and a 400 answers malformed syntax.
        let response = error_response(&HttpError::TooLarge {
            what: "the request body",
            limit: MAX_BODY_BYTES,
        });
        assert_eq!(response.status(), 413);
        let response = error_response(&HttpError::Malformed { reason: "x" });
        assert_eq!(response.status(), 400);
    }

    #[test]
    fn reader_rejects_a_non_utf8_head() {
        let mut bytes = b"GET / HTTP/1.1\r\nX: ".to_vec();
        bytes.push(0xFF);
        bytes.extend_from_slice(b"\r\n\r\n");
        let mut reader = BufReader::new(Cursor::new(bytes));
        assert!(matches!(
            Request::read(&mut reader).unwrap_err(),
            HttpError::Malformed {
                reason: "the request head is not valid UTF-8"
            }
        ));
    }

    #[test]
    fn reader_accepts_lf_and_absolute_form_targets() {
        let request = read("GET http://127.0.0.1:5000/Commands?x=1 HTTP/1.1\n\n")
            .unwrap()
            .unwrap();
        assert_eq!(request.path(), "/Commands");
        assert_eq!(request.query_value("x"), Some("1"));
    }

    #[test]
    fn writer_emits_a_complete_response_and_a_head_variant() {
        let response = Response::html("<p>hello</p>").with_header("X-Test", "1");
        let mut bytes = Vec::new();
        response.write_to(&mut bytes).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "{text}");
        assert!(
            text.contains("Content-Type: text/html; charset=utf-8\r\n"),
            "{text}"
        );
        assert!(text.contains("Content-Length: 12\r\n"), "{text}");
        assert!(text.contains("Cache-Control: no-store\r\n"), "{text}");
        assert!(text.contains("X-Test: 1\r\n"), "{text}");
        assert!(text.contains("Connection: close\r\n"), "{text}");
        assert!(text.ends_with("\r\n\r\n<p>hello</p>"), "{text}");

        let mut bytes = Vec::new();
        response.write_head(&mut bytes).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.ends_with("\r\n\r\n"), "{text}");
        assert_eq!(response.body(), b"<p>hello</p>");
        assert_eq!(response.content_type(), "text/html; charset=utf-8");
    }

    #[test]
    fn constructors_cover_the_statuses_the_routes_need() {
        assert_eq!(Response::redirect("/").status(), 302);
        assert_eq!(Response::redirect("/").header("Location"), Some("/"));
        assert_eq!(Response::not_found().status(), 404);
        assert_eq!(Response::bad_request("x").status(), 400);
        assert_eq!(Response::internal_error("x").status(), 500);
        assert_eq!(Response::not_implemented("x").status(), 501);
        assert_eq!(Response::no_content().status(), 204);
        assert_eq!(
            Response::method_not_allowed("GET, POST").header("Allow"),
            Some("GET, POST")
        );
        assert_eq!(
            Response::json("{}").content_type(),
            "application/json; charset=utf-8"
        );
        assert_eq!(
            Response::bytes("image/png", vec![1, 2]).content_type(),
            "image/png"
        );
        assert_eq!(
            Response::text("x")
                .with_status(201)
                .with_content_type("text/x")
                .content_type(),
            "text/x"
        );
    }

    #[test]
    fn decoders_handle_escapes_and_reject_malformed_sequences() {
        assert_eq!(percent_decode("a%20b"), Some("a b".to_owned()));
        assert_eq!(percent_decode("a+b"), Some("a+b".to_owned()));
        assert_eq!(percent_decode_form("a+b"), Some("a b".to_owned()));
        assert_eq!(percent_decode("%zz"), None);
        assert_eq!(percent_decode("%2"), None);
        assert_eq!(percent_decode("%ff"), None);
        assert_eq!(
            parse_form(b"a=1&b=&c=x+y&d"),
            vec![
                ("a".to_owned(), "1".to_owned()),
                ("b".to_owned(), String::new()),
                ("c".to_owned(), "x y".to_owned()),
                ("d".to_owned(), String::new()),
            ]
        );
        assert!(parse_form(&[0xFF]).is_empty());
    }

    #[test]
    fn accessors_expose_the_parsed_request_shape() {
        let request = read("POST /Reset HTTP/1.1\r\nContent-Length: 8\r\n\r\nA=1&B=2!")
            .unwrap()
            .unwrap();
        assert_eq!(request.method(), "POST");
        assert_eq!(request.path(), "/Reset");
        assert_eq!(request.body(), b"A=1&B=2!");
        assert_eq!(request.form_value("B"), Some("2!".to_owned()));
        assert!(request.query_value("x").is_none());
        assert!(!request.query_flag("x"));
        assert_eq!(request.header("content-length"), Some("8"));
    }
}
