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

use core::convert::Infallible;
use core::ops::{Deref, DerefMut};
use core::pin::Pin;
use core::str::from_utf8_unchecked;
use core::task::{Context, Poll};
use std::any::Any;
use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};
use std::future::Future;
use std::io::{Error, Read};

use super::origin::{FromAsyncReader, FromBytes, FromReader};
use super::{async_impl, sync_impl};
use crate::body::origin::FromAsyncBody;
use crate::error::{ErrorKind, HttpError};
use crate::headers::{Header, HeaderName, HeaderValue, Headers};
use crate::{AsyncRead, AsyncReadExt, ReadBuf};

/// A chunk body is used to encode body to send message by chunk in `HTTP/1.1`
/// format.
///
/// This chunk body encoder supports you to use the chunk encode method multiple
/// times to output the result in multiple bytes slices.
///
/// # Examples
///
/// ```
/// use ylong_http::body::sync_impl::Body;
/// use ylong_http::body::ChunkBody;
///
/// let content = "aaaaa bbbbb ccccc ddddd";
/// // Gets `ChunkBody`
/// let mut task = ChunkBody::from_bytes(content.as_bytes());
/// let mut user_slice = [0_u8; 10];
/// let mut output_vec = vec![];
///
/// // First encoding, user_slice is filled.
/// let size = task.data(user_slice.as_mut_slice()).unwrap();
/// assert_eq!(&user_slice[..size], "17\r\naaaaa ".as_bytes());
/// output_vec.extend_from_slice(user_slice.as_mut_slice());
///
/// // Second encoding, user_slice is filled.
/// let size = task.data(user_slice.as_mut_slice()).unwrap();
/// assert_eq!(&user_slice[..size], "bbbbb cccc".as_bytes());
/// output_vec.extend_from_slice(user_slice.as_mut_slice());
///
/// // Third encoding, user_slice is filled.
/// let size = task.data(user_slice.as_mut_slice()).unwrap();
/// assert_eq!(&user_slice[..size], "c ddddd\r\n0".as_bytes());
/// output_vec.extend_from_slice(user_slice.as_mut_slice());
///
/// // Fourth encoding, part of user_slice is filled, this indicates that encoding has ended.
/// let size = task.data(user_slice.as_mut_slice()).unwrap();
/// assert_eq!(&user_slice[..size], "\r\n\r\n".as_bytes());
/// output_vec.extend_from_slice(&user_slice[..size]);
///
/// // We can assemble temporary data into a complete data.
/// let result = "17\r\naaaaa bbbbb ccccc ddddd\r\n0\r\n\r\n";
/// assert_eq!(output_vec.as_slice(), result.as_bytes());
/// ```
pub struct ChunkBody<T> {
    from: T,
    trailer_value: Vec<u8>,
    chunk_data: ChunkData,
    data_status: DataState,
    encode_status: EncodeStatus,
    trailer: EncodeTrailer,
}

const CHUNK_SIZE: usize = 1024;

struct StatusVar {
    cnt: usize,
    data_status: DataState,
}

// Data encoding status
enum DataState {
    // Data encode is processing
    Partial,
    // Data encode is completed
    Complete,
    // Data encode is finished and return result
    Finish,
}

// Component encoding status
enum TokenStatus<T, E> {
    // The current component is completely encoded.
    Complete(T),
    // The current component is partially encoded.
    Partial(E),
}

type Token<T> = TokenStatus<usize, T>;

impl<'a> ChunkBody<FromBytes<'a>> {
    /// Creates a new `ChunkBody` by `bytes`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::ChunkBody;
    ///
    /// let task = ChunkBody::from_bytes("".as_bytes());
    /// ```
    pub fn from_bytes(bytes: &'a [u8]) -> Self {
        ChunkBody {
            from: FromBytes::new(bytes),
            trailer_value: vec![],
            chunk_data: ChunkData::new(vec![]),
            data_status: DataState::Partial,
            encode_status: EncodeStatus::new(),
            trailer: EncodeTrailer::new(),
        }
    }

    fn chunk_encode(&mut self, src: &[u8], dst: &mut [u8]) -> usize {
        self.encode_status.chunk_last = self.chunk_data.chunk_last;
        let (output_size, var) = self.encode_status.encode(src, dst);

        if let Some(v) = var {
            self.chunk_data.chunk_count = v.cnt;
            self.data_status = v.data_status;
        }
        output_size
    }
}

impl<T: Read> ChunkBody<FromReader<T>> {
    /// Creates a new `ChunkBody` by `reader`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::ChunkBody;
    ///
    /// let task = ChunkBody::from_reader("".as_bytes());
    /// ```
    pub fn from_reader(reader: T) -> Self {
        ChunkBody {
            from: FromReader::new(reader),
            trailer_value: vec![],
            chunk_data: ChunkData::new(vec![0; CHUNK_SIZE]),
            data_status: DataState::Partial,
            encode_status: EncodeStatus::new(),
            trailer: EncodeTrailer::new(),
        }
    }

    fn chunk_encode(&mut self, dst: &mut [u8]) -> usize {
        self.chunk_encode_reader(dst)
    }
}

impl<T: AsyncRead + Unpin + Send + Sync> ChunkBody<FromAsyncReader<T>> {
    /// Creates a new `ChunkBody` by `async reader`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::ChunkBody;
    ///
    /// let task = ChunkBody::from_async_reader("".as_bytes());
    /// ```
    pub fn from_async_reader(reader: T) -> Self {
        ChunkBody {
            from: FromAsyncReader::new(reader),
            trailer_value: vec![],
            chunk_data: ChunkData::new(vec![0; CHUNK_SIZE]),
            data_status: DataState::Partial,
            encode_status: EncodeStatus::new(),
            trailer: EncodeTrailer::new(),
        }
    }

    fn chunk_encode(&mut self, dst: &mut [u8]) -> usize {
        self.chunk_encode_reader(dst)
    }
}

impl<'a> sync_impl::Body for ChunkBody<FromBytes<'a>> {
    type Error = Infallible;

    fn data(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let mut count = 0;
        while count != buf.len() {
            let encode_size = match self.data_status {
                DataState::Partial => self.bytes_encode(&mut buf[count..]),
                DataState::Complete => self.trailer_encode(&mut buf[count..]),
                DataState::Finish => return Ok(count),
            };
            count += encode_size;
        }
        Ok(buf.len())
    }
}

impl<T: Read> sync_impl::Body for ChunkBody<FromReader<T>> {
    type Error = Error;

    fn data(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let mut count = 0;
        while count != buf.len() {
            let encode_size = match self.data_status {
                DataState::Partial => {
                    if !self.encode_status.get_flag() {
                        self.encode_status.set_flag(true);
                        self.encode_status.set_chunk_idx(0);
                        self.chunk_data.chunk_last =
                            (*self.from).read(&mut self.chunk_data.chunk_buf).unwrap();
                    }
                    self.chunk_encode(&mut buf[count..])
                }
                DataState::Complete => self.trailer_encode(&mut buf[count..]),
                DataState::Finish => {
                    return Ok(count);
                }
            };
            count += encode_size;
        }
        Ok(buf.len())
    }
}

impl<'c> async_impl::Body for ChunkBody<FromBytes<'c>> {
    type Error = Error;

    fn poll_data(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, Self::Error>> {
        let mut count = 0;
        while count != buf.len() {
            let encode_size: Poll<usize> = match self.data_status {
                DataState::Partial => Poll::Ready(self.bytes_encode(&mut buf[count..])),
                DataState::Complete => Poll::Ready(self.trailer_encode(&mut buf[count..])),
                DataState::Finish => return Poll::Ready(Ok(count)),
            };
            match encode_size {
                Poll::Ready(size) => {
                    count += size;
                }
                Poll::Pending => return Poll::Pending,
            }
        }
        Poll::Ready(Ok(buf.len()))
    }
}

impl<T: AsyncRead + Unpin + Send + Sync> async_impl::Body for ChunkBody<FromAsyncReader<T>> {
    type Error = Error;

