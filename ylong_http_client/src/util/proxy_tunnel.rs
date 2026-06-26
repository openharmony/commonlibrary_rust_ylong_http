// Copyright (c) 2026 Huawei Device Co., Ltd.
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! CONNECT tunnel helpers shared by proxy transports.

use std::error;
use std::fmt::{Debug, Display, Formatter};
use std::io::{Error, ErrorKind, Write};

use crate::runtime::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const DEFAULT_CONNECT_BUF_SIZE: usize = 8192;

/// Establishes an HTTP CONNECT tunnel over an arbitrary asynchronous stream.
pub(crate) async fn connect<S>(
    mut stream: S,
    host: &str,
    port: u16,
    basic_auth: Option<String>,
) -> Result<S, Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let request = connect_request(host, port, basic_auth)?;
    stream.write_all(&request).await?;
    read_connect_response(&mut stream, DEFAULT_CONNECT_BUF_SIZE).await?;
    Ok(stream)
}

fn connect_request(host: &str, port: u16, basic_auth: Option<String>) -> Result<Vec<u8>, Error> {
    let mut req = Vec::new();
    let authority = format_authority(host, port);

    write!(
        &mut req,
        "CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n"
    )?;

    if let Some(value) = basic_auth {
        write!(&mut req, "Proxy-Authorization: Basic {value}\r\n")?;
    }

    write!(&mut req, "\r\n")?;
    Ok(req)
}

fn format_authority(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

async fn read_connect_response<S>(stream: &mut S, max_header_size: usize) -> Result<(), Error>
where
    S: AsyncRead + Unpin,
{
    let mut buf = vec![0; max_header_size];
    let mut pos = 0;

    loop {
        if pos == max_header_size {
            return Err(other_io_error(ConnectTunnelError::ProxyHeadersTooLong));
        }

        let n = stream.read(&mut buf[pos..]).await?;
        if n == 0 {
            return Err(other_io_error(ConnectTunnelError::UnexpectedEof));
        }
        pos += n;

        if let Some(header_end) = find_header_end(&buf[..pos]) {
            return parse_connect_response(&buf[..header_end]);
        }
    }
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|idx| idx + 4)
}

fn parse_connect_response(buf: &[u8]) -> Result<(), Error> {
    let status_line = buf
        .split(|byte| *byte == b'\n')
        .next()
        .ok_or_else(|| other_io_error(ConnectTunnelError::InvalidResponse))?;
    let status_line = if status_line.ends_with(b"\r") {
        &status_line[..status_line.len() - 1]
    } else {
        status_line
    };

    if !status_line.starts_with(b"HTTP/") {
        return Err(other_io_error(ConnectTunnelError::InvalidResponse));
    }

    let mut parts = status_line.split(|byte| *byte == b' ');
    let _version = parts.next();
    let code = parts
        .find(|part| !part.is_empty())
        .ok_or_else(|| other_io_error(ConnectTunnelError::InvalidResponse))?;

    match code {
        b"200" => Ok(()),
        b"407" => Err(other_io_error(
            ConnectTunnelError::ProxyAuthenticationRequired,
        )),
        _ => Err(other_io_error(ConnectTunnelError::Unsuccessful)),
    }
}

fn other_io_error(err: ConnectTunnelError) -> Error {
    Error::new(ErrorKind::Other, err)
}

enum ConnectTunnelError {
    ProxyHeadersTooLong,
    ProxyAuthenticationRequired,
    UnexpectedEof,
    InvalidResponse,
    Unsuccessful,
}

impl Debug for ConnectTunnelError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProxyHeadersTooLong => f.write_str("proxy headers too long for tunnel"),
            Self::ProxyAuthenticationRequired => f.write_str("proxy authentication required"),
            Self::UnexpectedEof => f.write_str("unexpected EOF from proxy"),
            Self::InvalidResponse => f.write_str("invalid proxy tunnel response"),
            Self::Unsuccessful => f.write_str("unsuccessful tunnel"),
        }
    }
}

impl Display for ConnectTunnelError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(self, f)
    }
}

impl error::Error for ConnectTunnelError {}

#[cfg(test)]
mod tests {
    use super::parse_connect_response;

    #[test]
    fn parse_success() {
        assert!(parse_connect_response(b"HTTP/1.1 200 Connection Established\r\n\r\n").is_ok());
        assert!(parse_connect_response(b"HTTP/1.0 200 OK\r\nProxy-Agent: x\r\n\r\n").is_ok());
    }

    #[test]
    fn parse_auth_required() {
        let err = parse_connect_response(b"HTTP/1.1 407 Proxy Authentication Required\r\n\r\n")
            .expect_err("407 should fail");
        assert_eq!(err.to_string(), "proxy authentication required");
    }

    #[test]
    fn parse_failure() {
        let err = parse_connect_response(b"HTTP/1.1 502 Bad Gateway\r\n\r\n")
            .expect_err("502 should fail");
        assert_eq!(err.to_string(), "unsuccessful tunnel");
    }
}
