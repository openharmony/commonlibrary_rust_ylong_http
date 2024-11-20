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
    ErrorCode, Frame, FrameDecoder, FrameKind, FramesIntoIter, H2Error, Payload, Setting,
};

use crate::runtime::{AsyncRead, BoundedSender, ReadBuf, ReadHalf, SendError};
use crate::util::dispatcher::http2::{
    DispatchErrorKind, OutputMessage, SettingsState, SettingsSync,
};

pub(crate) type OutputSendFut =
    Pin<Box<dyn Future<Output = Result<(), SendError<OutputMessage>>> + Send + Sync>>;

#[derive(Copy, Clone)]
enum DecodeState {
    Read,
    Send,
    Exit(DispatchErrorKind),
}

pub(crate) struct RecvData<S> {
    decoder: FrameDecoder,
    settings: Arc<Mutex<SettingsSync>>,
    reader: ReadHalf<S>,
    state: DecodeState,
    next_state: DecodeState,
    resp_tx: BoundedSender<OutputMessage>,
    curr_message: Option<OutputSendFut>,
    pending_iter: Option<FramesIntoIter>,
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
        resp_tx: BoundedSender<OutputMessage>,
    ) -> Self {
        Self {
            decoder,
            settings,
            reader,
            state: DecodeState::Read,
            next_state: DecodeState::Read,
            resp_tx,
            curr_message: None,
            pending_iter: None,
        }
    }

    fn poll_read_frame(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), DispatchErrorKind>> {
        let mut buf = [0u8; 1024];
        loop {
            match self.state {
                DecodeState::Read => {
                    let mut read_buf = ReadBuf::new(&mut buf);
                    match Pin::new(&mut self.reader).poll_read(cx, &mut read_buf) {
                        Poll::Ready(Err(e)) => {
                            return self.transmit_error(cx, e.into());
                        }
                        Poll::Ready(Ok(())) => {}
                        Poll::Pending => {
                            return Poll::Pending;
                        }
                    }
                    let read = read_buf.filled().len();
                    if read == 0 {
                        let _ = self.transmit_message(
                            cx,
                            OutputMessage::OutputExit(DispatchErrorKind::Disconnect),
                        );
                        self.state = DecodeState::Send;
                        return Poll::Pending;
                    }

                    match self.decoder.decode(&buf[..read]) {
                        Ok(frames) => match self.poll_iterator_frames(cx, frames.into_iter()) {
                            Poll::Ready(Ok(_)) => {}
                            Poll::Ready(Err(e)) => {
                                return Poll::Ready(Err(e));
                            }
                            Poll::Pending => {
                                self.next_state = DecodeState::Read;
                            }
                        },
                        Err(e) => {
                            match self.transmit_message(cx, OutputMessage::OutputExit(e.into())) {
                                Poll::Ready(Err(_)) => {
                                    return Poll::Ready(Err(DispatchErrorKind::ChannelClosed))
                                }
                                Poll::Ready(Ok(_)) => {}
                                Poll::Pending => {
                                    self.next_state = DecodeState::Read;
                                    return Poll::Pending;
                                }
                            }
                        }
                    }
                }
                DecodeState::Send => {
                    match self.poll_blocked_task(cx) {
                        Poll::Ready(Ok(_)) => {
                            self.state = self.next_state;
                            // Reset next state.
                            self.next_state = DecodeState::Read;
                        }
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                        Poll::Pending => return Poll::Pending,
                    }
                }
                DecodeState::Exit(e) => {
                    return Poll::Ready(Err(e));
                }
            }
        }
    }

    fn poll_blocked_task(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), DispatchErrorKind>> {
        if let Some(mut task) = self.curr_message.take() {
            match task.as_mut().poll(cx) {
                Poll::Ready(Ok(_)) => {}
                Poll::Ready(Err(_)) => {
                    return Poll::Ready(Err(DispatchErrorKind::ChannelClosed));
                }
                Poll::Pending => {
                    self.curr_message = Some(task);
                    return Poll::Pending;
                }
            }
        }

        if let Some(iter) = self.pending_iter.take() {
            return self.poll_iterator_frames(cx, iter);
        }
        Poll::Ready(Ok(()))
    }

    fn poll_iterator_frames(
        &mut self,
        cx: &mut Context<'_>,
        mut iter: FramesIntoIter,
    ) -> Poll<Result<(), DispatchErrorKind>> {
        while let Some(kind) = iter.next() {
            match kind {
                FrameKind::Complete(frame) => {
                    // TODO Whether to continue processing the remaining frames after connection
                    // error occurs in the Settings frame.
                    let message = if let Err(e) = self.update_settings(&frame) {
                        OutputMessage::OutputExit(DispatchErrorKind::H2(e))
                    } else {
                        OutputMessage::Output(frame)
                    };

                    match self.transmit_message(cx, message) {
                        Poll::Ready(Ok(_)) => {}
                        Poll::Ready(Err(e)) => {
                            return Poll::Ready(Err(e));
                        }
                        Poll::Pending => {
                            self.pending_iter = Some(iter);
                            return Poll::Pending;
                        }
                    }
                }
                FrameKind::Partial => {}
            }
        }
        Poll::Ready(Ok(()))
    }

    fn transmit_error(
        &mut self,
        cx: &mut Context<'_>,
        exit_err: DispatchErrorKind,
    ) -> Poll<Result<(), DispatchErrorKind>> {
        match self.transmit_message(cx, OutputMessage::OutputExit(exit_err)) {
            Poll::Ready(_) => Poll::Ready(Err(exit_err)),
            Poll::Pending => {
                self.next_state = DecodeState::Exit(exit_err);
                Poll::Pending
            }
        }
    }

    fn transmit_message(
        &mut self,
        cx: &mut Context<'_>,
        message: OutputMessage,
    ) -> Poll<Result<(), DispatchErrorKind>> {
        let mut task = {
            let sender = self.resp_tx.clone();
            let ft = async move { sender.send(message).await };
            Box::pin(ft)
        };

        match task.as_mut().poll(cx) {
            Poll::Ready(Ok(_)) => Poll::Ready(Ok(())),
            // The current coroutine sending the request exited prematurely.
            Poll::Ready(Err(_)) => Poll::Ready(Err(DispatchErrorKind::ChannelClosed)),
            Poll::Pending => {
                self.state = DecodeState::Send;
                self.curr_message = Some(task);
                Poll::Pending
            }
        }
    }

    fn update_settings(&mut self, frame: &Frame) -> Result<(), H2Error> {
        if let Payload::Settings(_settings) = frame.payload() {
            if frame.flags().is_ack() {
                self.update_decoder_settings()?;
            }
        }
        Ok(())
    }

    fn update_decoder_settings(&mut self) -> Result<(), H2Error> {
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
                Ok(())
            }
            SettingsState::Synced => Err(H2Error::ConnectionError(ErrorCode::ConnectError)),
        }
    }
}
