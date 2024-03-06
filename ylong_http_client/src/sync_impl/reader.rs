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

use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::time::{Duration, Instant};

use super::Body;
use crate::error::HttpClientError;
use crate::util::Timeout;
use crate::ErrorKind;

/// A reader used to read all the body data to a specified location and provide
/// echo function.
///
/// # Examples
///
/// ```
/// use ylong_http_client::sync_impl::{BodyProcessError, BodyProcessor, BodyReader, TextBody};
///
/// // Defines a processor, which provides read and echo ability.
/// struct Processor {
///     vec: Vec<u8>,
///     echo: usize,
/// }
///
/// // Implements `BodyProcessor` trait for `&mut Processor` instead of `Processor`
/// // if users want to get the result in struct after reading.
/// impl BodyProcessor for &mut Processor {
///     fn write(&mut self, data: &[u8]) -> Result<(), BodyProcessError> {
///         self.vec.extend_from_slice(data);
///         Ok(())
///     }
///
///     fn progress(&mut self, filled: usize) -> Result<(), BodyProcessError> {
///         self.echo += 1;
///         Ok(())
///     }
/// }
///
/// let mut body = TextBody::from_bytes(b"HelloWorld");
/// let mut processor = Processor {
///     vec: Vec::new(),
///     echo: 0,
/// };
/// let _ = BodyReader::new(&mut processor).read_all(&mut body);
///
/// // All data is read.
/// assert_eq!(processor.vec, b"HelloWorld");
/// // It will be echoed multiple times during the reading process.
/// assert_ne!(processor.echo, 0);
/// ```
pub struct BodyReader<T: BodyProcessor> {
    pub(crate) read_timeout: Timeout,
    pub(crate) processor: T,
}

impl<T: BodyProcessor> BodyReader<T> {
    /// Creates a new `BodyReader` with the given `Processor`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::sync_impl::{BodyReader, DefaultBodyProcessor};
    ///
    /// let reader = BodyReader::new(DefaultBodyProcessor::new());
    /// ```
    pub fn new(processor: T) -> Self {
        Self {
            read_timeout: Timeout::none(),
            processor,
        }
    }

    /// Sets body read timeout.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::sync_impl::{BodyReader, DefaultBodyProcessor};
    /// use ylong_http_client::Timeout;
    ///
    /// let reader = BodyReader::new(DefaultBodyProcessor::new()).read_timeout(Timeout::none());
    /// ```
    pub fn read_timeout(mut self, timeout: Timeout) -> Self {
        self.read_timeout = timeout;
        self
    }

    /// Reads all the body data. During the read process,
    /// [`BodyProcessor::write`] and [`BodyProcessor::progress`] will be
    /// called multiple times.
    ///
    /// [`BodyProcessor::write`]: BodyProcessor::write
    /// [`BodyProcessor::progress`]: BodyProcessor::progress
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::sync_impl::{BodyProcessor, BodyReader, TextBody};
    ///
    /// let mut body = TextBody::from_bytes(b"HelloWorld");
    /// let _ = BodyReader::default().read_all(&mut body);
    /// ```
    pub fn read_all<B: Body>(&mut self, body: &mut B) -> Result<(), HttpClientError> {
        // Use buffers up to 16K in size to read body.
        const TEMP_BUF_SIZE: usize = 16 * 1024;

        let mut last = Instant::now();
        let mut buf = [0u8; TEMP_BUF_SIZE];
        let mut written = 0usize;

        loop {
            let read_len = body
                .data(&mut buf)
                .map_err(|e| HttpClientError::from_error(ErrorKind::BodyDecode, e))?;

            if read_len == 0 {
                self.processor
                    .progress(written)
                    .map_err(|e| HttpClientError::from_error(ErrorKind::BodyDecode, e))?;
                break;
            }

            self.processor
                .write(&buf[..read_len])
                .map_err(|e| HttpClientError::from_error(ErrorKind::BodyDecode, e))?;

            written += read_len;

            let now = Instant::now();
            if now.duration_since(last) >= Duration::from_secs(1) {
                self.processor
                    .progress(written)
                    .map_err(|e| HttpClientError::from_error(ErrorKind::BodyDecode, e))?;
            }
            last = now;
        }
        Ok(())
    }
}

impl Default for BodyReader<DefaultBodyProcessor> {
    fn default() -> Self {
        Self::new(DefaultBodyProcessor::new())
    }
}

/// The trait defines methods for processing bodies of HTTP messages. Unlike the
/// async version, this is for synchronous usage.
pub trait BodyProcessor {
    /// Writes the body data read each time to the specified location.
    ///
    /// This method will be called every time a part of the body data is read.
    fn write(&mut self, data: &[u8]) -> Result<(), BodyProcessError>;

    /// Informs users how many bytes have been written to the specified location
    /// at this time. Users can display the progress according to the number of
    /// bytes written.
    fn progress(&mut self, filled: usize) -> Result<(), BodyProcessError>;
}

/// Error occurs when processing body data.
#[derive(Debug)]
pub struct BodyProcessError;

impl Display for BodyProcessError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(self, f)
    }
}

impl Error for BodyProcessError {}

/// A default body processor that write data to console directly.
pub struct DefaultBodyProcessor;

impl DefaultBodyProcessor {
    /// Creates a new `DefaultBodyProcessor`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::sync_impl::DefaultBodyProcessor;
    ///
    /// let processor = DefaultBodyProcessor::new();
    /// ```
    pub fn new() -> Self {
        Self
    }
}

impl BodyProcessor for DefaultBodyProcessor {
    fn write(&mut self, data: &[u8]) -> Result<(), BodyProcessError> {
        println!("{data:?}");
        Ok(())
    }

    fn progress(&mut self, filled: usize) -> Result<(), BodyProcessError> {
        println!("filled: {filled}");
        Ok(())
    }
}

impl Default for DefaultBodyProcessor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod ut_syn_reader {
    use ylong_http::body::TextBody;

    use crate::sync_impl::{BodyReader, DefaultBodyProcessor};
    use crate::util::Timeout;

    /// UT test cases for `BodyReader::read_timeout`.
    ///
    /// # Brief
    /// 1. Creates a `BodyReader` with `DefaultBodyProcessor::default` by
    ///    calling `BodyReader::new`.
    /// 2. Calls `read_timeout`.
    /// 3. Checks if the result is correct.
    #[test]
    fn ut_body_reader_read_timeout() {
        let reader = BodyReader::new(DefaultBodyProcessor).read_timeout(Timeout::none());
        assert_eq!(reader.read_timeout, Timeout::none());
    }

    /// UT test cases for `BodyReader::read_all`.
    ///
    /// # Brief
    /// 1. Creates a `BodyReader` by calling `BodyReader::default`.
    /// 2. Creates a `TextBody`.
    /// 3. Calls `read_all` method.
    /// 4. Checks if the result is corrent.
    #[test]
    fn ut_body_reader_read_all() {
        let mut body = TextBody::from_bytes(b"HelloWorld");
        let res = BodyReader::default().read_all(&mut body);
        assert!(res.is_ok());
    }
}