    fn poll_data(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, Self::Error>> {
        let chunk_body = self.get_mut();
        let mut count = 0;
        while count != buf.len() {
            let encode_size = match chunk_body.data_status {
                DataState::Partial => {
                    if !chunk_body.encode_status.get_flag() {
                        let mut read_buf = ReadBuf::new(&mut chunk_body.chunk_data.chunk_buf);

                        match Pin::new(&mut *chunk_body.from).poll_read(_cx, &mut read_buf) {
                            Poll::Ready(Ok(())) => {
                                let size = read_buf.filled().len();
                                chunk_body.encode_status.set_flag(true);
                                // chunk idx reset zero
                                chunk_body.encode_status.set_chunk_idx(0);
                                chunk_body.chunk_data.chunk_last = size;
                                let data_size = chunk_body.chunk_encode(&mut buf[count..]);
                                Poll::Ready(data_size)
                            }
                            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                            Poll::Pending => Poll::Pending,
                        }
                    } else {
                        Poll::Ready(chunk_body.chunk_encode(&mut buf[count..]))
                    }
                }
                DataState::Complete => Poll::Ready(chunk_body.trailer_encode(&mut buf[count..])),
                DataState::Finish => {
                    return Poll::Ready(Ok(count));
                }
            };

            match encode_size {
                Poll::Ready(size) => {
                    count += size;
                }
                Poll::Pending => {
                    if count != 0 {
                        return Poll::Ready(Ok(count));
                    }
                    return Poll::Pending;
                }
            }
        }
        Poll::Ready(Ok(buf.len()))
    }
}

impl<'a> ChunkBody<FromBytes<'a>> {
    fn bytes_encode(&mut self, dst: &mut [u8]) -> usize {
        if !self.encode_status.get_flag() {
            self.encode_status.set_flag(true);
            self.encode_status.set_chunk_idx(0);
            let data_left = self.from.len() - self.chunk_data.chunk_count * CHUNK_SIZE;
            self.chunk_data.chunk_last = if data_left < CHUNK_SIZE {
                data_left
            } else {
                CHUNK_SIZE
            };
        }
        let src = &self.from[self.chunk_data.chunk_count * CHUNK_SIZE
            ..(self.chunk_data.chunk_count * CHUNK_SIZE + self.chunk_data.chunk_last)];
        self.chunk_encode(src, dst)
    }
}

impl<T> ChunkBody<T> {
    /// Creates a new `Trailer` by `set_trailer`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::ChunkBody;
    /// use ylong_http::headers::Headers;
    ///
    /// let mut headers = Headers::new();
    /// let _ = headers.insert("accept", "text/html");
    /// let mut task = ChunkBody::from_bytes("".as_bytes()).set_trailer(headers);
    /// ```
    pub fn set_trailer(mut self, trailer_headers: Headers) -> Self {
        let mut trailer_vec = vec![];
        for (name, value) in trailer_headers.into_iter() {
            // Operate on each `HeaderName` and `HeaderValue` pair.
            trailer_vec.extend_from_slice(name.as_bytes());
            trailer_vec.extend_from_slice(b":");
            // to_string will not return err, so can use unwrap directly
            trailer_vec.extend_from_slice(value.to_string().unwrap().as_bytes());
            trailer_vec.extend_from_slice(b"\r\n");
        }
        self.trailer_value = trailer_vec;
        self
    }
    fn chunk_encode_reader(&mut self, dst: &mut [u8]) -> usize {
        self.encode_status.chunk_last = self.chunk_data.chunk_last;
        let (output_size, var) = self.encode_status.encode(
            &self.chunk_data.chunk_buf[..self.chunk_data.chunk_last],
            dst,
        );
        if let Some(v) = var {
            self.chunk_data.chunk_count = v.cnt;
            self.data_status = v.data_status;
        }
        output_size
    }

    fn trailer_encode(&mut self, dst: &mut [u8]) -> usize {
        let mut src = b"0\r\n".to_vec();
        if self.trailer_value.is_empty() {
            src.extend_from_slice(b"\r\n");
        } else {
            src.extend_from_slice(self.trailer_value.as_slice());
            src.extend_from_slice(b"\r\n");
        };
        match self.trailer.encode(src.as_slice(), dst) {
            TokenStatus::Complete(output_size) => {
                self.data_status = DataState::Finish;
                output_size
            }
            TokenStatus::Partial(output_size) => output_size,
        }
    }
}

struct ChunkData {
    chunk_buf: Vec<u8>,
    chunk_count: usize,
    chunk_last: usize,
}

impl ChunkData {
    fn new(buf: Vec<u8>) -> Self {
        ChunkData {
            chunk_buf: buf,
            chunk_count: 0,
            chunk_last: 0,
        }
    }
}

struct EncodeStatus {
    chunk_size: usize,
    chunk_last: usize,
    chunk_idx: usize,
    read_flag: bool,
    src_idx: usize,
    chunk_status: ChunkState,
    meta_crlf: EncodeCrlf,
    data_crlf: EncodeCrlf,
    finish_crlf: EncodeCrlf,
    hex: EncodeHex,
    hex_last: EncodeHex,
}

impl EncodeStatus {
    fn new() -> Self {
        EncodeStatus {
            chunk_size: CHUNK_SIZE,
            chunk_last: 0,
            chunk_idx: 0,
            read_flag: false,
            src_idx: 0,
            chunk_status: ChunkState::MetaSize,
            meta_crlf: EncodeCrlf::new(),
            data_crlf: EncodeCrlf::new(),
            finish_crlf: EncodeCrlf::new(),
            hex: EncodeHex::new(format!("{CHUNK_SIZE:x}")),
            hex_last: EncodeHex::new("".to_string()),
        }
    }

    fn encode(&mut self, src: &[u8], dst: &mut [u8]) -> (usize, Option<StatusVar>) {
        match self.chunk_status {
            ChunkState::MetaSize => (self.meta_size_encode(dst), None),
            ChunkState::MetaExt => (0, None),
            ChunkState::MetaCrlf => (self.meta_crlf_encode(dst), None),
            ChunkState::Data => {
                if self.chunk_last != CHUNK_SIZE {
                    self.tail_encode(src, dst)
                } else {
                    self.data_encode(src, dst)
                }
            }
            ChunkState::DataCrlf => (self.data_crlf_encode(dst), None),
            ChunkState::Finish => self.finish_encode(dst),
        }
    }

    fn meta_size_encode(&mut self, dst: &mut [u8]) -> usize {
        if self.chunk_last == CHUNK_SIZE {
            match self.hex.encode(dst) {
                TokenStatus::Complete(output_size) => {
                    self.chunk_status = ChunkState::MetaCrlf;
                    self.hex.src_idx = 0;
                    output_size
                }
                TokenStatus::Partial(output_size) => output_size,
            }
        } else {
            self.hex_last = EncodeHex::new(format!("{last:x}", last = self.chunk_last));
            match self.hex_last.encode(dst) {
                TokenStatus::Complete(output_size) => {
                    self.chunk_status = ChunkState::MetaCrlf;
                    self.hex_last.src_idx = 0;
                    output_size
                }
                TokenStatus::Partial(output_size) => output_size,
            }
        }
    }

    fn meta_crlf_encode(&mut self, dst: &mut [u8]) -> usize {
        match self.meta_crlf.encode(dst) {
            TokenStatus::Complete(output_size) => {
                self.chunk_status = ChunkState::Data;
                self.meta_crlf.src_idx = 0;
                output_size
            }
            TokenStatus::Partial(output_size) => output_size,
        }
    }

    fn data_crlf_encode(&mut self, dst: &mut [u8]) -> usize {
        match self.data_crlf.encode(dst) {
            TokenStatus::Complete(output_size) => {
                self.chunk_status = ChunkState::MetaSize;
                self.data_crlf.src_idx = 0;
                output_size
            }
            TokenStatus::Partial(output_size) => output_size,
        }
    }

    fn finish_encode(&mut self, dst: &mut [u8]) -> (usize, Option<StatusVar>) {
        match self.finish_crlf.encode(dst) {
            TokenStatus::Complete(output_size) => {
                self.meta_crlf.src_idx = 0;
                let var = StatusVar {
                    cnt: 0,
                    data_status: DataState::Complete,
                };
                (output_size, Some(var))
            }
            TokenStatus::Partial(output_size) => (output_size, None),
        }
    }

    fn data_encode(&mut self, src: &[u8], dst: &mut [u8]) -> (usize, Option<StatusVar>) {
        let mut task = WriteData::new(src, &mut self.chunk_idx, dst);

        match task.write() {
            TokenStatus::Complete(output_size) => {
                self.chunk_status = ChunkState::DataCrlf;
                self.read_flag = false;
                let var = StatusVar {
                    cnt: 1,
                    data_status: DataState::Partial,
                };
                (output_size, Some(var))
            }
            TokenStatus::Partial(output_size) => (output_size, None),
        }
    }

