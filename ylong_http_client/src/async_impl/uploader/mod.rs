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

mod builder;
mod operator;

use std::pin::Pin;
use std::task::{Context, Poll};

pub use builder::{UploaderBuilder, WantsReader};
pub use operator::{Console, UploadOperator};
use ylong_http::body::{MultiPart, MultiPartBase};

use crate::runtime::{AsyncRead, ReadBuf};

/// An uploader that can help you upload the request body.
///
/// An `Uploader` provides a template method for uploading a file or a slice and
/// needs to use a structure that implements [`UploadOperator`] trait to read
/// the file or the slice and convert it into request body.
///
/// The `UploadOperator` trait provides a [`progress`] method which is
/// responsible for progress display.
///
/// You only need to provide a structure that implements the `UploadOperator`
/// trait to complete the upload process.
///
/// A default structure `Console` which implements `UploadOperator` is
/// provided to show download message on console. You can use
/// `Uploader::console` to build a `Uploader` which based on it.
///
/// [`UploadOperator`]: UploadOperator
/// [`progress`]: UploadOperator::progress
///
/// # Examples
///
/// `Console`:
/// ```no_run
/// # use ylong_http_client::async_impl::Uploader;
///
/// // Creates a default `Uploader` that show progress on console.
/// let mut uploader = Uploader::console("HelloWorld".as_bytes());
/// ```
///
/// `Custom`:
/// ```no_run
/// # use std::pin::Pin;
/// # use std::task::{Context, Poll};
/// # use ylong_http_client::async_impl::{Uploader, UploadOperator, Response};
/// # use ylong_http_client::{SpeedLimit, Timeout};
/// # use ylong_http_client::HttpClientError;
///
/// # async fn upload_and_show_progress() {
/// // Customizes your own `UploadOperator`.
/// struct MyUploadOperator;
///
/// impl UploadOperator for MyUploadOperator {
///     fn poll_progress(
///         self: Pin<&mut Self>,
///         cx: &mut Context<'_>,
///         uploaded: u64,
///         total: Option<u64>,
///     ) -> Poll<Result<(), HttpClientError>> {
///         todo!()
///     }
/// }
///
/// // Creates a default `Uploader` based on `MyUploadOperator`.
/// // Configures your uploader by using `UploaderBuilder`.
/// let uploader = Uploader::builder()
///     .reader("HelloWorld".as_bytes())
///     .operator(MyUploadOperator)
///     .build();
/// # }
/// ```
pub struct Uploader<R, T> {
    reader: R,
    operator: T,
    config: UploadConfig,
    info: Option<UploadInfo>,
}

impl<R: AsyncRead + Unpin> Uploader<R, Console> {
    /// Creates an `Uploader` with a `Console` operator which displays process
    /// on console.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::async_impl::Uploader;
    ///
    /// let uploader = Uploader::console("HelloWorld".as_bytes());
    /// ```
    pub fn console(reader: R) -> Uploader<R, Console> {
        UploaderBuilder::new().reader(reader).console().build()
    }
}

impl Uploader<(), ()> {
    /// Creates an `UploaderBuilder` and configures uploader step by step.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ylong_http_client::async_impl::Uploader;
    ///
    /// let builder = Uploader::builder();
    /// ```
    pub fn builder() -> UploaderBuilder<WantsReader> {
        UploaderBuilder::new()
    }
}

impl<R, T> AsyncRead for Uploader<R, T>
where
    R: AsyncRead + Unpin,
    T: UploadOperator + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();

        if this.info.is_none() {
            this.info = Some(UploadInfo::new());
        }

        let info = this.info.as_mut().unwrap();

        match Pin::new(&mut this.operator).poll_progress(
            cx,
            info.uploaded_bytes,
            this.config.total_bytes,
        ) {
            Poll::Ready(Ok(())) => {}
            // TODO: Consider another way to handle error.
            Poll::Ready(Err(e)) => {
                return Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    Box::new(e),
                )))
            }
            Poll::Pending => return Poll::Pending,
        }

        Pin::new(&mut this.reader).poll_read(cx, buf)
    }
}

impl<T: UploadOperator + Unpin> MultiPartBase for Uploader<MultiPart, T> {
    fn multipart(&self) -> &MultiPart {
        &self.reader
    }
}

#[derive(Default)]
struct UploadConfig {
    total_bytes: Option<u64>,
}

struct UploadInfo {
    uploaded_bytes: u64,
}

impl UploadInfo {
    fn new() -> Self {
        Self { uploaded_bytes: 0 }
    }
}

