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
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use ylong_runtime::time::{sleep, Sleep};

use crate::async_impl::QuicConn;
use crate::runtime::{AsyncRead, AsyncWrite, ReadBuf, UnboundedReceiver, UnboundedSender};
use crate::util::dispatcher::http3::DispatchErrorKind;
use crate::util::h3::stream_manager::UPD_RECV_BUF_SIZE;
use crate::util::ConnInfo;

const UDP_SEND_BUF_SIZE: usize = 1350;

enum IOManagerState {
    IORecving,
    Timeout,
    IOSending,
    ChannelRecving,
}

pub(crate) struct IOManager<S> {
    io: S,
    conn: Arc<Mutex<QuicConn>>,
    io_manager_rx: UnboundedReceiver<Result<(), DispatchErrorKind>>,
    stream_manager_tx: UnboundedSender<Result<(), DispatchErrorKind>>,
    recv_timeout: Option<Pin<Box<Sleep>>>,
    state: IOManagerState,
    recv_buf: [u8; UPD_RECV_BUF_SIZE],
    send_data: SendData,
}

impl<S: AsyncRead + AsyncWrite + ConnInfo + Unpin + Sync + Send + 'static> IOManager<S> {
    pub(crate) fn new(
        io: S,
        conn: Arc<Mutex<QuicConn>>,
        io_manager_rx: UnboundedReceiver<Result<(), DispatchErrorKind>>,
        stream_manager_tx: UnboundedSender<Result<(), DispatchErrorKind>>,
    ) -> Self {
        Self {
            io,
            conn,
            io_manager_rx,
            stream_manager_tx,
            recv_timeout: None,
            state: IOManagerState::IORecving,
            recv_buf: [0u8; UPD_RECV_BUF_SIZE],
            send_data: SendData::new(),
        }
    }
    fn poll_recv_signal(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Result<(), DispatchErrorKind>, DispatchErrorKind>> {
        #[cfg(feature = "tokio_base")]
        match self.io_manager_rx.poll_recv(cx) {
            Poll::Ready(None) => Poll::Ready(Err(DispatchErrorKind::ChannelClosed)),
            Poll::Ready(Some(data)) => Poll::Ready(Ok(data)),
            Poll::Pending => Poll::Pending,
        }
        #[cfg(feature = "ylong_base")]
        match self.io_manager_rx.poll_recv(cx) {
            Poll::Ready(Err(_e)) => Poll::Ready(Err(DispatchErrorKind::ChannelClosed)),
            Poll::Ready(Ok(data)) => Poll::Ready(Ok(data)),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_io_recv(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), DispatchErrorKind>> {
        let mut buf = ReadBuf::new(&mut self.recv_buf);
        if self.recv_timeout.is_none() {
            if let Some(time) = self.conn.lock().unwrap().timeout() {
                self.recv_timeout = Some(Box::pin(sleep(time)));
            };
        }

        if let Some(delay) = self.recv_timeout.as_mut() {
            if let Poll::Ready(()) = delay.as_mut().poll(cx) {
                self.recv_timeout = None;
                self.conn.lock().unwrap().on_timeout();
                self.state = IOManagerState::Timeout;
                return Poll::Ready(Ok(()));
            }
        }
        match Pin::new(&mut self.io).poll_read(cx, &mut buf) {
            Poll::Ready(Ok(())) => {
                let info = self.io.conn_data().detail();
                self.recv_timeout = None;
                let recv_info = quiche::RecvInfo {
                    to: info.local,
                    from: info.peer,
                };
                return match self.conn.lock().unwrap().recv(buf.filled_mut(), recv_info) {
                    Ok(_) => {
                        let _ = self.stream_manager_tx.send(Ok(()));
                        // io recv once again
                        Poll::Ready(Ok(()))
                    }
                    Err(e) => Poll::Ready(Err(DispatchErrorKind::Quic(e))),
                };
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(DispatchErrorKind::Io(e.kind()))),
            Poll::Pending => {
                self.state = IOManagerState::IOSending;
                Poll::Pending
            }
        }
    }

    fn poll_io_send(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), DispatchErrorKind>> {
        loop {
            // UDP buf has not been sent to the peer, send rest UDP buf first
            if self.send_data.buf_size == self.send_data.offset {
                // Retrieve the data to be sent via UDP from the connection
                let size = match self.conn.lock().unwrap().send(&mut self.send_data.buf) {
                    Ok((size, _)) => size,
                    Err(quiche::Error::Done) => {
                        self.state = IOManagerState::ChannelRecving;
                        return Poll::Ready(Ok(()));
                    }
                    Err(e) => {
                        return Poll::Ready(Err(DispatchErrorKind::Quic(e)));
                    }
                };
                self.send_data.buf_size = size;
                self.send_data.offset = 0;
            }

            match Pin::new(&mut self.io).poll_write(
                cx,
                &self.send_data.buf[self.send_data.offset..self.send_data.buf_size],
            ) {
                Poll::Ready(Ok(size)) => {
                    self.send_data.offset += size;
                    if self.send_data.offset != self.send_data.buf_size {
                        // loop to send UDP buf
                        continue;
                    } else {
                        self.send_data.offset = 0;
                        self.send_data.buf_size = 0;
                    }
                }
                Poll::Ready(Err(e)) => {
                    return Poll::Ready(Err(DispatchErrorKind::Io(e.kind())));
                }
                Poll::Pending => {
                    self.state = IOManagerState::ChannelRecving;
                    return Poll::Pending;
                }
            }
        }
    }
}

impl<S: AsyncRead + AsyncWrite + ConnInfo + Unpin + Sync + Send + 'static> Future for IOManager<S> {
    type Output = Result<(), DispatchErrorKind>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        this.state = IOManagerState::IORecving;
        loop {
            match this.state {
                IOManagerState::IORecving => {
                    if let Poll::Ready(Err(e)) = this.poll_io_recv(cx) {
                        return Poll::Ready(Err(e));
                    }
                }
                IOManagerState::IOSending => {
                    if let Poll::Ready(Err(e)) = this.poll_io_send(cx) {
                        return Poll::Ready(Err(e));
                    }
                }
                IOManagerState::Timeout => {
                    if let Poll::Ready(Err(e)) = this.poll_io_send(cx) {
                        return Poll::Ready(Err(e));
                    }
                    // ensure pending at io recv
                    this.state = IOManagerState::IORecving;
                }
                IOManagerState::ChannelRecving => match this.poll_recv_signal(cx) {
                    // won't recv Err now
                    Poll::Ready(Ok(_)) => {
                        continue;
                    }
                    Poll::Ready(Err(e)) => {
                        return Poll::Ready(Err(e));
                    }
                    Poll::Pending => {
                        this.state = IOManagerState::IORecving;
                        return Poll::Pending;
                    }
                },
            }
        }
    }
}

pub(crate) struct SendData {
    pub(crate) buf: [u8; UDP_SEND_BUF_SIZE],
    pub(crate) buf_size: usize,
    pub(crate) offset: usize,
}

impl SendData {
    pub(crate) fn new() -> Self {
        Self {
            buf: [0u8; UDP_SEND_BUF_SIZE],
            buf_size: 0,
            offset: 0,
        }
    }
}
