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

use core::fmt::Debug;
use core::pin::Pin;
use core::task::{Context, Poll};
use std::io::Read;

use crate::body::async_impl::{self, DataFuture};
use crate::body::mime::common::{data_copy, SizeResult, TokenStatus};
use crate::body::sync_impl;
use crate::{AsyncRead, ReadBuf};

// Uses Box<dyn trait> so that it can be put into a list(like vec) with
// different T.
pub(crate) enum MixFrom<'a> {
    // the read content is Vec<u8>
    Owned { bytes: Vec<u8>, index: usize },
    // the read content is from memory
    Slice { bytes: &'a [u8], index: usize },
    // the read content is from a synchronous reader
    Reader(Box<dyn Read + Send + Sync>),
    // the read content is from an asynchronous reader
    AsyncReader(Box<dyn AsyncRead + Send + Sync + Unpin>),
}

impl<'a> MixFrom<'a> {
    // Usually needs owned.
    pub(crate) fn new() -> Self {
        MixFrom::Owned {
            bytes: vec![],
            index: 0,
        }
    }

    pub(crate) fn new_as_bytes() -> Self {
        MixFrom::Slice {
            bytes: &[],
            index: 0,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        match self {
            MixFrom::Owned { bytes, index: _ } => bytes.is_empty(),
            MixFrom::Slice { bytes, index: _ } => bytes.is_empty(),
            _ => false,
        }
    }

    pub(crate) fn set_owned(&mut self, bytes: Vec<u8>) {
        *self = MixFrom::Owned { bytes, index: 0 };
    }

    pub(crate) fn set_bytes(&mut self, bytes: &'a [u8]) {
        *self = MixFrom::Slice { bytes, index: 0 };
    }

    pub(crate) fn set_reader<T>(&mut self, data: T)
    where
        T: Read + Send + Sync + 'static,
    {
        *self = MixFrom::Reader(Box::new(data));
    }

    pub(crate) fn set_async_reader<T>(&mut self, data: T)
    where
        T: AsyncRead + Send + Sync + Unpin + 'static,
    {
        *self = MixFrom::AsyncReader(Box::new(data));
    }

    fn owned_encode(&mut self, dst: &mut [u8]) -> SizeResult {
        if let MixFrom::Owned { bytes, index } = self {
            match data_copy(bytes, index, dst)? {
                TokenStatus::Partial(size) => Ok(size),
                TokenStatus::Complete(size) => Ok(size),
            }
        } else {
            Ok(0)
        }
    }

    fn bytes_encode(&mut self, dst: &mut [u8]) -> SizeResult {
        if let MixFrom::Slice { bytes, index } = self {
            match data_copy(bytes, index, dst)? {
                TokenStatus::Partial(size) => Ok(size),
                TokenStatus::Complete(size) => Ok(size),
            }
        } else {
            Ok(0)
        }
    }

    fn read_encode(&mut self, dst: &mut [u8]) -> SizeResult {
        if let MixFrom::Reader(data) = self {
            let size = data.read(dst)?;
            Ok(size)
        } else {
            Ok(0)
        }
    }
}

impl Default for MixFrom<'_> {
    fn default() -> Self {
        MixFrom::new()
    }
}

impl Debug for MixFrom<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Owned { bytes, index } => f
                .debug_struct("Owned")
                .field("bytes", bytes)
                .field("index", index)
                .finish(),
            Self::Slice { bytes, index } => f
                .debug_struct("Slice")
                .field("bytes", bytes)
                .field("index", index)
                .finish(),
            Self::Reader(_) => f.debug_tuple("Reader").finish(),
            Self::AsyncReader(_) => f.debug_tuple("AsyncReader").finish(),
        }
    }
}

// It is not a complete implementation, only implements for MixFrom::Owned &&
// MixFrom::Slice.
impl PartialEq for MixFrom<'_> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Owned {
                    bytes: l_bytes,
                    index: l_index,
                },
                Self::Owned {
                    bytes: r_bytes,
                    index: r_index,
                },
            ) => l_bytes == r_bytes && l_index == r_index,
            (
                Self::Slice {
                    bytes: l_bytes,
                    index: l_index,
                },
                Self::Slice {
                    bytes: r_bytes,
                    index: r_index,
                },
            ) => l_bytes == r_bytes && l_index == r_index,
            // Dyn trait object can not impl PartialEq
            (Self::Reader(_l0), Self::Reader(_r0)) => false,
            (Self::AsyncReader(_l0), Self::AsyncReader(_r0)) => false,
            _ => false,
        }
    }
}

impl sync_impl::Body for MixFrom<'_> {
    type Error = std::io::Error;

    fn data(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }

        match self {
            MixFrom::Owned { bytes: _, index: _ } => self.owned_encode(buf),
            MixFrom::Slice { bytes: _, index: _ } => self.bytes_encode(buf),
            MixFrom::Reader(_) => self.read_encode(buf),
            MixFrom::AsyncReader(_) => Ok(0),
        }
    }
}

