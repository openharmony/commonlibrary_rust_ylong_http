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

// TODO: reuse mime later.

use std::io::Cursor;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::vec::IntoIter;

use crate::body::async_impl::Body;
use crate::{AsyncRead, ReadBuf};

/// A structure that helps you build a `multipart/form-data` message.
///
/// # Examples
///
/// ```
/// # use ylong_http::body::{MultiPart, Part};
///
/// let multipart = MultiPart::new()
///     .part(Part::new().name("name").body("xiaoming"))
///     .part(Part::new().name("password").body("123456789"));
/// ```
pub struct MultiPart {
    parts: Vec<Part>,
    boundary: String,
    status: ReadStatus,
}

impl MultiPart {
    /// Creates an empty `Multipart` with boundary created automatically.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http::body::MultiPart;
    ///
    /// let multipart = MultiPart::new();
    /// ```
    pub fn new() -> Self {
        Self {
            parts: Vec::new(),
            boundary: gen_boundary(),
            status: ReadStatus::Never,
        }
    }

    /// Sets a part to the `Multipart`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http::body::{MultiPart, Part};
    ///
    /// let multipart = MultiPart::new().part(Part::new().name("name").body("xiaoming"));
    /// ```
    pub fn part(mut self, part: Part) -> Self {
        self.parts.push(part);
        self
    }

    /// Gets the boundary of this `Multipart`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http::body::MultiPart;
    ///
    /// let multipart = MultiPart::new();
    /// let boundary = multipart.boundary();
    /// ```
    pub fn boundary(&self) -> &str {
        self.boundary.as_str()
    }

    /// Get the total bytes of the `multpart/form-data` message, including
    /// length of every parts, such as boundaries, headers, bodies, etc.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http::body::{MultiPart, Part};
    ///
    /// let multipart = MultiPart::new().part(Part::new().name("name").body("xiaoming"));
    ///
    /// let bytes = multipart.total_bytes();
    /// ```
    pub fn total_bytes(&self) -> Option<u64> {
        let mut size = 0u64;
        for part in self.parts.iter() {
            size += part.length?;

            // start boundary + \r\n
            size += 2 + self.boundary.len() as u64 + 2;

            // Content-Disposition: form-data
            size += 30;

            // ; name="xxx"
            if let Some(name) = part.name.as_ref() {
                size += 9 + name.len() as u64;
            }

            // ; filename="xxx"
            if let Some(name) = part.file_name.as_ref() {
                size += 13 + name.len() as u64;
            }

            // \r\n
            size += 2;

            // Content-Type: xxx
            if let Some(mime) = part.mime.as_ref() {
                size += 16 + mime.len() as u64;
            }

            // \r\n\r\n
            size += 2 + 2;
        }
        // last boundary
        size += 2 + self.boundary.len() as u64 + 4;
        Some(size)
    }

    pub(crate) fn build_status(&mut self) {
        let mut states = Vec::new();
        for part in self.parts.iter_mut() {
            states.push(MultiPartState::bytes(
                format!("--{}\r\n", self.boundary).into_bytes(),
            ));
            states.push(MultiPartState::bytes(
                b"Content-Disposition: form-data".to_vec(),
            ));

            if let Some(ref name) = part.name {
                states.push(MultiPartState::bytes(
                    format!("; name=\"{name}\"").into_bytes(),
                ));
            }

            if let Some(ref file_name) = part.file_name {
                states.push(MultiPartState::bytes(
                    format!("; filename=\"{file_name}\"").into_bytes(),
                ));
            }

            states.push(MultiPartState::bytes(b"\r\n".to_vec()));

            if let Some(ref mime) = part.mime {
                states.push(MultiPartState::bytes(
                    format!("Content-Type: {mime}\r\n").into_bytes(),
                ));
            }

            states.push(MultiPartState::bytes(b"\r\n".to_vec()));

            if let Some(body) = part.body.take() {
                states.push(body);
            }

            states.push(MultiPartState::bytes(b"\r\n".to_vec()));
        }
        states.push(MultiPartState::bytes(
            format!("--{}--\r\n", self.boundary).into_bytes(),
        ));
        self.status = ReadStatus::Reading(MultiPartStates {
            states: states.into_iter(),
            curr: None,
        })
    }
}

impl Default for MultiPart {
    fn default() -> Self {
        Self::new()
    }
}

impl AsyncRead for MultiPart {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.status {
            ReadStatus::Never => self.build_status(),
            ReadStatus::Reading(_) => {}
            ReadStatus::Finish => return Poll::Ready(Ok(())),
        }