    fn tail_encode(&mut self, src: &[u8], dst: &mut [u8]) -> (usize, Option<StatusVar>) {
        let mut task = WriteData::new(src, &mut self.chunk_idx, dst);
        match task.write() {
            TokenStatus::Complete(output_size) => {
                self.chunk_status = ChunkState::Finish;
                self.read_flag = false;
                let var = StatusVar {
                    cnt: 0,
                    data_status: DataState::Partial,
                };
                (output_size, Some(var))
            }
            TokenStatus::Partial(output_size) => (output_size, None),
        }
    }

    fn get_flag(&mut self) -> bool {
        self.read_flag
    }

    fn set_flag(&mut self, flag: bool) {
        self.read_flag = flag;
    }

    fn set_chunk_idx(&mut self, num: usize) {
        self.chunk_idx = num;
    }
}

struct EncodeHex {
    inner: String,
    src_idx: usize,
}

impl EncodeHex {
    fn new(hex: String) -> Self {
        Self {
            inner: hex,
            src_idx: 0,
        }
    }

    fn encode(&mut self, buf: &mut [u8]) -> Token<usize> {
        let hex = self.inner.as_bytes();
        let mut task = WriteData::new(hex, &mut self.src_idx, buf);
        task.write()
    }
}

struct EncodeCrlf {
    src_idx: usize,
}

impl EncodeCrlf {
    fn new() -> Self {
        Self { src_idx: 0 }
    }

    fn encode(&mut self, buf: &mut [u8]) -> Token<usize> {
        let crlf = "\r\n".as_bytes();
        let mut task = WriteData::new(crlf, &mut self.src_idx, buf);
        task.write()
    }
}

struct EncodeTrailer {
    src_idx: usize,
}

impl EncodeTrailer {
    fn new() -> Self {
        Self { src_idx: 0 }
    }

    fn encode(&mut self, src: &[u8], buf: &mut [u8]) -> Token<usize> {
        let mut task = WriteData::new(src, &mut self.src_idx, buf);
        task.write()
    }
}

struct WriteData<'a> {
    src: &'a [u8],
    src_idx: &'a mut usize,
    dst: &'a mut [u8],
}

impl<'a> WriteData<'a> {
    fn new(src: &'a [u8], src_idx: &'a mut usize, dst: &'a mut [u8]) -> Self {
        WriteData { src, src_idx, dst }
    }

    fn write(&mut self) -> Token<usize> {
        let src_idx = *self.src_idx;
        let input_len = self.src.len() - src_idx;
        let output_len = self.dst.len();
        let num = std::io::Read::read(&mut &self.src[src_idx..], self.dst).unwrap();
        if output_len >= input_len {
            *self.src_idx += num;
            return TokenStatus::Complete(num);
        }
        *self.src_idx += num;
        TokenStatus::Partial(num)
    }
}

// Stage of decode chunks, The elements of the chunk-body are as follows:
// |========================================================================
// | chunked-body   = *chunk                                               |
// |                    last-chunk                                         |
// |                    trailer-section                                    |
// |                    CRLF                                               |
// |                                                                       |
// |   chunk          = chunk-size [ chunk-ext ] CRLF                      |
// |                    chunk-data CRLF                                    |
// |   chunk-size     = 1*HEXDIG                                           |
// |   last-chunk     = 1*("0") [ chunk-ext ] CRLF                         |
// |                                                                       |
// |   chunk-data     = 1*OCTET ; a sequence of chunk-size octets          |
// |                                                                       |
// |   chunk-ext      = *( BWS ";" BWS chunk-ext-name                      |
// |                       [ BWS "=" BWS chunk-ext-val ] )                 |
// |                                                                       |
// |   chunk-ext-name = token                                              |
// |   chunk-ext-val  = token / quoted-string                              |
// |========================================================================
enum Stage {
    Size,
    Extension,
    SizeEnd,
    Data,
    DataEnd,
    TrailerCrlf,
    TrailerData,
    TrailerEndCrlf,
}

/// Chunk-ext part of a chunk,
/// Currently, the `ChunkBodyDecoder` does not decode the chunk-ext part.
/// Therefore, the chunk-ext key-value pair cannot be inserted or extracted.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct ChunkExt {
    map: HashMap<String, String>,
}

impl ChunkExt {
    /// Constructor of `ChunkExt`
    pub fn new() -> Self {
        ChunkExt {
            map: HashMap::new(),
        }
    }
}

/// Decode state of the chunk buffer.
/// When chunks in the buffer end in different elements, `ChunkBodyDecoder`
/// returns different `ChunkState`, as shown in the following figure:
/// > ```trust
/// > Meta:     `chunk-size [ chunk-ext ] CRLF`
/// > Partial:  `chunk-size [ chunk-ext ] CRLF chunk-data`
/// > Complete: `chunk-size [ chunk-ext ] CRLF chunk-data CRLF`
/// > ```
#[derive(Debug, Eq, PartialEq)]
pub enum ChunkState {
    /// State of `chunk-size`
    MetaSize,
    /// State of `chunk-ext`
    MetaExt,
    /// CRLF
    MetaCrlf,
    /// State of `chunk-data`
    Data,
    /// CRLF
    DataCrlf,
    /// End
    Finish,
}

/// Decode result of the chunk buffer, contains all chunks in a buffer.
#[derive(Debug, Eq, PartialEq)]
pub struct Chunks<'a> {
    chunks: Vec<Chunk<'a>>,
}

/// An iterator of `Chunks`.
pub struct ChunksIter<'a> {
    iter: core::slice::Iter<'a, Chunk<'a>>,
}

/// An iterator that moves out of a `Chunks`.
pub struct ChunksIntoIter<'a> {
    into_iter: std::vec::IntoIter<Chunk<'a>>,
}

impl ChunksIter<'_> {
    fn new<'a>(iter: core::slice::Iter<'a, Chunk<'a>>) -> ChunksIter<'a> {
        ChunksIter { iter }
    }
}

impl<'a> Deref for ChunksIter<'a> {
    type Target = core::slice::Iter<'a, Chunk<'a>>;

    fn deref(&self) -> &Self::Target {
        &self.iter
    }
}

impl<'a> DerefMut for ChunksIter<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.iter
    }
}

impl ChunksIntoIter<'_> {
    fn new(into_iter: std::vec::IntoIter<Chunk>) -> ChunksIntoIter {
        ChunksIntoIter { into_iter }
    }
}

impl<'a> Iterator for ChunksIntoIter<'a> {
    type Item = Chunk<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.into_iter.next()
    }
}

impl<'a> Deref for ChunksIntoIter<'a> {
    type Target = std::vec::IntoIter<Chunk<'a>>;

    fn deref(&self) -> &Self::Target {
        &self.into_iter
    }
}

impl<'b> Chunks<'b> {
    /// Returns an `ChunksIter`
    pub fn iter(&self) -> ChunksIter {
        ChunksIter::new(self.chunks.iter())
    }

    fn new() -> Self {
        Chunks { chunks: vec![] }
    }

    fn push<'a: 'b>(&mut self, chunk: Chunk<'a>) {
        self.chunks.push(chunk)
    }
}

impl<'a> IntoIterator for Chunks<'a> {
    type Item = Chunk<'a>;
    type IntoIter = ChunksIntoIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        ChunksIntoIter::new(self.chunks.into_iter())
    }
}

/// Chunk instance, Indicates a chunk.
/// After a decode, the `ChunkBodyDecoder` returns a `Chunk` regardless of
/// whether a chunk is completely decoded. The decode status is recorded by the
/// `state` variable.
#[derive(Debug, Eq, PartialEq)]
pub struct Chunk<'a> {
    id: usize,
    state: ChunkState,
    size: usize,
    extension: ChunkExt,
    data: &'a [u8],
    trailer: Option<&'a [u8]>,
}

impl Chunk<'_> {
    fn set_id(&mut self, id: usize) {
        self.id = id;
    }

    fn is_complete(&self) -> bool {
        matches!(self.state, ChunkState::Finish)
    }

    /// Get the id of chunk-data.
    pub fn id(&self) -> usize {
        self.id
    }

    /// Get the immutable reference of a state.
    pub fn state(&self) -> &ChunkState {
        &self.state
    }

    /// Get the size of chunk-data,
    /// If the size part of a chunk is not completely decoded, the value of size
    /// is 0.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Get the immutable reference of chunk-ext.
    /// Currently, only one empty ChunkExt is contained.
    pub fn extension(&self) -> &ChunkExt {
        &self.extension
    }

    /// Get the chunk-data.
    /// When the state is partial, only partial data is returned.
    pub fn data(&self) -> &[u8] {
        self.data
    }
    /// Get the trailer.
    pub fn trailer(&self) -> Option<&[u8]> {
        self.trailer
    }
}

