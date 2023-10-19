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

//! Frame send coroutine.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use ylong_http::h2::{ErrorCode, Frame, FrameEncoder, H2Error, Payload, Setting, Settings};

use crate::runtime::{AsyncWrite, UnboundedReceiver, WriteHalf};
use crate::util::dispatcher::http2::{DispatchErrorKind, SettingsState, SettingsSync};

pub(crate) struct SendData<S> {
    encoder: FrameEncoder,
    settings: Arc<Mutex<SettingsSync>>,
    writer: WriteHalf<S>,
    req_rx: UnboundedReceiver<Frame>,
    state: InputState,
    buf: WriteBuf,
}

enum InputState {
    RecvFrame,
    WriteFrame,
}

enum SettingState {
    Not,
    Local(Settings),
    Ack,
}

pub(crate) struct WriteBuf {
    buf: [u8; 1024],
    end: usize,
    start: usize,
    empty: bool,
}

impl WriteBuf {
    pub(crate) fn new() -> Self {
        Self {
            buf: [0; 1024],
            end: 0,
            start: 0,
            empty: true,
        }
    }
    pub(crate) fn clear(&mut self) {
        self.start = 0;
        self.end = 0;
        self.empty = true;
    }
}

impl<S: AsyncWrite + Unpin + Sync + Send + 'static> Future for SendData<S> {
    type Output = Result<(), DispatchErrorKind>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let sender = self.get_mut();
        loop {
            match sender.state {
                InputState::RecvFrame => {
                    let frame = match sender.poll_recv_frame(cx) {
                        Poll::Ready(Ok(frame)) => frame,
                        Poll::Ready(Err(e)) => {
                            // Errors in the Frame Writer are thrown directly to exit the coroutine.
                            return Poll::Ready(Err(e));
                        }
                        Poll::Pending => return Poll::Pending,
                    };

                    let state = sender.update_settings(&frame);

                    if let SettingState::Local(setting) = &state {
                        let mut sync = sender.settings.lock().unwrap();
                        sync.settings = SettingsState::Acknowledging(setting.clone());
                    }

                    let frame = if let SettingState::Ack = state {
                        Settings::ack()
                    } else {
                        frame
                    };
                    sender.encoder.set_frame(frame);
                    sender.state = InputState::WriteFrame;
                }
                InputState::WriteFrame => {
                    match sender.poll_writer_frame(cx) {
                        Poll::Ready(_) => {}
                        Poll::Pending => return Poll::Pending,
                    };
                    sender.state = InputState::RecvFrame;
                }
            }
        }
    }
}

impl<S: AsyncWrite + Unpin + Sync + Send + 'static> SendData<S> {
    pub(crate) fn new(
        encoder: FrameEncoder,
        settings: Arc<Mutex<SettingsSync>>,
        writer: WriteHalf<S>,
        req_rx: UnboundedReceiver<Frame>,
    ) -> Self {
        Self {
            encoder,
            settings,
            writer,
            req_rx,
            state: InputState::RecvFrame,
            buf: WriteBuf::new(),
        }
    }

    // io write interface
    fn poll_writer_frame(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), DispatchErrorKind>> {
        if !self.buf.empty {
            loop {
                match Pin::new(&mut self.writer)
                    .poll_write(cx, &self.buf.buf[self.buf.start..self.buf.end])
                    .map_err(|e| DispatchErrorKind::Io(e.kind()))?
                {
                    Poll::Ready(written) => {
                        self.buf.start += written;
                        if self.buf.start == self.buf.end {
                            self.buf.clear();
                            break;
                        }
                    }
                    Poll::Pending => {
                        return Poll::Pending;
                    }
                }
            }
        }

        loop {
            let size = self.encoder.encode(&mut self.buf.buf).map_err(|_| {
                DispatchErrorKind::H2(H2Error::ConnectionError(ErrorCode::IntervalError))
            })?;

            if size == 0 {
                break;
            }
            let mut index = 0;

            loop {
                match Pin::new(&mut self.writer)
                    .poll_write(cx, &self.buf.buf[index..size])
                    .map_err(|e| DispatchErrorKind::Io(e.kind()))?
                {
                    Poll::Ready(written) => {
                        index += written;
                        if index == size {
                            break;
                        }
                    }
                    Poll::Pending => {
                        self.buf.start = index;
                        self.buf.end = size;
                        self.buf.empty = false;
                        return Poll::Pending;
                    }
                }
            }
        }
        Poll::Ready(Ok(()))
    }

    // io write interface
    fn poll_recv_frame(&mut self, cx: &mut Context<'_>) -> Poll<Result<Frame, DispatchErrorKind>> {
        #[cfg(feature = "tokio_base")]
        match self.req_rx.poll_recv(cx) {
            Poll::Ready(None) => Poll::Ready(Err(DispatchErrorKind::ChannelClosed)),
            Poll::Ready(Some(frame)) => Poll::Ready(Ok(frame)),
            Poll::Pending => Poll::Pending,
        }
        #[cfg(feature = "ylong_base")]
        match self.req_rx.poll_recv(cx) {
            Poll::Ready(Err(_e)) => Poll::Ready(Err(DispatchErrorKind::ChannelClosed)),
            Poll::Ready(Ok(frame)) => Poll::Ready(Ok(frame)),
            Poll::Pending => Poll::Pending,
        }
    }

    fn update_settings(&mut self, frame: &Frame) -> SettingState {
        if let Payload::Settings(settings) = frame.payload() {
            // The ack in Writer is sent from the client to the server to confirm the
            // Settings of the encoder on the client. The ack in Reader is sent
            // from the server to the client to confirm the Settings of the decoder on the
            // client
            return if frame.flags().is_ack() {
                for setting in settings.get_settings() {
                    if let Setting::HeaderTableSize(size) = setting {
                        self.encoder.update_header_table_size(*size as usize);
                    }
                    if let Setting::MaxFrameSize(size) = setting {
                        self.encoder.update_max_frame_size(*size as usize);
                    }
                }
                SettingState::Ack
            } else {
                SettingState::Local(settings.clone())
            };
        }
        SettingState::Not
    }
}