        if let ReadStatus::Reading(ref mut status) = self.status {
            if buf.initialize_unfilled().is_empty() {
                return Poll::Ready(Ok(()));
            }
            let filled = buf.filled().len();
            return match Pin::new(status).poll_read(cx, buf) {
                Poll::Ready(Ok(())) => {
                    let new_filled = buf.filled().len();
                    if filled == new_filled {
                        self.status = ReadStatus::Finish;
                    }
                    Poll::Ready(Ok(()))
                }
                Poll::Pending => {
                    let new_filled = buf.filled().len();
                    return if new_filled != filled {
                        Poll::Ready(Ok(()))
                    } else {
                        Poll::Pending
                    };
                }
                x => x,
            };
        }
        Poll::Ready(Ok(()))
    }
}

/// A structure that represents a part of `multipart/form-data` message.
///
/// # Examples
///
/// ```
/// # use ylong_http::body::Part;
///
/// let part = Part::new().name("name").body("xiaoming");
/// ```
pub struct Part {
    name: Option<String>,
    file_name: Option<String>,
    mime: Option<String>,
    length: Option<u64>,
    body: Option<MultiPartState>,
}

impl Part {
    /// Creates an empty `Part`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::Part;
    ///
    /// let part = Part::new();
    /// ```
    pub fn new() -> Self {
        Self {
            name: None,
            file_name: None,
            mime: None,
            length: None,
            body: None,
        }
    }

    /// Sets the name of this `Part`.
    ///
    /// The name message will be set to `Content-Disposition` header.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::Part;
    ///
    /// let part = Part::new().name("name");
    /// ```
    pub fn name(mut self, name: &str) -> Self {
        self.name = Some(String::from(name));
        self
    }

    /// Sets the file name of this `Part`.
    ///
    /// The file name message will be set to `Content-Disposition` header.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::Part;
    ///
    /// let part = Part::new().file_name("example.txt");
    /// ```
    pub fn file_name(mut self, file_name: &str) -> Self {
        self.file_name = Some(String::from(file_name));
        self
    }

    /// Sets the mime type of this `Part`.
    ///
    /// The mime type message will be set to `Content-Type` header.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::Part;
    ///
    /// let part = Part::new().mime("application/octet-stream");
    /// ```
    pub fn mime(mut self, mime: &str) -> Self {
        self.mime = Some(String::from(mime));
        self
    }

    /// Sets the length of body of this `Part`.
    ///
    /// The length message will be set to `Content-Length` header.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::Part;
    ///
    /// let part = Part::new().length(Some(8)).body("xiaoming");
    /// ```
    pub fn length(mut self, length: Option<u64>) -> Self {
        self.length = length;
        self
    }

    /// Sets a slice body of this `Part`.
    ///
    /// The body message will be set to the body part.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::Part;
    ///
    /// let part = Part::new().mime("application/octet-stream");
    /// ```
    pub fn body<T: AsRef<[u8]>>(mut self, body: T) -> Self {
        let body = body.as_ref().to_vec();
        self.length = Some(body.len() as u64);
        self.body = Some(MultiPartState::bytes(body));
        self
    }

    /// Sets a stream body of this `Part`.
    ///
    /// The body message will be set to the body part.
    pub fn stream<T: AsyncRead + Send + Sync + 'static>(mut self, body: T) -> Self {
        self.body = Some(MultiPartState::stream(Box::pin(body)));
        self
    }
}

impl Default for Part {
    fn default() -> Self {
        Self::new()
    }
}

/// A basic trait for MultiPart.
pub trait MultiPartBase: AsyncRead {
    /// Get reference of MultiPart.
    fn multipart(&self) -> &MultiPart;
}

impl MultiPartBase for MultiPart {
    fn multipart(&self) -> &MultiPart {
        self
    }
}

enum ReadStatus {
    Never,
    Reading(MultiPartStates),
    Finish,
}

struct MultiPartStates {
    states: IntoIter<MultiPartState>,
    curr: Option<MultiPartState>,
}