/// Chunk decoder.
/// The decoder decode only all chunks and last-chunk in chunk-body and does not
/// decode subsequent trailer-section. The decoder maintains a state saving
/// decode phase. When a chunk is not completely decoded or a decoding exception
/// occurs, the state is not reset.
pub struct ChunkBodyDecoder {
    chunk_num: usize,
    total_size: usize,
    rest_size: usize,
    hex_count: i64,
    trailer: Vec<u8>,
    cr_meet: bool,
    chunk_flag: bool,
    num_flag: bool,
    is_last_chunk: bool,
    is_chunk_trailer: bool,
    is_trailer: bool,
    is_trailer_crlf: bool,
    stage: Stage,
}

impl Default for ChunkBodyDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl ChunkBodyDecoder {
    /// Constructor of `ChunkBodyDecoder` bytes decoder.
    /// Initial stage is `Size`
    pub fn new() -> ChunkBodyDecoder {
        ChunkBodyDecoder {
            chunk_num: 0,
            total_size: 0,
            rest_size: 0,
            hex_count: 0,
            trailer: vec![],
            cr_meet: false,
            chunk_flag: false,
            num_flag: false,
            is_last_chunk: false,
            is_chunk_trailer: false,
            is_trailer: false,
            is_trailer_crlf: false,
            stage: Stage::Size,
        }
    }

    /// Initial trailer settings for check whether body contain trailer.
    pub fn contains_trailer(mut self, contain_trailer: bool) -> Self {
        self.is_trailer = contain_trailer;
        self
    }

