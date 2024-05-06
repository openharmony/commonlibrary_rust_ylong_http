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

//! Frame recv coroutine.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use ylong_http::h2::{
    ErrorCode, Frame, FrameDecoder, FrameKind, Frames, H2Error, Payload, Setting,
};

use crate::runtime::{AsyncRead, ReadBuf, ReadHalf, UnboundedSender};
use crate::util::dispatcher::http2::{
    DispatchErrorKind, OutputMessage, SettingsState, SettingsSync,
};

pub(crate) struct RecvData<S> {
    decoder: FrameDecoder,
    settings: Arc<Mutex<SettingsSync>>,
    reader: ReadHalf<S>,
    resp_tx: UnboundedSender<OutputMessage>,
}

impl<S: AsyncRead + Unpin + Sync + Send + 'static> Future for RecvData<S> {
    type Output = Result<(), DispatchErrorKind>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let receiver = self.get_mut();
        receiver.poll_read_frame(cx)
    }
}

impl<S: AsyncRead + Unpin + Sync + Send + 'static> RecvData<S> {
    pub(crate) fn new(
        decoder: FrameDecoder,
        settings: Arc<Mutex<SettingsSync>>,
        reader: ReadHalf<S>,
        resp_tx: UnboundedSender<OutputMessage>,
    ) -> Self {
        Self {
            decoder,
            settings,
            reader,
            resp_tx,
        }
    }

    fn poll_read_frame(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), DispatchErrorKind>> {
        let mut buf = [0u8; 1024];
        loop {
            let mut read_buf = ReadBuf::new(&mut buf);
            match Pin::new(&mut self.reader).poll_read(cx, &mut read_buf) {
                Poll::Ready(Err(e)) => {
                    self.transmit_error(DispatchErrorKind::Disconnect)?;
                    return Poll::Ready(Err(e.into()));
                }
                Poll::Ready(Ok(())) => {}
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
            let read = read_buf.filled().len();
            if read == 0 {
                self.transmit_error(DispatchErrorKind::Disconnect)?;
                return Poll::Ready(Err(DispatchErrorKind::Disconnect));
            }

            match self.decoder.decode(&buf[..read]) {
                Ok(frames) => match self.transmit_frame(frames) {
                    Ok(_) => {}
                    Err(DispatchErrorKind::H2(e)) => {
                        self.transmit_error(e.into())?;
                    }
                    Err(e) => {
                        return Poll::Ready(Err(e));
                    }
                },
                Err(e) => {
                    self.transmit_error(e.into())?;
                }
            }
        }
    }

    fn transmit_frame(&mut self, frames: Frames) -> Result<(), DispatchErrorKind> {
        for kind in frames.into_iter() {
            match kind {
                FrameKind::Complete(frame) => {
                    self.update_settings(&frame)?;
                    self.resp_tx
                        .send(OutputMessage::Output(frame))
                        .map_err(|_e| DispatchErrorKind::ChannelClosed)?;
                }
                FrameKind::Partial => {}
            }
        }
        Ok(())
    }

    fn transmit_error(&self, err: DispatchErrorKind) -> Result<(), DispatchErrorKind> {
        self.resp_tx
            .send(OutputMessage::OutputExit(err))
            .map_err(|_e| DispatchErrorKind::ChannelClosed)
    }

    fn update_settings(&mut self, frame: &Frame) -> Result<(), H2Error> {
        if let Payload::Settings(_settings) = frame.payload() {
            if frame.flags().is_ack() {
                {
                    let connection = self.settings.lock().unwrap();
                    match &connection.settings {
                        SettingsState::Acknowledging(settings) => {
                            for setting in settings.get_settings() {
                                if let Setting::MaxHeaderListSize(size) = setting {
                                    self.decoder.set_max_header_list_size(*size as usize);
                                }
                                if let Setting::MaxFrameSize(size) = setting {
                                    self.decoder.set_max_frame_size(*size)?;
                                }
                            }
                        }
                        SettingsState::Synced => {
                            return Err(H2Error::ConnectionError(ErrorCode::ConnectError))
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
