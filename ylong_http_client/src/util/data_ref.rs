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

//! defines `BodyDataRef`.

use std::pin::Pin;
use std::task::{Context, Poll};

use crate::runtime::{AsyncRead, ReadBuf};
use crate::util::request::RequestArc;

pub(crate) struct BodyDataRef {
    body: Option<RequestArc>,
}

impl BodyDataRef {
    pub(crate) fn new(request: RequestArc) -> Self {
        Self {
            body: Some(request),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.body = None;
    }

    pub(crate) fn poll_read(
        &mut self,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Option<usize>> {
        let request = if let Some(ref mut request) = self.body {
            request
        } else {
            return Poll::Ready(Some(0));
        };
        let data = request.ref_mut().body_mut();
        let mut read_buf = ReadBuf::new(buf);
        let data = Pin::new(data);
        match data.poll_read(cx, &mut read_buf) {
            Poll::Ready(Err(_)) => Poll::Ready(None),
            Poll::Ready(Ok(_)) => Poll::Ready(Some(read_buf.filled().len())),
            Poll::Pending => Poll::Pending,
        }
    }
}