    fn merge_trailer(&mut self, chunk: &Chunk) {
        if chunk.state() == &ChunkState::Finish || chunk.state() == &ChunkState::DataCrlf {
            self.trailer.extend_from_slice(chunk.trailer().unwrap());
            if !self.trailer.is_empty() {
                self.trailer.extend_from_slice(b"\r\n");
            }
        } else {
            self.trailer.extend_from_slice(chunk.trailer().unwrap());
        }
    }
    /// Decode interface of the chunk decoder.
    /// It transfers a u8 slice pointing to the chunk data and returns the data
    /// of a chunk and the remaining data. When the data in the u8 slice is
    /// not completely decoded for a chunk, An empty u8 slice is returned
    /// for the remaining data.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::{Chunk, ChunkBodyDecoder, ChunkExt, ChunkState};
    /// let mut decoder = ChunkBodyDecoder::new();
    /// let chunk_body_bytes = "\
    ///             5\r\n\
    ///             hello\r\n\
    ///             000; message = last\r\n\
    ///             \r\n\
    ///             "
    /// .as_bytes();
    /// let (chunks, rest) = decoder.decode(chunk_body_bytes).unwrap();
    /// assert_eq!(chunks.iter().len(), 2);
    /// let chunk = chunks.iter().next().unwrap();
    /// assert_eq!(
    ///     (
    ///         chunk.id(),
    ///         chunk.state(),
    ///         chunk.size(),
    ///         chunk.extension(),
    ///         chunk.data()
    ///     ),
    ///     (
    ///         0,
    ///         &ChunkState::Finish,
    ///         5,
    ///         &ChunkExt::new(),
    ///         "hello".as_bytes()
    ///     )
    /// );
    /// ```
    pub fn decode<'a>(&mut self, buf: &'a [u8]) -> Result<(Chunks<'a>, &'a [u8]), HttpError> {
        let mut results = Chunks::new();
        let mut remains = buf;
        loop {
            let (mut chunk, rest) = match self.stage {
                Stage::Size => self.decode_size(remains),
                Stage::Extension => self.skip_extension(remains),
                Stage::SizeEnd => self.skip_crlf(remains),
                Stage::Data => self.decode_data(remains),
                Stage::DataEnd => self.skip_last_crlf(&remains[..0], remains),
                Stage::TrailerCrlf => self.skip_trailer_crlf(remains),
                Stage::TrailerData => self.decode_trailer_data(remains),
                Stage::TrailerEndCrlf => self.skip_trailer_last_crlf(&remains[..0], remains),
            }?;

            chunk.set_id(self.chunk_num);

            if chunk.trailer.is_some() {
                self.merge_trailer(&chunk);
            }

            remains = rest;
            match (chunk.is_complete(), self.is_last_chunk) {
                (false, _) => {
                    if self.is_chunk_trailer
                        && (chunk.state == ChunkState::Data || chunk.state == ChunkState::DataCrlf)
                    {
                        results.push(chunk);
                        self.chunk_num += 1;
                        if remains.is_empty() {
                            break;
                        }
                    } else {
                        results.push(chunk);
                        break;
                    }
                }
                (true, true) => {
                    results.push(chunk);
                    self.is_last_chunk = false;
                    self.chunk_num = 0;
                    break;
                }
                (true, false) => {
                    results.push(chunk);
                    self.chunk_num += 1;
                    if remains.is_empty() {
                        break;
                    }
                }
            }
        }
        Ok((results, remains))
    }

    /// Get trailer headers.
    pub fn get_trailer(&self) -> Result<Option<Headers>, HttpError> {
        if self.trailer.is_empty() {
            return Ok(None);
        }

        let mut colon = 0;
        let mut lf = 0;
        let mut trailer_header_name = HeaderName::from_bytes(b"")?;
        let mut trailer_headers = Headers::new();
        for (i, b) in self.trailer.iter().enumerate() {
            if *b == b' ' {
                continue;
            }

            if *b == b':' && colon == 0 {
                colon = i;
                if lf == 0 {
                    let trailer_name = &self.trailer[..colon];
                    trailer_header_name = HeaderName::from_bytes(trailer_name)?;
                } else {
                    let trailer_name = &self.trailer[lf + 1..colon];
                    trailer_header_name = HeaderName::from_bytes(trailer_name)?;
                }
                continue;
            }

            if *b == b'\n' {
                if &self.trailer[i - 2..i - 1] == "\n".as_bytes() {
                    break;
                }
                lf = i;
                let mut trailer_value = &self.trailer[colon + 1..lf - 1];
                if let Some(start) = trailer_value.iter().position(|b| *b != b' ' && *b != b'\t') {
                    trailer_value = &trailer_value[start..];
                }
                if let Some(end) = trailer_value
                    .iter()
                    .rposition(|b| *b != b' ' && *b != b'\t')
                {
                    trailer_value = &trailer_value[..end + 1];
                }
                let trailer_header_value = HeaderValue::from_bytes(trailer_value)?;
                let _ = trailer_headers.insert::<HeaderName, HeaderValue>(
                    trailer_header_name.clone(),
                    trailer_header_value.clone(),
                )?;
                colon = 0;
            }
        }

        Ok(Some(trailer_headers))
    }

    fn hex_to_decimal(mut count: i64, num: i64) -> Result<i64, HttpError> {
        count = count
            .checked_mul(16)
            .ok_or_else(|| HttpError::from(ErrorKind::InvalidInput))?;
        count
            .checked_add(num)
            .ok_or_else(|| HttpError::from(ErrorKind::InvalidInput))
    }

    fn decode_size<'a>(&mut self, buf: &'a [u8]) -> Result<(Chunk<'a>, &'a [u8]), HttpError> {
        self.stage = Stage::Size;
        if buf.is_empty() {
            return Ok((
                Chunk {
                    id: 0,
                    state: ChunkState::MetaSize,
                    size: self.total_size,
                    extension: ChunkExt::new(),
                    data: &buf[..0],
                    trailer: None,
                },
                buf,
            ));
        }
        self.chunk_flag = false;
        for (i, &b) in buf.iter().enumerate() {
            match b {
                b'0' => {
                    if buf.len() <= i + 1 {
                        self.hex_count = Self::hex_to_decimal(self.hex_count, 0_i64)?;
                        continue;
                    }
                    if buf[i + 1] != b';' && buf[i + 1] != b' ' && buf[i + 1] != b'\r' {
                        self.hex_count = Self::hex_to_decimal(self.hex_count, 0_i64)?;
                        continue;
                    }
                    if self.is_trailer && !self.chunk_flag && !self.num_flag {
                        self.is_chunk_trailer = true;
                        self.num_flag = false;
                        return self.skip_extension(&buf[i..]);
                    } else {
                        self.hex_count = Self::hex_to_decimal(self.hex_count, 0_i64)?;
                        continue;
                    }
                }
                b'1'..=b'9' => {
                    self.hex_count = Self::hex_to_decimal(self.hex_count, b as i64 - '0' as i64)?;
                    self.chunk_flag = true;
                    self.num_flag = true;
                    continue;
                }
                b'a'..=b'f' => {
                    self.hex_count =
                        Self::hex_to_decimal(self.hex_count, b as i64 - 'a' as i64 + 10i64)?;
                    self.chunk_flag = true;
                    self.num_flag = true;
                    continue;
                }
                b'A'..=b'F' => {
                    self.hex_count =
                        Self::hex_to_decimal(self.hex_count, b as i64 - 'A' as i64 + 10i64)?;
                    self.chunk_flag = true;
                    self.num_flag = true;
                    continue;
                }
                b' ' | b'\t' | b';' | b'\r' | b'\n' => {
                    if self.is_chunk_trailer {
                        self.num_flag = false;
                        return self.skip_trailer_crlf(&buf[i..]);
                    } else {
                        self.total_size = self.hex_count as usize;
                        self.hex_count = 0;
                        self.num_flag = false;
                        // Decode to the last chunk
                        return if self.total_size == 0 {
                            self.is_last_chunk = true;
                            self.skip_extension(&buf[i..])
                        } else {
                            self.rest_size = self.total_size;
                            self.skip_extension(&buf[i..])
                        };
                    }
                }
                _ => return Err(ErrorKind::InvalidInput.into()),
            }
        }
        Ok((
            Chunk {
                id: 0,
                state: ChunkState::MetaSize,
                size: self.total_size,
                extension: ChunkExt::new(),
                data: &buf[..0],
                trailer: None,
            },
            &buf[buf.len()..],
        ))
    }

    fn skip_extension<'a>(&mut self, buf: &'a [u8]) -> Result<(Chunk<'a>, &'a [u8]), HttpError> {
        self.stage = Stage::Extension;
        if self.is_chunk_trailer {
            for (i, &b) in buf.iter().enumerate() {
                match b {
                    b'\r' => {
                        if self.cr_meet {
                            return Err(ErrorKind::InvalidInput.into());
                        }
                        self.cr_meet = true;
                        return self.skip_trailer_crlf(&buf[i + 1..]);
                    }
                    b'\n' => {
                        if !self.cr_meet {
                            return Err(ErrorKind::InvalidInput.into());
                        }
                        self.cr_meet = false;

                        return self.skip_trailer_crlf(&buf[i..]);
                    }
                    _ => {}
                }
            }
            Ok((
                Chunk {
                    id: 0,
                    state: ChunkState::MetaExt,
                    size: self.total_size,
                    extension: ChunkExt::new(),
                    data: &buf[..0],
                    trailer: Some(&buf[..0]),
                },
                &buf[buf.len()..],
            ))
        } else {
            for (i, &b) in buf.iter().enumerate() {
                match b {
                    b'\r' => {
                        if self.cr_meet {
                            return Err(ErrorKind::InvalidInput.into());
                        }
                        self.cr_meet = true;
                        return self.skip_crlf(&buf[i + 1..]);
                    }
                    b'\n' => {
                        if !self.cr_meet {
                            return Err(ErrorKind::InvalidInput.into());
                        }
                        self.cr_meet = false;
                        return self.skip_crlf(&buf[i..]);
                    }
                    _ => {}
                }
            }
            Ok((
                Chunk {
                    id: 0,
                    state: ChunkState::MetaExt,
                    size: self.total_size,
                    extension: ChunkExt::new(),
                    data: &buf[..0],
                    trailer: None,
                },
                &buf[buf.len()..],
            ))
        }
    }

    fn skip_crlf<'a>(&mut self, buf: &'a [u8]) -> Result<(Chunk<'a>, &'a [u8]), HttpError> {
        self.stage = Stage::SizeEnd;
        for (i, &b) in buf.iter().enumerate() {
            match b {
                b'\r' => {
                    if self.cr_meet {
                        // TODO Check whether the state machine needs to be reused after the parsing
                        // fails and whether the state machine status needs to be adjusted.
                        return Err(ErrorKind::InvalidInput.into());
                    }
                    self.cr_meet = true;
                }
                b'\n' => {
                    if !self.cr_meet {
                        return Err(ErrorKind::InvalidInput.into());
                    }
                    self.cr_meet = false;
                    return self.decode_data(&buf[i + 1..]);
                }
                _ => return Err(ErrorKind::InvalidInput.into()),
            }
        }
        Ok((
            Chunk {
                id: 0,
                state: ChunkState::MetaCrlf,
                size: self.total_size,
                extension: ChunkExt::new(),
                data: &buf[..0],
                trailer: None,
            },
            &buf[buf.len()..],
        ))
    }

    fn skip_trailer_crlf<'a>(&mut self, buf: &'a [u8]) -> Result<(Chunk<'a>, &'a [u8]), HttpError> {
        self.stage = Stage::TrailerCrlf;
        for (i, &b) in buf.iter().enumerate() {
            match b {
                b'\r' => {
                    if self.cr_meet {
                        return Err(ErrorKind::InvalidInput.into());
                    }
                    self.cr_meet = true;
                }
                b'\n' => {
                    if !self.cr_meet {
                        return Err(ErrorKind::InvalidInput.into());
                    }
                    self.cr_meet = false;
                    self.is_trailer_crlf = true;
                    return self.decode_trailer_data(&buf[i + 1..]);
                }
                _ => return Err(ErrorKind::InvalidInput.into()),
            }
        }
        Ok((
            Chunk {
                id: 0,
                state: ChunkState::MetaCrlf,
                size: self.total_size,
                extension: ChunkExt::new(),
                data: &buf[..0],
                trailer: Some(&buf[..0]),
            },
            &buf[buf.len()..],
        ))
    }

    fn decode_trailer_data<'a>(
        &mut self,
        buf: &'a [u8],
    ) -> Result<(Chunk<'a>, &'a [u8]), HttpError> {
        self.stage = Stage::TrailerData;
        if buf.is_empty() {
            return Ok((
                Chunk {
                    id: 0,
                    state: ChunkState::Data,
                    size: 0,
                    extension: ChunkExt::new(),
                    data: &buf[..0],
                    trailer: Some(&buf[..0]),
                },
                &buf[buf.len()..],
            ));
        }

        if buf[0] == b'\r' && self.is_trailer_crlf {
            self.is_last_chunk = true;
        }

        for (i, &b) in buf.iter().enumerate() {
            match b {
                b'\r' => {
                    if self.cr_meet {
                        return Err(ErrorKind::InvalidInput.into());
                    }
                    self.cr_meet = true;
                    return self.skip_trailer_last_crlf(&buf[..i], &buf[i + 1..]);
                }
                b'\n' => {
                    if !self.cr_meet {
                        return Err(ErrorKind::InvalidInput.into());
                    }
                    self.cr_meet = false;
                    return self.skip_trailer_last_crlf(&buf[..i], &buf[i..]);
                }
                _ => {}
            }
        }
        self.is_trailer_crlf = false;
        Ok((
            Chunk {
                id: 0,
                state: ChunkState::Data,
                size: 0,
                extension: ChunkExt::new(),
                data: &buf[..0],
                trailer: Some(buf),
            },
            &buf[buf.len()..],
        ))
    }

    fn decode_data<'a>(&mut self, buf: &'a [u8]) -> Result<(Chunk<'a>, &'a [u8]), HttpError> {
        self.stage = Stage::Data;
        if buf.is_empty() {
            return Ok((
                Chunk {
                    id: 0,
                    state: ChunkState::Data,
                    size: self.total_size,
                    extension: ChunkExt::new(),
                    data: &buf[..0],
                    trailer: None,
                },
                &buf[buf.len()..],
            ));
        }

        let rest = self.rest_size;
        if buf.len() >= rest {
            self.rest_size = 0;
            self.cr_meet = false;
            self.skip_last_crlf(&buf[..rest], &buf[rest..])
        } else {
            self.rest_size -= buf.len();
            Ok((
                Chunk {
                    id: 0,
                    state: ChunkState::Data,
                    size: self.total_size,
                    extension: ChunkExt::new(),
                    data: buf,
                    trailer: None,
                },
                &buf[buf.len()..],
            ))
        }
    }

    fn skip_trailer_last_crlf<'a>(
        &mut self,
        data: &'a [u8],
        buf: &'a [u8],
    ) -> Result<(Chunk<'a>, &'a [u8]), HttpError> {
        self.stage = Stage::TrailerEndCrlf;
        for (i, &b) in buf.iter().enumerate() {
            match b {
                b'\r' => {
                    if self.cr_meet {
                        return Err(ErrorKind::InvalidInput.into());
                    }
                    self.cr_meet = true;
                }
                b'\n' => {
                    if !self.cr_meet {
                        return Err(ErrorKind::InvalidInput.into());
                    }
                    self.cr_meet = false;
                    return if self.is_last_chunk {
                        self.stage = Stage::TrailerEndCrlf;
                        Ok((
                            Chunk {
                                id: 0,
                                state: ChunkState::Finish,
                                size: 0,
                                extension: ChunkExt::new(),
                                data: &buf[..0],
                                trailer: Some(&buf[..0]),
                            },
                            &buf[i + 1..],
                        ))
                    } else {
                        self.cr_meet = false;
                        self.is_trailer_crlf = true;
                        self.stage = Stage::TrailerData;
                        let complete_chunk = Chunk {
                            id: 0,
                            state: ChunkState::DataCrlf,
                            size: 0,
                            extension: ChunkExt::new(),
                            data: &data[..0],
                            trailer: Some(data),
                        };
                        return Ok((complete_chunk, &buf[i + 1..]));
                    };
                }
                _ => return Err(ErrorKind::InvalidInput.into()),
            }
        }

        Ok((
            Chunk {
                id: 0,
                state: ChunkState::DataCrlf,
                size: 0,
                extension: ChunkExt::new(),
                data: &data[..0],
                trailer: Some(data),
            },
            &buf[buf.len()..],
        ))
    }

    fn skip_last_crlf<'a>(
        &mut self,
        data: &'a [u8],
        buf: &'a [u8],
    ) -> Result<(Chunk<'a>, &'a [u8]), HttpError> {
        self.stage = Stage::DataEnd;
        for (i, &b) in buf.iter().enumerate() {
            match b {
                b'\r' => {
                    if self.cr_meet {
                        return Err(ErrorKind::InvalidInput.into());
                    }
                    self.cr_meet = true;
                }
                b'\n' => {
                    if !self.cr_meet {
                        return Err(ErrorKind::InvalidInput.into());
                    }
                    self.cr_meet = false;
                    self.stage = Stage::Size;
                    let complete_chunk = Chunk {
                        id: 0,
                        state: ChunkState::Finish,
                        size: self.total_size,
                        extension: ChunkExt::new(),
                        data,
                        trailer: None,
                    };
                    self.total_size = 0;
                    return Ok((complete_chunk, &buf[i + 1..]));
                }
                _ => return Err(ErrorKind::InvalidInput.into()),
            }
        }
        Ok((
            Chunk {
                id: 0,
                state: ChunkState::DataCrlf,
                size: self.total_size,
                extension: ChunkExt::new(),
                data,
                trailer: None,
            },
            &buf[buf.len()..],
        ))
    }
}

