use hyper::client::HttpConnector;
use hyper_rustls::HttpsConnector;

pub type Connector = HttpsConnector<HttpConnector>;

#[derive(Debug)]
pub struct QueryMap<'a>(Vec<(&'a str, &'a str)>);

impl<'a> QueryMap<'a> {
    pub fn parse(query: Option<&'a str>) -> anyhow::Result<QueryMap<'a>> {
        let mut vec = Vec::new();
        if let Some(query) = query {
            let parts = query.split("&");
            for part in parts {
                let (name, value) = part.split_once('=').unwrap_or((part, ""));
                vec.push((name, value));
            }
        }
        Ok(QueryMap(vec))
    }

    pub fn get(&'a self, key: &str) -> Option<&'a str> {
        for (k, v) in self.0.iter() {
            if *k == key {
                return Some(*v);
            }
        }
        None
    }
}

pub(super) mod server {
    use std::str::{self};

    use anyhow::Context;
    use chrono::Utc;
    use http::{HeaderValue, Method, Request, Uri};
    use tokio::io;

    use super::util::read_until_pattern;

    pub async fn parse_request<R>(reader: R) -> anyhow::Result<Request<Vec<u8>>>
    where
        R: io::AsyncBufRead,
    {
        use io::AsyncReadExt;

        tokio::pin!(reader);

        const DELIM: &[u8; 2] = b"\r\n";

        let mut buf = Vec::new();
        read_until_pattern(&mut reader, DELIM, &mut buf).await?;
        if buf.is_empty() {
            anyhow::bail!("Empty HTTP request");
        }
        let (method, uri) = parse_command(&buf)?;

        let mut req = Request::builder().method(method).uri(uri);

        let mut content_length: Option<usize> = None;
        loop {
            buf.clear();
            read_until_pattern(&mut reader, DELIM, &mut buf).await?;
            if buf.len() <= 2 {
                break;
            }
            let header = parse_header(&buf)?;
            if str::eq_ignore_ascii_case(&header.0, "transfer-encoding") {
                anyhow::bail!("Unsupported header: Transfer-Encoding")
            }
            if str::eq_ignore_ascii_case(&header.0, "content-length") {
                content_length = Some(header.1.parse()?);
            }
            let (name, value) = parse_header(&buf)?;
            req = req.header(name, value.parse::<HeaderValue>()?);
        }
        buf.clear();
        if let Some(len) = content_length {
            if len > buf.capacity() {
                buf.reserve(len - buf.capacity());
            }
            unsafe {
                buf.set_len(len);
            }
            reader.read_exact(&mut buf).await?;
        }
        Ok(req.body(buf)?)
    }

    pub(super) fn parse_command(line: &[u8]) -> anyhow::Result<(Method, Uri)> {
        let mut parts = line.split(|b| *b == b' ');
        let line = str::from_utf8(line)?;

        let method = parts
            .next()
            .with_context(|| format!("no method in header {line}"))?;
        let method = Method::from_bytes(method)
            .with_context(|| format!("Unrecognized method: {}", String::from_utf8_lossy(method)))?;

        let uri = parts
            .next()
            .with_context(|| format!("no path in HTTP header {line}"))?;
        let uri = uri.try_into()?;

        let protocol = parts
            .next()
            .with_context(|| format!("no protocol in HTTP header {line}"))?;
        if protocol != b"HTTP/1.1\r\n" {
            anyhow::bail!("unsupported HTTP protocol in header {line}");
        }
        Ok((method, uri))
    }

    pub(super) fn parse_header(line: &[u8]) -> anyhow::Result<(&str, &str)> {
        let line = str::from_utf8(line)?;
        let (name, value) = line
            .split_once(|b| b == ':')
            .with_context(|| format!("Invalid header: {line}"))?;
        let name = name.trim();
        let value = value.trim();
        Ok((name, value))
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Status(pub u32);

    impl Status {
        pub fn code(&self) -> u32 {
            self.0
        }
        pub fn reason_phrase(self) -> Option<&'static str> {
            match self.0 {
                100 => Some("Continue"),
                101 => Some("Switching Protocols"),
                200 => Some("OK"),
                201 => Some("Created"),
                202 => Some("Accepted"),
                203 => Some("Non-Authoritative Information"),
                204 => Some("No Content"),
                205 => Some("Reset Content"),
                206 => Some("Partial Content"),
                300 => Some("Multiple Choices"),
                301 => Some("Moved Permanently"),
                302 => Some("Found"),
                303 => Some("See Other"),
                304 => Some("Not Modified"),
                305 => Some("Use Proxy"),
                307 => Some("Temporary Redirect"),
                400 => Some("Bad Request"),
                401 => Some("Unauthorized"),
                402 => Some("Payment Required"),
                403 => Some("Forbidden"),
                404 => Some("Not Found"),
                405 => Some("Method Not Allowed"),
                406 => Some("Not Acceptable"),
                407 => Some("Proxy Authentication Required"),
                408 => Some("Request Time-out"),
                409 => Some("Conflict"),
                410 => Some("Gone"),
                411 => Some("Length Required"),
                412 => Some("Precondition Failed"),
                413 => Some("Request Entity Too Large"),
                414 => Some("Request-URI Too Large"),
                415 => Some("Unsupported Media Type"),
                416 => Some("Requested range not satisfiable"),
                417 => Some("Expectation Failed"),
                500 => Some("Internal Server Error"),
                501 => Some("Not Implemented"),
                502 => Some("Bad Gateway"),
                503 => Some("Service Unavailable"),
                504 => Some("Gateway Time-out"),
                505 => Some("HTTP Version not supported"),
                _ => None,
            }
        }
    }

    impl From<u32> for Status {
        fn from(value: u32) -> Self {
            Status(value)
        }
    }

    #[derive(Debug)]
    pub struct Response<'a> {
        status: Option<Status>,
        headers: Vec<(String, String)>,
        body: &'a [u8],
    }

    impl Response<'_> {
        pub fn builder() -> ResponseBuilder {
            Default::default()
        }

        pub async fn write<W>(self, writer: W) -> anyhow::Result<()>
        where
            W: io::AsyncWrite,
        {
            use io::AsyncWriteExt;

            let Self {
                status,
                headers,
                body,
            } = self;

            let status = status.context("Status code is missing")?;
            let reason_phrase = status.reason_phrase().unwrap_or("??");

            let mut has_date = false;
            let mut has_server = false;
            let mut has_content_length = false;
            for (name, _) in headers.iter() {
                if name.eq_ignore_ascii_case("date") {
                    has_date = true;
                }
                if name.eq_ignore_ascii_case("server") {
                    has_server = true;
                }
                if name.eq_ignore_ascii_case("content-length") {
                    has_content_length = true;
                }
            }

            tokio::pin!(writer);
            writer
                .write(format!("HTTP/1.1 {} {reason_phrase}\r\n", status.code()).as_bytes())
                .await?;
            if !has_date {
                writer
                    .write(format!("Date: {}\r\n", Utc::now().to_rfc2822()).as_bytes())
                    .await?;
            }
            if !has_server {
                writer.write(b"Server: fsync::http::server\r\n").await?;
            }
            if !has_content_length {
                writer
                    .write(format!("Content-Length: {}\r\n", body.len()).as_bytes())
                    .await?;
            }
            for (name, value) in headers.iter() {
                writer
                    .write(format!("{name}: {value}\r\n").as_bytes())
                    .await?;
            }
            writer.write(b"\r\n").await?;
            writer.write(&body).await?;
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    pub struct ResponseBuilder {
        status: Option<Status>,
        headers: Vec<(String, String)>,
    }

    impl ResponseBuilder {
        pub fn status<S>(self, status: S) -> Self
        where
            S: Into<Status>,
        {
            Self {
                status: Some(status.into()),
                ..self
            }
        }

        pub fn header(self, name: String, value: String) -> Self {
            let mut headers = self.headers;
            headers.push((name, value));
            Self { headers, ..self }
        }

        pub fn body(self, body: &[u8]) -> Response {
            Response {
                status: self.status,
                headers: self.headers,
                body,
            }
        }
    }
}

mod util {
    use tokio::io::{self, AsyncReadExt};

    /// Read from reader until either pattern or EOF is found.
    /// Pattern is included in the buffer.
    pub(super) async fn read_until_pattern<R>(
        reader: R,
        pattern: &[u8],
        buf: &mut Vec<u8>,
    ) -> anyhow::Result<usize>
    where
        R: io::AsyncBufRead,
    {
        use io::AsyncBufReadExt;

        debug_assert!(pattern.len() > 0);
        tokio::pin!(reader);
        let mut bb: [u8; 1] = [0];
        let mut len = 0;
        'outer: loop {
            let sz = reader.read_until(pattern[0], buf).await?;
            if sz == 0 {
                break;
            }
            len += sz;
            for c in pattern[1..].iter() {
                let sz = reader.read(&mut bb[..]).await?;
                if sz == 0 {
                    break 'outer;
                }
                len += sz;
                buf.push(bb[0]);
                if bb[0] != *c {
                    continue 'outer;
                }
            }
            break;
        }
        Ok(len)
    }
}

#[cfg(test)]
mod tests {
    use http::Method;