impl MultiPartStates {
    fn poll_read_curr(
        &mut self,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if let Some(mut state) = self.curr.take() {
            return match state {
                MultiPartState::Bytes(ref mut bytes) => {
                    let filled_len = buf.filled().len();
                    let unfilled = buf.initialize_unfilled();
                    let unfilled_len = unfilled.len();
                    let new = std::io::Read::read(bytes, unfilled).unwrap();
                    buf.set_filled(filled_len + new);

                    if new >= unfilled_len {
                        self.curr = Some(state);
                    }
                    Poll::Ready(Ok(()))
                }
                MultiPartState::Stream(ref mut stream) => {
                    let old_len = buf.filled().len();
                    match stream.as_mut().poll_read(cx, buf) {
                        Poll::Ready(Ok(())) => {
                            if old_len != buf.filled().len() {
                                self.curr = Some(state);
                            }
                            Poll::Ready(Ok(()))
                        }
                        Poll::Pending => {
                            self.curr = Some(state);
                            Poll::Pending
                        }
                        x => x,
                    }
                }
            };
        }
        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for MultiPartStates {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        while !buf.initialize_unfilled().is_empty() {
            if this.curr.is_none() {
                this.curr = match this.states.next() {
                    None => break,
                    x => x,
                }
            }

            match this.poll_read_curr(cx, buf) {
                Poll::Ready(Ok(())) => {}
                x => return x,
            }
        }
        Poll::Ready(Ok(()))
    }
}

enum MultiPartState {
    Bytes(Cursor<Vec<u8>>),
    Stream(Pin<Box<dyn AsyncRead + Send + Sync>>),
}

impl MultiPartState {
    fn bytes(bytes: Vec<u8>) -> Self {
        Self::Bytes(Cursor::new(bytes))
    }

    fn stream(reader: Pin<Box<dyn AsyncRead + Send + Sync>>) -> Self {
        Self::Stream(reader)
    }
}

#[cfg(test)]
impl PartialEq for MultiPartState {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Bytes(l0), Self::Bytes(r0)) => l0 == r0,
            // Cant not compare Stream, Should not do this.
            (Self::Stream(l0), Self::Stream(r0)) => core::ptr::eq(l0, r0),
            _ => false,
        }
    }
}

#[cfg(test)]
impl core::fmt::Debug for MultiPartState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bytes(arg0) => f.debug_tuple("Bytes").field(arg0).finish(),
            Self::Stream(arg0) => f.debug_tuple("Stream").field(&(arg0 as *const _)).finish(),
        }
    }
}

fn gen_boundary() -> String {
    format!(
        "{:016x}-{:016x}-{:016x}-{:016x}",
        xor_shift(),
        xor_shift(),
        xor_shift(),
        xor_shift()
    )
}

// XORShift* fast-random realization.
fn xor_shift() -> u64 {
    use std::cell::Cell;
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::num::Wrapping;

    thread_local! {
        static RNG: Cell<Wrapping<u64>> = Cell::new(Wrapping(seed()));
    }

    // The returned value of `seed()` must be nonzero.
    fn seed() -> u64 {
        let seed = RandomState::new();

        let mut out;
        let mut cnt = 1;
        let mut hasher = seed.build_hasher();

        loop {
            hasher.write_usize(cnt);
            out = hasher.finish();
            if out != 0 {
                break;
            }
            cnt += 1;
            hasher = seed.build_hasher();
        }
        out
    }

    RNG.with(|rng| {
        let mut n = rng.get();
        n ^= n >> 12;
        n ^= n << 25;
        n ^= n >> 27;
        rng.set(n);
        n.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    })
}

#[cfg(test)]
mod ut_mime {
    use crate::body::mime::simple::{gen_boundary, MultiPartState, ReadStatus};
    use crate::body::{MultiPart, Part};

    /// UT test cases for `gen_boundar`.
    ///
    /// # Brief
    /// 1. Creates two boundarys and compares.
    /// 3. Checks whether the result is correct.
    #[test]
    fn ut_gen_boundary() {
        let s1 = gen_boundary();
        let s2 = gen_boundary();
        assert_ne!(s1, s2);
    }

    /// UT test cases for `Part::new`.
    ///
    /// # Brief
    /// 1. Creates a `Part` by `Part::new`.
    /// 2. Checks members of `Part`.
    /// 3. Checks whether the result is correct.
    #[test]
    fn ut_part_new() {
        let part = Part::new();
        assert!(part.name.is_none());
        assert!(part.file_name.is_none());
        assert!(part.mime.is_none());
        assert!(part.length.is_none());
        assert!(part.body.is_none());
    }

    /// UT test cases for `Part::default`.
    ///
    /// # Brief
    /// 1. Creates a `Part` by `Part::default`.
    /// 2. Checks members of `Part`.
    /// 3. Checks whether the result is correct.
    #[test]
    fn ut_part_default() {
        let part = Part::default();
        assert!(part.name.is_none());
        assert!(part.file_name.is_none());
        assert!(part.mime.is_none());
        assert!(part.length.is_none());
        assert!(part.body.is_none());
    }

    /// UT test cases for `Part::name`, `Part::name`, `Part::file_name` and
    /// `Part::body`.
    ///
    /// # Brief
    /// 1. Creates a `Part` and sets values.
    /// 2. Checks members of `Part`.
    /// 3. Checks whether the result is correct.
    #[test]
    fn ut_part_set() {
        let part = Part::new()
            .name("name")
            .file_name("example.txt")
            .mime("application/octet-stream")
            .body("1234");
        assert_eq!(part.name, Some("name".to_string()));
        assert_eq!(part.file_name, Some("example.txt".to_string()));
        assert_eq!(part.mime, Some("application/octet-stream".to_string()));
        assert_eq!(part.body, Some(MultiPartState::bytes("1234".into())));
        assert_eq!(part.length, Some(4));

        let part = part.stream("11223344".as_bytes()).length(Some(8));
        assert_eq!(part.length, Some(8));
    }