#[cfg(test)]
mod ut_chunk {
    use crate::body::chunk::ChunkBody;
    use crate::body::sync_impl::Body;
    use crate::body::{async_impl, Chunk, ChunkBodyDecoder, ChunkExt, ChunkState, Chunks};
    use crate::error::ErrorKind;
    use crate::headers::Headers;

    fn data_message() -> Vec<u8> {
        let mut vec = Vec::new();
        for i in 0..=10 {
            vec.extend_from_slice(&[i % 10; 100]);
        }
        vec
    }

    fn res_message() -> Vec<u8> {
        let mut res = b"400\r\n".to_vec();
        for i in 0..=9 {
            res.extend_from_slice(&[i % 10; 100]);
        }
        res.extend_from_slice(&[0; 24]);
        res.extend_from_slice(b"\r\n4c\r\n");
        res.extend_from_slice(&[0; 76]);
        res.extend_from_slice(b"\r\n0\r\n\r\n");
        res
    }
    fn res_trailer_message() -> Vec<u8> {
        let mut res = b"400\r\n".to_vec();
        for i in 0..=9 {
            res.extend_from_slice(&[i % 10; 100]);
        }
        res.extend_from_slice(&[0; 24]);
        res.extend_from_slice(b"\r\n4c\r\n");
        res.extend_from_slice(&[0; 76]);
        res.extend_from_slice(b"\r\n0\r\n");
        res.extend_from_slice(b"accept:text/html\r\n");
        res.extend_from_slice(b"\r\n");
        res
    }

    /// UT test cases for `ChunkBody::set_trailer`.
    ///
    /// # Brief
    /// 1. Creates a `ChunkBody` by calling `ChunkBody::set_trailer`.
    /// 2. Encodes chunk body by calling `ChunkBody::data`
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_chunk_body_encode_trailer_0() {
        let mut headers = Headers::new();
        let _ = headers.insert("accept", "text/html");
        let content = data_message();
        let mut task = ChunkBody::from_bytes(content.as_slice()).set_trailer(headers);
        let mut user_slice = [0_u8; 20];
        let mut output_vec = vec![];
        let mut size = user_slice.len();
        while size == user_slice.len() {
            size = task.data(user_slice.as_mut_slice()).unwrap();
            output_vec.extend_from_slice(&user_slice[..size]);
        }
        assert_eq!(output_vec, res_trailer_message());
    }

    /// UT test cases for `ChunkBody::data`.
    ///
    /// # Brief
    /// 1. Creates a `ChunkBody` by calling `ChunkBody::from_bytes`.
    /// 2. Encodes chunk body by calling `ChunkBody::data`
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_chunk_body_encode_0() {
        let content = data_message();
        let mut task = ChunkBody::from_bytes(content.as_slice());
        let mut user_slice = [0_u8; 20];
        let mut output_vec = vec![];

        let mut size = user_slice.len();
        while size == user_slice.len() {
            size = task.data(user_slice.as_mut_slice()).unwrap();
            output_vec.extend_from_slice(&user_slice[..size]);
        }
        assert_eq!(output_vec, res_message());
    }

    /// UT test cases for `ChunkBody::data`.
    ///
    /// # Brief
    /// 1. Creates a `ChunkBody` by calling `ChunkBody::from_reader`.
    /// 2. Encodes chunk body by calling `ChunkBody::data`
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_chunk_body_encode_1() {
        let content = data_message();
        let mut task = ChunkBody::from_reader(content.as_slice());
        let mut user_slice = [0_u8; 20];
        let mut output_vec = vec![];

        let mut size = user_slice.len();
        while size == user_slice.len() {
            size = task.data(user_slice.as_mut_slice()).unwrap();
            output_vec.extend_from_slice(&user_slice[..size]);
        }
        assert_eq!(output_vec, res_message());
    }