    use super::{server::*, util::*};

    const TEST_REQ: &str = concat!(
        "GET /some/path HTTP/1.1\r\n",
        "User-Agent: fsyncd/13.0\r\n",
        "Content-Length: 123456789\r\n",
        "\r\n",
        "Request Body",
    );

    #[tokio::test]
    async fn test_read_until_pattern() -> anyhow::Result<()> {
        let expected: &[&[u8]] = &[
            b"GET /some/path HTTP/1.1\r\n",
            b"User-Agent: fsyncd/13.0\r\n",
            b"Content-Length: 12\r\n",
            b"\r\n",
            b"Request Body",
        ];

        let mut cursor = std::io::Cursor::new(TEST_REQ.as_bytes());
        let mut buf = Vec::new();

        for &exp in expected.iter() {
            let res = read_until_pattern(&mut cursor, b"\r\n", &mut buf).await?;
            assert_eq!(res, exp.len());
            assert_eq!(buf.as_slice(), exp);
            buf.clear();
        }

        Ok(())
    }

    #[test]
    fn test_parse_command() -> anyhow::Result<()> {
        let (method, path) = parse_command(b"GET /some/path HTTP/1.1\r\n")?;
        assert_eq!(method, Method::GET);
        assert_eq!(path, "/some/path");
        Ok(())
    }

    #[test]
    fn test_parse_header() -> anyhow::Result<()> {
        let (name, value) = parse_header(b"Content-Length: 12\r\n")?;
        assert_eq!(name, "Content-Length");
        assert_eq!(value, "123456789");
        assert!(parse_header(b"Content-Length; 12\r\n").is_err());
        Ok(())
    }

    #[tokio::test]
    async fn test_parse_request() -> anyhow::Result<()> {
        let req = parse_request(TEST_REQ.as_bytes()).await?;
        assert_eq!(req.method(), Method::GET);
        assert_eq!(req.uri(), "/some/path");
        assert_eq!(req.headers().get("User-Agent").unwrap(), &"fsyncd/13.0");
        assert_eq!(req.headers().get("Content-Length").unwrap(), &"12");
        Ok(())
    }
}
