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

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use ylong_http::response::Response;

use crate::async_impl::HttpBody;
use crate::{ErrorKind, HttpClientError, Sleep};

pub(crate) struct TimeoutFuture<T> {
    pub(crate) timeout: Option<Pin<Box<Sleep>>>,
    pub(crate) future: T,
}

impl<T> Future for TimeoutFuture<T>
where
    T: Future<Output = Result<Response<HttpBody>, HttpClientError>> + Unpin,
{
    type Output = Result<Response<HttpBody>, HttpClientError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        if let Some(delay) = this.timeout.as_mut() {
            if let Poll::Ready(()) = delay.as_mut().poll(cx) {
                return Poll::Ready(Err(HttpClientError::new_with_message(
                    ErrorKind::Timeout,
                    "Request timeout",
                )));
            }
        }
        match Pin::new(&mut this.future).poll(cx) {
            Poll::Ready(Ok(mut response)) => {
                response.body_mut().set_sleep(this.timeout.take());
                Poll::Ready(Ok(response))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(all(test, feature = "ylong_base"))]
mod ut_timeout {
    use ylong_http::response::status::StatusCode;
    use ylong_http::response::{Response, ResponsePart};
    use ylong_http::version::Version;

    use crate::async_impl::timeout::TimeoutFuture;
    use crate::async_impl::HttpBody;
    use crate::util::normalizer::BodyLength;
    use crate::HttpClientError;

    /// UT test cases for `TimeoutFuture`.
    ///
    /// # Brief
    /// 1. Creates a `Future`.
    /// 2. Calls `ylong_runtime::block_on` to run the future.
    /// 3. Checks if result is correct.
    #[test]
    fn ut_timeout_future() {
        let future1 = Box::pin(async {
            let part = ResponsePart {
                version: Version::HTTP1_1,
                status: StatusCode::OK,
                headers: Default::default(),
            };
            let body = HttpBody::new(BodyLength::Empty, Box::new([].as_slice()), &[]).unwrap();
            Ok(Response::from_raw_parts(part, body))
        });

        let future2 = Box::pin(async {
            Result::<Response<HttpBody>, HttpClientError>::Err(HttpClientError::user_aborted())
        });

        let time_future1 = TimeoutFuture {
            timeout: None,
            future: future1,
        };

        let time_future2 = TimeoutFuture {
            timeout: None,
            future: future2,
        };

        assert!(ylong_runtime::block_on(time_future1).is_ok());
        assert!(ylong_runtime::block_on(time_future2).is_err());
    }
}