impl async_impl::Body for MixFrom<'_> {
    type Error = std::io::Error;

    fn poll_data(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, Self::Error>> {
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }

        match &mut *self {
            MixFrom::Owned { bytes: _, index: _ } => Poll::Ready(self.owned_encode(buf)),
            MixFrom::Slice { bytes: _, index: _ } => Poll::Ready(self.bytes_encode(buf)),
            MixFrom::Reader(_) => Poll::Ready(Ok(0)),
            MixFrom::AsyncReader(data) => {
                let mut buf = ReadBuf::new(buf);
                match Pin::new(data).poll_read(cx, &mut buf) {
                    Poll::Ready(Ok(())) => Poll::Ready(Ok(buf.filled().len())),
                    Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                    Poll::Pending => Poll::Pending,
                }
            }
        }
    }
}

#[cfg(test)]
mod ut_mix {
    use crate::body::mime::common::mix::MixFrom;
    use crate::body::{async_impl, sync_impl};

    /// Builds a `MixFrom`.
    macro_rules! mix_build {
        (
            MixFrom: {
                $(BodyOwned: $body1: expr,)?
                $(BodySlice: $body2: expr,)?
                $(BodyReader: $body3: expr,)?
                $(BodyAsyncReader: $body4: expr,)?
            },
        ) => {
            {
                #[allow(unused_mut)]
                let mut mix = MixFrom::new_as_bytes();

                $(mix.set_owned($body1);)?
                $(mix.set_bytes($body2);)?
                $(mix.set_reader($body3);)?
                $(mix.set_async_reader($body4);)?
                mix
            }
        }
    }

    /// Builds a `Mimepart`, encodes it, Compares with result.
    macro_rules! mix_encode_compare {
        (
            MixFrom: {
                $(BodyOwned: $body1: expr,)?
                $(BodySlice: $body2: expr,)?
                $(BodyReader: $body3: expr,)?
                $(BodyAsyncReader: $body4: expr,)?
            },
            $(BufSize: $size: expr,)?
            $(ResultSize: $v_size: expr,)?
            $(ResultData: $v_data: expr,)?
            Sync,
        ) => {
            let mut mix = mix_build!(
                MixFrom: {
                    $(BodyOwned: $body1,)?
                    $(BodySlice: $body2,)?
                    $(BodyReader: $body3,)?
                    $(BodyAsyncReader: $body4,)?
                },
            );
            // default 1
            #[allow(unused_assignments, unused_mut)]
            let mut len = 1;

            $(len = $size;)?
            let mut buf = vec![0u8; len];
            let mut v_data = vec![];
            let mut v_size = vec![];

            loop {
                let size = sync_impl::Body::data(&mut mix, &mut buf).expect("MixFrom encode failed");
                if size == 0 {
                    break;
                }
                v_size.push(size);
                v_data.extend_from_slice(&buf[..size]);
            }
            $(assert_eq!(v_size, $v_size);)?
            $(assert_eq!(v_data, $v_data);)?
        };

        (
            MixFrom: {
                $(BodyOwned: $body1: expr,)?
                $(BodySlice: $body2: expr,)?
                $(BodyReader: $body3: expr,)?
                $(BodyAsyncReader: $body4: expr,)?
            },
            $(BufSize: $size: expr,)?
            $(ResultSize: $v_size: expr,)?
            $(ResultData: $v_data: expr,)?
            Async,
        ) => {
            let mut mix = mix_build!(
                MixFrom: {
                    $(BodyOwned: $body1,)?
                    $(BodySlice: $body2,)?
                    $(BodyReader: $body3,)?
                    $(BodyAsyncReader: $body4,)?
                },
            );

            // default 1
            #[allow(unused_assignments, unused_mut)]
            let mut len = 1;

            $(len = $size;)?
            let mut buf = vec![0u8; len];
            let mut v_data = vec![];
            let mut v_size = vec![];

            loop {
                let size = async_impl::Body::data(&mut mix, &mut buf).await.expect("MixFrom encode failed");
                if size == 0 {
                    break;
                }
                v_size.push(size);
                v_data.extend_from_slice(&buf[..size]);
            }
            $(assert_eq!(v_size, $v_size);)?
            $(assert_eq!(v_data, $v_data);)?
        };
    }

    /// UT test cases for `MixFrom::new`.
    ///
    /// # Brief
    /// 1. Creates a `MixFrom` by `MixFrom::new`.
    /// 2. Checks whether the result is correct.
    #[test]
    fn ut_mix_new() {
        mix_encode_compare!(
            MixFrom: {
            },
            ResultData: b"",
            Sync,
        );
    }

