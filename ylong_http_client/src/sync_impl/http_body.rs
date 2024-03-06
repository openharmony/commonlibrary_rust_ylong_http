// Copyright (c) 2023 Huawei Device Co., Ltd.
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

use std::io::{Cursor, Read};

use ylong_http::body::{ChunkBodyDecoder, ChunkState, TextBodyDecoder};
use ylong_http::headers::Headers;

use super::Body;
use crate::error::{ErrorKind, HttpClientError};
use crate::sync_impl::conn::StreamData;

/// `HttpBody` is the body part of the `Response` returned by `Client::request`.
/// `HttpBody` implements `Body` trait, so users can call related methods to get
/// body data.
///
/// # Examples
///
/// ```no_run
/// use ylong_http_client::sync_impl::{Body, Client, EmptyBody, HttpBody, Request};
///
/// let mut client = Client::new();
///
/// // `HttpBody` is the body part of `response`.
/// let mut response = client.request(Request::new(EmptyBody)).unwrap();
///
/// // Users can use `Body::data` to get body data.
/// let mut buf = [0u8; 1024];
/// loop {
///     let size = response.body_mut().data(&mut buf).unwrap();
///     if size == 0 {
///         break;
///     }
///     let _data = &buf[..size];
///     // Deals with the data.
/// }
/// ```
pub struct HttpBody {
    kind: Kind,
}

type BoxStreamData = Box<dyn StreamData>;

impl HttpBody {
    pub(crate) fn empty() -> Self {
        Self { kind: Kind::Empty }
    }

    pub(crate) fn text(len: usize, pre: &[u8], io: BoxStreamData) -> Self {
        Self {
            kind: Kind::Text(Text::new(len, pre, io)),
        }
    }

    pub(crate) fn chunk(pre: &[u8], io: BoxStreamData, is_trailer: bool) -> Self {
        Self {
            kind: Kind::Chunk(Chunk::new(pre, io, is_trailer)),
        }
    }
}

// TODO: `TextBodyDecoder` implementation and `ChunkBodyDecoder` implementation.
enum Kind {
    Empty,
    Text(Text),
    Chunk(Chunk),
}

struct Text {
    decoder: TextBodyDecoder,
    pre: Option<Cursor<Vec<u8>>>,
    io: Option<BoxStreamData>,
}

impl Text {
    pub(crate) fn new(len: usize, pre: &[u8], io: BoxStreamData) -> Self {
        Self {
            decoder: TextBodyDecoder::new(len),
            pre: (!pre.is_empty()).then_some(Cursor::new(pre.to_vec())),
            io: Some(io),
        }
    }
}

impl Body for HttpBody {
    type Error = HttpClientError;

    fn data(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }

        match self.kind {
            Kind::Empty => Ok(0),
            Kind::Text(ref mut text) => text.data(buf),
            Kind::Chunk(ref mut chunk) => chunk.data(buf),
        }
    }

    fn trailer(&mut self) -> Result<Option<Headers>, Self::Error> {
        match self.kind {
            Kind::Chunk(ref mut chunk) => chunk.decoder.get_trailer().map_err(|_| {
                HttpClientError::from_str(ErrorKind::BodyDecode, "Get trailer failed")
            }),
            _ => Ok(None),
        }
    }
}

impl Text {
    fn data(&mut self, buf: &mut [u8]) -> Result<usize, HttpClientError> {
        if buf.is_empty() {
            return Ok(0);
        }

        let mut read = 0;

        if let Some(pre) = self.pre.as_mut() {
            // Here cursor read never failed.
            let this_read = pre.read(buf).unwrap();
            if this_read == 0 {
                self.pre = None;
            } else {
                read += this_read;
                let (text, rem) = self.decoder.decode(&buf[..read]);

                match (text.is_complete(), rem.is_empty()) {
                    (true, false) => {
                        if let Some(io) = self.io.take() {
                            io.shutdown();
                        };
                        return Err(HttpClientError::from_str(ErrorKind::BodyDecode, "Not Eof"));
                    }
                    (true, true) => {
                        self.io = None;
                        return Ok(read);
                    }
                    _ => {}
                }
            }
        }

        if !buf[read..].is_empty() {
            if let Some(mut io) = self.io.take() {
                match io.read(&mut buf[read..]) {
                    // Disconnected.
                    Ok(0) => {
                        io.shutdown();
                        return Err(HttpClientError::from_str(
                            ErrorKind::BodyDecode,
                            "Response Body Incomplete",
                        ));
                    }
                    Ok(filled) => {
                        let (text, rem) = self.decoder.decode(&buf[read..read + filled]);
                        read += filled;
                        // Contains redundant `rem`, return error.
                        match (text.is_complete(), rem.is_empty()) {
                            (true, false) => {
                                io.shutdown();
                                return Err(HttpClientError::from_str(
                                    ErrorKind::BodyDecode,
                                    "Not Eof",
                                ));
                            }
                            (true, true) => return Ok(read),
                            _ => {}
                        }
                        self.io = Some(io);
                    }
                    Err(e) => return Err(HttpClientError::from_error(ErrorKind::BodyTransfer, e)),
                }
            }
        }
        Ok(read)
    }
}

