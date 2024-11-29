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
use crate::util::progress::SpeedController;
use crate::util::request::RequestArc;
use crate::HttpClientError;

pub(crate) struct BodyDataRef {
    pub(crate) speed_controller: SpeedController,
    body: Option<RequestArc>,
}

impl BodyDataRef {
    pub(crate) fn new(request: RequestArc, speed_controller: SpeedController) -> Self {
        Self {
            speed_controller,
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
    ) -> Poll<Result<usize, HttpClientError>> {
        let request = if let Some(ref mut request) = self.body {
            request
        } else {
            return Poll::Ready(Ok(0));
        };
        self.speed_controller.init_min_send_if_not_start();
        if self
            .speed_controller
            .poll_max_send_delay_time(cx)
            .is_pending()
        {
            return Poll::Pending;
        }
        self.speed_controller.init_max_send_if_not_start();
        let data = request.ref_mut().body_mut();
        let mut read_buf = ReadBuf::new(buf);
        let data = Pin::new(data);
        match data.poll_read(cx, &mut read_buf) {
            Poll::Ready(Err(e)) => Poll::Ready(err_from_io!(BodyTransfer, e)),
            Poll::Ready(Ok(_)) => {
                let filled: usize = read_buf.filled().len();
                // Limit the write I/O speed by limiting the read file speed.
                self.speed_controller.min_send_speed_limit(filled)?;
                self.speed_controller.delay_max_send_speed_limit(filled);
                Poll::Ready(Ok(filled))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