    /// UT test cases for `MixFrom::set_owned`.
    ///
    /// # Brief
    /// 1. Creates a `MixFrom` from Vec<u8> by `MixFrom::set_owned`.
    /// 2. Checks whether the result is correct.
    #[test]
    fn ut_mix_set_owned() {
        mix_encode_compare!(
            MixFrom: {
                BodyOwned: b"123456".to_vec(),
            },
            BufSize: 5,
            ResultSize: vec![5, 1],
            ResultData: b"123456",
            Sync,
        );
    }

    /// UT test cases for `MixFrom::set_bytes`.
    ///
    /// # Brief
    /// 1. Creates a `MixFrom` from bytes by `MixFrom::set_bytes`.
    /// 2. Checks whether the result is correct.
    #[test]
    fn ut_mix_set_bytes() {
        mix_encode_compare!(
            MixFrom: {
                BodySlice: b"123456",
            },
            ResultData: b"123456",
            Sync,
        );
    }

    /// UT test cases for `MixFrom::set_reader`.
    ///
    /// # Brief
    /// 1. Creates a `MixFrom` from synchronous read content by
    ///    `MixFrom::set_reader`.
    /// 2. Checks whether the result is correct.
    #[test]
    fn ut_mix_set_reader() {
        mix_encode_compare!(
            MixFrom: {
                BodyReader: "12345678".as_bytes(),
            },
            ResultData: b"12345678",
            Sync,
        );
    }

    /// UT test cases for `MixFrom::set_async_reader`.
    ///
    /// # Brief
    /// 1. Creates a `MixFrom` from asynchronous read content by
    ///    `MixFrom::set_async_reader`.
    /// 2. Encodes by synchronous encoding.
    /// 3. Checks whether the result is correct.
    #[test]
    fn ut_mix_set_async_reader_then_sync_data() {
        mix_encode_compare!(
            MixFrom: {
                BodyAsyncReader: "123456".as_bytes(),
            },
            ResultData: b"",
            Sync,
        );
    }

    /// UT test cases for `MixFrom::set_async_reader`.
    ///
    /// # Brief
    /// 1. Creates a `MixFrom` from asynchronous read content by
    ///    `MixFrom::set_async_reader`.
    /// 2. Encodes by asynchronous encoding.
    /// 3. Checks whether the result is correct.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_mix_set_async_reader() {
        let handle = ylong_runtime::spawn(async move {
            mix_set_async_reader().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    #[cfg(feature = "ylong_base")]
    async fn mix_set_async_reader() {
        mix_encode_compare!(
            MixFrom: {
                BodyAsyncReader: "12345678".as_bytes(),
            },
            ResultData: b"12345678",
            Async,
        );
    }

    /// UT test cases for `MixFrom::set_reader`.
    ///
    /// # Brief
    /// 1. Creates a `MixFrom` from synchronous read content by
    ///    `MixFrom::set_reader`.
    /// 2. Encodes by asynchronous encoding.
    /// 3. Checks whether the result is correct.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_mix_set_reader_then_async_data() {
        let handle = ylong_runtime::spawn(async move {
            mix_set_reader_then_async_data().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    #[cfg(feature = "ylong_base")]
    async fn mix_set_reader_then_async_data() {
        mix_encode_compare!(
            MixFrom: {
                BodyReader: "12345678".as_bytes(),
            },
            ResultData: b"",
            Async,
        );
    }

    /// UT test cases for `MixFrom::set_bytes`.
    ///
    /// # Brief
    /// 1. Creates a `MixFrom` from synchronous read content by
    ///    `MixFrom::set_bytes`.
    /// 2. Encodes by asynchronous encoding.
    /// 3. Checks whether the result is correct.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_mix_set_bytes_then_async_data() {
        let handle = ylong_runtime::spawn(async move {
            mix_set_bytes_then_async_data().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    #[cfg(feature = "ylong_base")]
    async fn mix_set_bytes_then_async_data() {
        mix_encode_compare!(
            MixFrom: {
                BodyReader: "12345678".as_bytes(),
            },
            ResultData: b"",
            Async,
        );
    }

    /// UT test cases for `MixFrom::set_owned`.
    ///
    /// # Brief
    /// 1. Creates a `MixFrom` from synchronous read content by
    ///    `MixFrom::set_owned`.
    /// 2. Encodes by asynchronous encoding.
    /// 3. Checks whether the result is correct.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_mix_set_owned_then_async_data() {
        let handle = ylong_runtime::spawn(async move {
            mix_set_owned_then_async_data().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    #[cfg(feature = "ylong_base")]
    async fn mix_set_owned_then_async_data() {
        mix_encode_compare!(
            MixFrom: {
                BodyOwned: b"12345678".to_vec(),
            },
            ResultData: b"12345678",
            Async,
        );
    }
}
