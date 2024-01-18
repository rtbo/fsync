use std::str::{self};

use anyhow::Context;
use chrono::Utc;
use http::{HeaderValue, Method, Request, Uri};
use tokio::io;
use util::read_until_pattern;

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

pub async fn write_response<W, B>(resp: http::Response<B>, writer: W) -> anyhow::Result<()>
where
    W: io::AsyncWrite,
    B: AsRef<[u8]>,
{
    use io::AsyncWriteExt;

    let (parts, body) = resp.into_parts();

    let has_body = !body.as_ref().is_empty();

    let has_date = parts.headers.contains_key("date");
    let has_server = parts.headers.contains_key("server");
    let has_content_length = parts.headers.contains_key("content-length");

    tokio::pin!(writer);
    writer
        .write(format!("{:?} {}\r\n", parts.version, parts.status).as_bytes())
        .await?;
    if !has_date {
        writer
            .write(format!("Date: {}\r\n", Utc::now().to_rfc2822()).as_bytes())
            .await?;
    }
    if !has_server {
        writer.write(b"Server: fsync::http::server\r\n").await?;
    }
    if has_body && !has_content_length {
        writer
            .write(format!("Content-Length: {}\r\n", body.as_ref().len()).as_bytes())
            .await?;
    }
    for (name, value) in parts.headers.iter() {
        writer.write(format!("{name}: ").as_bytes()).await?;
        writer.write(value.as_bytes()).await?;
        writer.write(b"\r\n").await?;
    }
    writer.write(b"\r\n").await?;
    if has_body {
        writer.write(body.as_ref()).await?;
    }
    writer.flush().await?;
    Ok(())
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

    use super::*;

    const TEST_REQ: &str = concat!(
        "GET /some/path HTTP/1.1\r\n",
        "User-Agent: fsyncd/13.0\r\n",
        "Content-Length: 12\r\n",
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
        assert_eq!(value, "12");
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