    /// UT test cases for `MultiPart::new`.
    ///
    /// # Brief
    /// 1. Creates a `MultiPart` by `MultiPart::new`.
    /// 2. Checks members of `MultiPart`.
    /// 3. Checks whether the result is correct.
    #[test]
    fn ut_multipart_new() {
        let mp = MultiPart::new();
        assert!(mp.parts.is_empty());
        assert!(!mp.boundary().is_empty());
    }

    /// UT test cases for `MultiPart::part` and `MultiPart::total_bytes`.
    ///
    /// # Brief
    /// 1. Creates a `MultiPart` and sets values.
    /// 2. Checks total bytes of `MultiPart`.
    /// 3. Checks whether the result is correct.
    #[test]
    fn ut_multipart_set() {
        let mp = MultiPart::default();
        // --boundary--/r/n
        assert_eq!(mp.total_bytes(), Some(2 + mp.boundary().len() as u64 + 4));

        let mp = mp.part(
            Part::new()
                .name("name")
                .file_name("example.txt")
                .mime("application/octet-stream")
                .body("1234"),
        );
        assert_eq!(
            mp.total_bytes(),
            Some(
                (2 + mp.boundary().len() as u64 + 2)
                    + (30 + 9 + 4 + 13 + 11 + 2)       // name, filename, \r\n
                    + (16 + 24 + 2 + 2)                // mime, \r\n
                    + 4                                // body
                    + (2 + mp.boundary().len() as u64 + 4)
            )
        );
    }

    /// UT test cases for `MultiPart::poll_data`.
    ///
    /// # Brief
    /// 1. Creates a `MultiPart` and sets values.
    /// 2. Encodes `MultiPart` by `async_impl::Body::data`.
    /// 3. Checks whether the result is correct.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_multipart_poll_data() {
        let handle = ylong_runtime::spawn(async move {
            multipart_poll_data().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    #[cfg(feature = "ylong_base")]
    async fn multipart_poll_data() {
        use std::pin::Pin;

        use ylong_runtime::futures::poll_fn;
        use ylong_runtime::io::{AsyncRead, ReadBuf};

        let mut mp = MultiPart::new().part(
            Part::new()
                .name("name")
                .file_name("example.txt")
                .mime("application/octet-stream")
                .body("1234"),
        );

        let mut buf = vec![0u8; 50];
        let mut v_size = vec![];
        let mut v_str = vec![];

        loop {
            let mut read_buf = ReadBuf::new(&mut buf);
            poll_fn(|cx| Pin::new(&mut mp).poll_read(cx, &mut read_buf))
                .await
                .unwrap();

            let len = read_buf.filled_len();
            if len == 0 {
                break;
            }
            v_size.push(len);
            v_str.extend_from_slice(&buf[..len]);
        }
        assert_eq!(v_size, vec![50, 50, 50, 50, 50, 11]);
    }

    /// UT test cases for `MultiPart::poll_data`.
    ///
    /// # Brief
    /// 1. Creates a `MultiPart` and sets values.
    /// 2. Encodes `MultiPart` by `async_impl::Body::data`.
    /// 3. Checks whether the result is correct.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_multipart_poll_data_stream() {
        let handle = ylong_runtime::spawn(async move {
            multipart_poll_data_stream().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    #[cfg(feature = "ylong_base")]
    async fn multipart_poll_data_stream() {
        use std::pin::Pin;

        use ylong_runtime::futures::poll_fn;
        use ylong_runtime::io::{AsyncRead, ReadBuf};

        let mut mp = MultiPart::new().part(
            Part::new()
                .name("name")
                .file_name("example.txt")
                .mime("application/octet-stream")
                .stream("1234".as_bytes())
                .length(Some(4)),
        );

        let mut buf = vec![0u8; 50];
        let mut v_size = vec![];
        let mut v_str = vec![];

        loop {
            let mut read_buf = ReadBuf::new(&mut buf);
            poll_fn(|cx| Pin::new(&mut mp).poll_read(cx, &mut read_buf))
                .await
                .unwrap();

            let len = read_buf.filled().len();
            if len == 0 {
                break;
            }
            v_size.push(len);
            v_str.extend_from_slice(&buf[..len]);
        }
        assert_eq!(v_size, vec![50, 50, 50, 50, 50, 11]);
    }
}
