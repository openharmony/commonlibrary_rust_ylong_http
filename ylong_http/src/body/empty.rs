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
use core::pin::Pin;
use core::task::{Context, Poll};

use crate::body::{async_impl, sync_impl};

/// An empty body, indicating that there is no body part in the message.
///
/// `EmptyBody` both implements [`sync_impl::Body`] and [`async_impl::Body`].
/// Using [`sync_impl::Body::data`] method or [`async_impl::Body::data`] method
/// has no effect on buf and always returns `Ok(0)`.
///
/// [`sync_impl::Body`]: sync_impl::Body
/// [`async_impl::Body`]: async_impl::Body
/// [`sync_impl::Body::data`]: sync_impl::Body::data
/// [`async_impl::Body::data`]: async_impl::Body::data
///
/// # Examples
///
/// sync_impl:
///
/// ```
/// use ylong_http::body::sync_impl::Body;
/// use ylong_http::body::EmptyBody;
///
/// let mut body = EmptyBody::new();
/// let mut buf = [0u8; 1024];
///
/// // EmptyBody has no body data.
/// assert_eq!(body.data(&mut buf), Ok(0));
/// ```
///
/// async_impl:
///
/// ```
/// use ylong_http::body::async_impl::Body;
/// use ylong_http::body::EmptyBody;
///
/// # async fn read_empty_body() {
/// let mut body = EmptyBody::new();
/// let mut buf = [0u8; 1024];
///
/// // EmptyBody has no body data.
/// assert_eq!(body.data(&mut buf).await, Ok(0));
/// # }
/// ```
pub struct EmptyBody;

impl EmptyBody {
    /// Creates a new `EmptyBody`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http::body::EmptyBody;
    ///
    /// let body = EmptyBody::new();
    /// ```
    pub fn new() -> Self {
        Self
    }
}

impl Default for EmptyBody {
    fn default() -> Self {
        Self::new()
    }
}

impl sync_impl::Body for EmptyBody {
    type Error = Infallible;

    fn data(&mut self, _buf: &mut [u8]) -> Result<usize, Self::Error> {
        Ok(0)
    }
}

impl async_impl::Body for EmptyBody {
    type Error = Infallible;

    fn poll_data(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut [u8],
    ) -> Poll<Result<usize, Self::Error>> {
        Poll::Ready(Ok(0))
    }
}

#[cfg(test)]
mod ut_empty {
    use crate::body::empty::EmptyBody;

    /// UT test cases for `EmptyBody::new`.
    ///
    /// # Brief
    /// 1. Calls `EmptyBody::new()` to create an `EmptyBody`.
    #[test]
    fn ut_empty_body_new() {
        let _body = EmptyBody::new();
        // Success if no panic.
    }

    /// UT test cases for `sync_impl::Body::data` of `EmptyBody`.
    ///
    /// # Brief
    /// 1. Creates an `EmptyBody`.
    /// 2. Calls its `sync_impl::Body::data` method and then checks the results.
    #[test]
    fn ut_empty_body_sync_impl_data() {
        use crate::body::sync_impl::Body;

        let mut body = EmptyBody::new();
        let mut buf = [0u8; 1];
        assert_eq!(body.data(&mut buf), Ok(0));
    }

    /// UT test cases for `async_impl::Body::data` of `EmptyBody`.
    ///
    /// # Brief
    /// 1. Creates an `EmptyBody`.
    /// 2. Calls its `async_impl::Body::data` method and then checks the
    ///    results.
    #[cfg(feature = "ylong_base")]
    #[test]
    fn ut_empty_body_async_impl_data() {
        let handle = ylong_runtime::spawn(async move {
            empty_body_async_impl_data().await;
        });
        ylong_runtime::block_on(handle).unwrap();
    }

    #[cfg(feature = "ylong_base")]
    async fn empty_body_async_impl_data() {
        use crate::body::async_impl::Body;

        let mut body = EmptyBody::new();
        let mut buf = [0u8; 1];
        assert_eq!(body.data(&mut buf).await, Ok(0));
    }
}