    /// UT test cases for `ChunkBody::data` in async condition.
    ///
    /// # Brief
    /// 1. Creates a `ChunkBody` by calling `ChunkBody::from_bytes`.
    /// 2. Encodes chunk body by calling `async_impl::Body::data`
    /// 3. Checks if the test result is correct.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_asnyc_chunk_body_encode_0() {
        let handle = ylong_runtime::spawn(async move {
            asnyc_chunk_body_encode_0().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    #[cfg(feature = "ylong_base")]
    async fn asnyc_chunk_body_encode_0() {
        let content = data_message();
        let mut task = ChunkBody::from_bytes(content.as_slice());
        let mut user_slice = [0_u8; 20];
        let mut output_vec = vec![];

        let mut size = user_slice.len();
        while size == user_slice.len() {
            size = async_impl::Body::data(&mut task, user_slice.as_mut_slice())
                .await
                .unwrap();
            output_vec.extend_from_slice(&user_slice[..size]);
        }
        assert_eq!(output_vec, res_message());
    }

    /// UT test cases for `ChunkBody::data` in async condition.
    ///
    /// # Brief
    /// 1. Creates a `ChunkBody` by calling `ChunkBody::from_async_reader`.
    /// 2. Encodes chunk body by calling `async_impl::Body::data`
    /// 3. Checks if the test result is correct.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_asnyc_chunk_body_encode_1() {
        let handle = ylong_runtime::spawn(async move {
            asnyc_chunk_body_encode_1().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    #[cfg(feature = "ylong_base")]
    async fn asnyc_chunk_body_encode_1() {
        let content = data_message();
        let mut task = ChunkBody::from_async_reader(content.as_slice());
        let mut user_slice = [0_u8; 1024];
        let mut output_vec = vec![];

        let mut size = user_slice.len();
        while size == user_slice.len() {
            size = async_impl::Body::data(&mut task, user_slice.as_mut_slice())
                .await
                .unwrap();
            output_vec.extend_from_slice(&user_slice[..size]);
        }
        assert_eq!(output_vec, res_message());
    }

    /// UT test cases for `ChunkBodyDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `ChunkBodyDecoder` by calling `ChunkBodyDecoder::new`.
    /// 2. Decodes chunk body by calling `ChunkBodyDecoder::decode`
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_chunk_body_decode_0() {
        let mut decoder = ChunkBodyDecoder::new().contains_trailer(true);
        let chunk_body_bytes = "\
            5\r\n\
            hello\r\n\
            C ; type = text ;end = !\r\n\
            hello world!\r\n\
            000; message = last\r\n\
            Trailer: value\r\n\
            another-trainer: another-value\r\n\
            \r\n\
            "
        .as_bytes();
        // 5
        let res = decoder.decode(&chunk_body_bytes[..1]);
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 0,
            state: ChunkState::MetaSize,
            size: 0,
            extension: ChunkExt::new(),
            data: &[] as &[u8],
            trailer: None,
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8],)));
        // 5\r
        let res = decoder.decode(&chunk_body_bytes[1..2]);
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 0,
            state: ChunkState::MetaCrlf,
            size: 5,
            extension: ChunkExt::new(),
            data: &[] as &[u8],
            trailer: None,
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8],)));
        // 5\r
        let res = decoder.decode(&chunk_body_bytes[2..2]);
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 0,
            state: ChunkState::MetaCrlf,
            size: 5,
            extension: ChunkExt::new(),
            data: &[] as &[u8],
            trailer: None,
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8],)));
        // 5\r\n
        let res = decoder.decode(&chunk_body_bytes[2..3]);
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 0,
            state: ChunkState::Data,
            size: 5,
            extension: ChunkExt::new(),
            data: &[] as &[u8],
            trailer: None,
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8],)));

        // 5\r\nhe
        let res = decoder.decode(&chunk_body_bytes[3..5]);
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 0,
            state: ChunkState::Data,
            size: 5,
            extension: ChunkExt::new(),
            data: "he".as_bytes(),
            trailer: None,
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8],)));

        // 5\r\nhello\r
        let res = decoder.decode(&chunk_body_bytes[5..9]);
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 0,
            state: ChunkState::DataCrlf,
            size: 5,
            extension: ChunkExt::new(),
            data: "llo".as_bytes(),
            trailer: None,
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8],)));

        // 5\r\nhello\r
        let res = decoder.decode(&chunk_body_bytes[9..9]);
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 0,
            state: ChunkState::DataCrlf,
            size: 5,
            extension: ChunkExt::new(),
            data: &[] as &[u8],
            trailer: None,
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8],)));

        // 5\r\nhello\r\n
        let res = decoder.decode(&chunk_body_bytes[9..10]);
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 0,
            state: ChunkState::Finish,
            size: 5,
            extension: ChunkExt::new(),
            data: &[] as &[u8],
            trailer: None,
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8],)));

        // 5\r\nhello\r\nC ;
        let res = decoder.decode(&chunk_body_bytes[10..13]);
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 1,
            state: ChunkState::MetaExt,
            size: 12,
            extension: ChunkExt::new(),
            data: &[] as &[u8],
            trailer: None,
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8],)));

        // 5\r\nhello\r\nC ; type = text ;
        let res = decoder.decode(&chunk_body_bytes[13..27]);
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 1,
            state: ChunkState::MetaExt,
            size: 12,
            extension: ChunkExt::new(),
            data: &[] as &[u8],
            trailer: None,
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8],)));

        // 5\r\nhello\r\nC ; type = text ;end = !\r\n
        let res = decoder.decode(&chunk_body_bytes[27..36]);
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 1,
            state: ChunkState::Data,
            size: 12,
            extension: ChunkExt::new(),
            data: &[] as &[u8],
            trailer: None,
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8],)));

        // 5\r\nhello\r\nC ; type = text ;end = !\r\nhello world!\r\n
        let res = decoder.decode(&chunk_body_bytes[36..50]);
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 1,
            state: ChunkState::Finish,
            size: 12,
            extension: ChunkExt::new(),
            data: "hello world!".as_bytes(),
            trailer: None,
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8],)));

        // 5\r\nhello\r\nC ; type = text ;end = !\r\nhello world!\r\n0
        let res = decoder.decode(&chunk_body_bytes[50..51]);
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 2,
            state: ChunkState::MetaSize,
            size: 0,
            extension: ChunkExt::new(),
            data: &[] as &[u8],
            trailer: None,
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8],)));

        // 5\r\nhello\r\nC ; type = text ;end = !\r\nhello world!\r\n000;
        let res = decoder.decode(&chunk_body_bytes[51..54]);
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 2,
            state: ChunkState::MetaExt,
            size: 0,
            extension: ChunkExt::new(),
            data: &[] as &[u8],
            trailer: Some(&[] as &[u8]),
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8],)));

        // 5\r\nhello\r\nC ; type = text ;end = !\r\nhello world!\r\n000; message =
        // last\r\n
        let res = decoder.decode(&chunk_body_bytes[54..71]);
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 2,
            state: ChunkState::Data,
            size: 0,
            extension: ChunkExt::new(),
            data: &[] as &[u8],
            trailer: Some(&[] as &[u8]),
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8])));
        // 5\r\nhello\r\nC ; type = text ;end = !\r\nhello world!\r\n000; message =
        // last\r\nTrailer: value\r\n
        let res = decoder.decode(&chunk_body_bytes[71..87]);
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 3,
            state: ChunkState::DataCrlf,
            size: 0,
            extension: ChunkExt::new(),
            data: &[] as &[u8],
            trailer: Some("Trailer: value".as_bytes()),
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8])));
        // 5\r\nhello\r\nC ; type = text ;end = !\r\nhello world!\r\n000;
        // message = last\r\nTrailer: value\r\n\another-trainer: another-value\r\n\
        let res = decoder.decode(&chunk_body_bytes[87..119]);
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 4,
            state: ChunkState::DataCrlf,
            size: 0,
            extension: ChunkExt::new(),
            data: &[] as &[u8],
            trailer: Some("another-trainer: another-value".as_bytes()),
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8])));
        // 5\r\nhello\r\nC ; type = text ;end = !\r\nhello world!\r\n000;
        // message = last\r\nTrailer: value\r\n\another-trainer: another-value\r\n\r\n\
        let res = decoder.decode(&chunk_body_bytes[119..121]);
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 5,
            state: ChunkState::Finish,
            size: 0,
            extension: ChunkExt::new(),
            data: &[] as &[u8],
            trailer: Some(&[] as &[u8]),
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8])));
    }

    /// UT test cases for `ChunkBodyDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `ChunkBodyDecoder` by calling `ChunkBodyDecoder::new`.
    /// 2. Decodes chunk body by calling `ChunkBodyDecoder::decode`
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_chunk_body_decode_1() {
        let mut decoder = ChunkBodyDecoder::new();
        let chunk_body_bytes = "\
            5\r\n\
            hello\r\n\
            C ; type = text ;end = !\r\n\
            hello world!\r\n\
            000; message = last\r\n\
            \r\n\
            "
        .as_bytes();

        // 5
        let (chunks, remaining) = decoder.decode(chunk_body_bytes).unwrap();
        let mut iter = chunks.iter();
        let chunk = Chunk {
            id: 0,
            state: ChunkState::Finish,
            size: 5,
            extension: ChunkExt::new(),
            data: "hello".as_bytes(),
            trailer: None,
        };
        assert_eq!(iter.next(), Some(&chunk));
        let chunk = Chunk {
            id: 1,
            state: ChunkState::Finish,
            size: 12,
            extension: ChunkExt::new(),
            data: "hello world!".as_bytes(),
            trailer: None,
        };
        assert_eq!(iter.next(), Some(&chunk));
        let chunk = Chunk {
            id: 2,
            state: ChunkState::Finish,
            size: 0,
            extension: ChunkExt::new(),
            data: &[] as &[u8],
            trailer: None,
        };
        assert_eq!(iter.next(), Some(&chunk));
        assert_eq!(iter.next(), None);
        assert_eq!(remaining, "".as_bytes());
    }

    /// UT test cases for `ChunkBodyDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `ChunkBodyDecoder` by calling `ChunkBodyDecoder::new`.
    /// 2. Decodes chunk body by calling `ChunkBodyDecoder::decode`
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_chunk_body_decode_2() {
        let mut decoder = ChunkBodyDecoder::new();
        let chunk_body_bytes = "\
            5\r\n\
            hello\r\n\
            C ; type = text ;end = !\r\n\
            hello world!\r\n\
            000; message = last\r\n\
            \r\n\
            "
        .as_bytes();

        // 5
        let (chunks, remaining) = decoder.decode(chunk_body_bytes).unwrap();
        let mut iter = chunks.into_iter();
        let chunk = Chunk {
            id: 0,
            state: ChunkState::Finish,
            size: 5,
            extension: ChunkExt::new(),
            data: "hello".as_bytes(),
            trailer: None,
        };
        assert_eq!(iter.next(), Some(chunk));
        let chunk = Chunk {
            id: 1,
            state: ChunkState::Finish,
            size: 12,
            extension: ChunkExt::new(),
            data: "hello world!".as_bytes(),
            trailer: None,
        };
        assert_eq!(iter.next(), Some(chunk));
        let chunk = Chunk {
            id: 2,
            state: ChunkState::Finish,
            size: 0,
            extension: ChunkExt::new(),
            data: &[] as &[u8],
            trailer: None,
        };
        assert_eq!(iter.next(), Some(chunk));
        assert_eq!(iter.next(), None);
        assert_eq!(remaining, "".as_bytes());
    }

    /// UT test cases for `ChunkBodyDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `ChunkBodyDecoder` by calling `ChunkBodyDecoder::new`.
    /// 2. Decodes chunk body by calling `ChunkBodyDecoder::decode`
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_chunk_body_decode_3() {
        let chunk_body_bytes = "\
            5 ; type = text ;end = !\r\n\
            hello world!\r\n\
            000; message = last\r\n\
            \r\n\
            "
        .as_bytes();
        let mut decoder = ChunkBodyDecoder::new();

        // 5
        let res = decoder.decode(chunk_body_bytes);
        assert_eq!(res, Err(ErrorKind::InvalidInput.into()));
    }

    /// UT test cases for `ChunkBodyDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `ChunkBodyDecoder` by calling `ChunkBodyDecoder::new`.
    /// 2. Decodes chunk body by calling `ChunkBodyDecoder::decode`
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_chunk_body_decode_4() {
        let chunk_body_bytes = "\
            C ; type = text ;end = !\r\r\n\
            hello world!\r\n\
            000; message = last\r\n\
            Trailer: value\r\n\
            another-trainer: another-value\r\n\
            \r\n\
            "
        .as_bytes();
        let mut decoder = ChunkBodyDecoder::new();

        // 5
        let res = decoder.decode(chunk_body_bytes);
        assert_eq!(res, Err(ErrorKind::InvalidInput.into()));
    }

    /// UT test cases for `ChunkBodyDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `ChunkBodyDecoder` by calling `ChunkBodyDecoder::new`.
    /// 2. Decodes chunk body by calling `ChunkBodyDecoder::decode`
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_chunk_body_decode_5() {
        let chunk_body_bytes = " C ; type = text ;end = !\r\n\
            hello world!\r\n\
            000; message = last\r\n\
            Trailer: value\r\n\
            another-trainer: another-value\r\n\
            \r\n\
            "
        .as_bytes();
        let mut decoder = ChunkBodyDecoder::new();
        // 5
        let res = decoder.decode(chunk_body_bytes);
        assert_eq!(res, Err(ErrorKind::InvalidInput.into()));
    }

    /// UT test cases for `ChunkBodyDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `ChunkBodyDecoder` by calling `ChunkBodyDecoder::new`.
    /// 2. Decodes chunk body by calling `ChunkBodyDecoder::decode`
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_chunk_body_decode_6() {
        let mut decoder = ChunkBodyDecoder::new();
        let chunk_body_bytes = "\
            5\r\n\
            hello\r\n\
            C ; type = text ;end = !\r\n\
            hello world!\r\n\
            000; message = last\r\n\
            \r\n\
            "
        .as_bytes();

        // 5
        let (chunks, _) = decoder.decode(chunk_body_bytes).unwrap();
        assert_eq!(chunks.iter().len(), 3);
        let chunk = chunks.iter().next().unwrap();
        assert_eq!(
            (
                chunk.id(),
                chunk.state(),
                chunk.size(),
                chunk.extension(),
                chunk.data()
            ),
            (
                0,
                &ChunkState::Finish,
                5,
                &ChunkExt::new(),
                "hello".as_bytes()
            )
        );
    }

    /// UT test cases for `ChunkBodyDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `ChunkBodyDecoder` by calling `ChunkBodyDecoder::new`.
    /// 2. Decodes chunk body by calling `ChunkBodyDecoder::decode`
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_chunk_body_decode_7() {
        let mut decoder = ChunkBodyDecoder::new().contains_trailer(true);
        let buf = b"010\r\nAAAAAAAAAAAAAAAA\r\n0\r\ntrailer:value\r\n\r\n";
        let res = decoder.decode(&buf[0..23]); // 010\r\nAAAAAAAAAAAAAAAA\r\n
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 0,
            state: ChunkState::Finish,
            size: 16,
            extension: ChunkExt::new(),
            data: "AAAAAAAAAAAAAAAA".as_bytes(),
            trailer: None,
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8])));

        let res = decoder.decode(&buf[23..39]); // 0\r\ntrailer:value
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 1,
            state: ChunkState::Data,
            size: 0,
            extension: ChunkExt::new(),
            data: &[] as &[u8],
            trailer: Some("trailer:value".as_bytes()),
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8])));

        let res = decoder.decode(&buf[39..41]); //\r\n
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 2,
            state: ChunkState::DataCrlf,
            size: 0,
            extension: ChunkExt::new(),
            data: &[] as &[u8],
            trailer: Some(&[] as &[u8]),
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8])));

        let res = decoder.decode(&buf[41..]); //\r\n
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 3,
            state: ChunkState::Finish,
            size: 0,
            extension: ChunkExt::new(),
            data: &[] as &[u8],
            trailer: Some(&[] as &[u8]),
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8])));

        let trailer_headers = decoder.get_trailer().unwrap().unwrap();
        let value = trailer_headers.get("trailer");
        assert_eq!(value.unwrap().to_string().unwrap(), "value");
    }

    /// UT test cases for `ChunkBodyDecoder::decode`.
    ///
    /// # Brief
    /// 1. Creates a `ChunkBodyDecoder` by calling `ChunkBodyDecoder::new`.
    /// 2. Decodes chunk body by calling `ChunkBodyDecoder::decode`
    /// 3. Checks if the test result is correct.
    #[test]
    fn ut_chunk_body_decode_8() {
        let mut decoder = ChunkBodyDecoder::new().contains_trailer(true);
        let buf = b"010\r\nAAAAAAAAAAAAAAAA\r\n0\r\ntrailer:value\r\n\r\n";
        let res = decoder.decode(&buf[0..2]); // 01
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 0,
            state: ChunkState::MetaSize,
            size: 0,
            extension: ChunkExt::new(),
            data: &[] as &[u8],
            trailer: None,
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8])));

        let res = decoder.decode(&buf[2..23]); // 0\r\nAAAAAAAAAAAAAAAA\r\n
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 0,
            state: ChunkState::Finish,
            size: 16,
            extension: ChunkExt::new(),
            data: "AAAAAAAAAAAAAAAA".as_bytes(),
            trailer: None,
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8])));

        let res = decoder.decode(&buf[23..39]); // 0\r\ntrailer:value
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 1,
            state: ChunkState::Data,
            size: 0,
            extension: ChunkExt::new(),
            data: &[] as &[u8],
            trailer: Some("trailer:value".as_bytes()),
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8])));

        let res = decoder.decode(&buf[39..41]); //\r\n
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 2,
            state: ChunkState::DataCrlf,
            size: 0,
            extension: ChunkExt::new(),
            data: &[] as &[u8],
            trailer: Some(&[] as &[u8]),
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8])));

        let res = decoder.decode(&buf[41..]); //\r\n
        let mut chunks = Chunks::new();
        chunks.push(Chunk {
            id: 3,
            state: ChunkState::Finish,
            size: 0,
            extension: ChunkExt::new(),
            data: &[] as &[u8],
            trailer: Some(&[] as &[u8]),
        });
        assert_eq!(res, Ok((chunks, &[] as &[u8])));

        let trailer_headers = decoder.get_trailer().unwrap().unwrap();
        let value = trailer_headers.get("trailer");
        assert_eq!(value.unwrap().to_string().unwrap(), "value");
    }
}