struct Chunk {
    decoder: ChunkBodyDecoder,
    pre: Option<Cursor<Vec<u8>>>,
    io: Option<BoxStreamData>,
}

impl Chunk {
    pub(crate) fn new(pre: &[u8], io: BoxStreamData, is_trailer: bool) -> Self {
        Self {
            decoder: ChunkBodyDecoder::new().contains_trailer(is_trailer),
            pre: (!pre.is_empty()).then_some(Cursor::new(pre.to_vec())),
            io: Some(io),
        }
    }
}

impl Chunk {
    fn data(&mut self, buf: &mut [u8]) -> Result<usize, HttpClientError> {
        if buf.is_empty() {
            return Ok(0);
        }

        let mut read = 0;

        while let Some(pre) = self.pre.as_mut() {
            // Here cursor read never failed.
            let size = pre.read(&mut buf[read..]).unwrap();

            if size == 0 {
                self.pre = None;
            }

            let (size, flag) = self.merge_chunks(&mut buf[read..read + size])?;
            read += size;

            if flag {
                // Return if we find a 0-sized chunk.
                self.io = None;
                return Ok(read);
            } else if read != 0 {
                // Return if we get some data.
                return Ok(read);
            }
        }

        // Here `read` must be 0.
        while let Some(mut io) = self.io.take() {
            match io.read(&mut buf[read..]) {
                Ok(filled) => {
                    if filled == 0 {
                        io.shutdown();
                        return Err(HttpClientError::from_str(
                            ErrorKind::BodyDecode,
                            "Response Body Incomplete",
                        ));
                    }
                    let (size, flag) = self.merge_chunks(&mut buf[read..read + filled])?;
                    read += size;
                    if flag {
                        // Return if we find a 0-sized chunk.
                        // Return if we get some data.
                        return Ok(read);
                    }
                    self.io = Some(io);
                    if read != 0 {
                        return Ok(read);
                    }
                }
                Err(e) => return Err(HttpClientError::from_error(ErrorKind::BodyTransfer, e)),
            }
        }
        Ok(read)
    }

    fn merge_chunks(&mut self, buf: &mut [u8]) -> Result<(usize, bool), HttpClientError> {
        // Here we need to merge the chunks into one data block and return.
        // The data arrangement in buf is as follows:
        //
        // data in buf:
        // +------+------+------+------+------+------+------+
        // | data | len  | data | len  |  ... | data |  len |
        // +------+------+------+------+------+------+------+
        //
        // We need to merge these data blocks into one block:
        //
        // after merge:
        // +---------------------------+
        // |            data           |
        // +---------------------------+

        let (chunks, junk) = self
            .decoder
            .decode(buf)
            .map_err(|e| HttpClientError::from_error(ErrorKind::BodyDecode, e))?;

        let mut finished = false;
        let mut ptrs = Vec::new();
        for chunk in chunks.into_iter() {
            if chunk.trailer().is_some() {
                if chunk.state() == &ChunkState::Finish {
                    finished = true;
                }
            } else {
                if chunk.size() == 0 && chunk.state() != &ChunkState::MetaSize {
                    finished = true;
                    break;
                }
                let data = chunk.data();
                ptrs.push((data.as_ptr(), data.len()))
            }
        }

        if finished && !junk.is_empty() {
            return Err(HttpClientError::from_str(
                ErrorKind::BodyDecode,
                "Invalid Chunk Body",
            ));
        }

        let start = buf.as_ptr();

        let mut idx = 0;
        for (ptr, len) in ptrs.into_iter() {
            let st = ptr as usize - start as usize;
            let ed = st + len;
            buf.copy_within(st..ed, idx);
            idx += len;
        }
        Ok((idx, finished))
    }
}

#[cfg(test)]
mod ut_syn_http_body {
    use crate::sync_impl::{Body, HttpBody};

    /// UT test cases for `HttpBody::empty`.
    ///
    /// # Brief
    /// 1. Creates a `HttpBody` by calling `HttpBody::empty`.
    /// 2. Calls `data` method.
    /// 3. Checks if the result is correct.
    #[test]
    fn ut_http_body_empty() {
        let mut body = HttpBody::empty();
        let mut buf = [];
        let data = body.data(&mut buf);
        assert!(data.is_ok());
        assert_eq!(data.unwrap(), 0);
    }
}