#[cfg(all(test, feature = "ylong_base"))]
mod ut_uploader {
    use ylong_http::body::{MultiPart, Part};
    use ylong_runtime::io::AsyncRead;

    use crate::async_impl::uploader::{Context, Pin, Poll};
    use crate::async_impl::{UploadOperator, Uploader, UploaderBuilder};
    use crate::HttpClientError;

    /// UT test cases for `UploadOperator::data`.
    ///
    /// # Brief
    /// 1. Creates a `Uploader`.
    /// 2. Calls `data` method.
    /// 3. Checks if the result is correct.

    #[test]
    fn ut_upload() {
        let handle = ylong_runtime::spawn(async move {
            upload().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    async fn upload() {
        let mut uploader = Uploader::console("HelloWorld".as_bytes());
        let mut user_slice = [0_u8; 10];
        let mut output_vec = vec![];

        let mut size = user_slice.len();
        while size == user_slice.len() {
            let mut buf = ylong_runtime::io::ReadBuf::new(user_slice.as_mut_slice());
            ylong_runtime::futures::poll_fn(|cx| Pin::new(&mut uploader).poll_read(cx, &mut buf))
                .await
                .unwrap();
            size = buf.filled_len();
            output_vec.extend_from_slice(&user_slice[..size]);
        }
        assert_eq!(&output_vec, b"HelloWorld");

        let mut user_slice = [0_u8; 12];
        let multipart = MultiPart::new().part(Part::new().name("name").body("xiaoming"));
        let mut multi_uploader = UploaderBuilder::default()
            .multipart(multipart)
            .console()
            .build();
        let mut buf = ylong_runtime::io::ReadBuf::new(user_slice.as_mut_slice());
        ylong_runtime::futures::poll_fn(|cx| Pin::new(&mut multi_uploader).poll_read(cx, &mut buf))
            .await
            .unwrap();
        let size = buf.filled_len();
        assert_eq!(size, 12);
    }

    /// UT test cases for `UploadOperator::progress`.
    ///
    /// # Brief
    /// 1. Creates a `MyUploadOperator`.
    /// 2. Calls `progress` method.
    /// 3. Checks if the result is correct.
    #[test]
    fn ut_upload_op_cov() {
        let handle = ylong_runtime::spawn(async move {
            upload_op_cov().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    async fn upload_op_cov() {
        struct MyUploadOperator;
        impl UploadOperator for MyUploadOperator {
            fn poll_progress(
                self: Pin<&mut Self>,
                _cx: &mut Context<'_>,
                uploaded: u64,
                total: Option<u64>,
            ) -> Poll<Result<(), HttpClientError>> {
                if uploaded > total.unwrap() {
                    return Poll::Ready(err_from_msg!(BodyTransfer, "UploadOperator failed"));
                }
                Poll::Ready(Ok(()))
            }
        }
        let res = MyUploadOperator.progress(10, Some(20)).await;
        assert!(res.is_ok());
    }

    /// UT test cases for `Uploader::builder`.
    ///
    /// # Brief
    /// 1. Creates a `UploaderBuilder` by `Uploader::builder`.
    /// 2. Checks if the result is correct.

    #[test]
    fn ut_uploader_builder() {
        let handle = ylong_runtime::spawn(async { upload_and_show_progress().await });
        ylong_runtime::block_on(handle).unwrap();
    }

    async fn upload_and_show_progress() {
        // Customizes your own `UploadOperator`.
        struct MyUploadOperator;

        impl UploadOperator for MyUploadOperator {
            fn poll_progress(
                self: Pin<&mut Self>,
                _cx: &mut Context<'_>,
                _uploaded: u64,
                _total: Option<u64>,
            ) -> Poll<Result<(), HttpClientError>> {
                Poll::Ready(Err(HttpClientError::user_aborted()))
            }
        }

        // Creates a default `Uploader` based on `MyUploadOperator`.
        // Configures your uploader by using `UploaderBuilder`.
        let mut uploader = Uploader::builder()
            .reader("HelloWorld".as_bytes())
            .operator(MyUploadOperator)
            .build();

        let mut user_slice = [0_u8; 12];
        let mut buf = ylong_runtime::io::ReadBuf::new(user_slice.as_mut_slice());
        let res =
            ylong_runtime::futures::poll_fn(|cx| Pin::new(&mut uploader).poll_read(cx, &mut buf))
                .await;
        assert_eq!(
            format!("{:?}", res.err()),
            format!(
                "{:?}",
                Some(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    Box::new(HttpClientError::user_aborted())
                ))
            ),
        );
    }
}
