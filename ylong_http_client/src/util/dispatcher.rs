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

use crate::util::ConnInfo;
use crate::{ConnDetail, TimeGroup};

pub(crate) trait Dispatcher {
    type Handle;

    fn dispatch(&self) -> Option<Self::Handle>;

    fn is_shutdown(&self) -> bool;

    #[allow(dead_code)]
    fn is_goaway(&self) -> bool;
}

pub(crate) enum ConnDispatcher<S> {
    #[cfg(feature = "http1_1")]
    Http1(http1::Http1Dispatcher<S>),

    #[cfg(feature = "http2")]
    Http2(http2::Http2Dispatcher<S>),

    #[cfg(feature = "http3")]
    Http3(http3::Http3Dispatcher<S>),
}

impl<S> Dispatcher for ConnDispatcher<S> {
    type Handle = Conn<S>;

    fn dispatch(&self) -> Option<Self::Handle> {
        match self {
            #[cfg(feature = "http1_1")]
            Self::Http1(h1) => h1.dispatch().map(Conn::Http1),

            #[cfg(feature = "http2")]
            Self::Http2(h2) => h2.dispatch().map(Conn::Http2),

            #[cfg(feature = "http3")]
            Self::Http3(h3) => h3.dispatch().map(Conn::Http3),
        }
    }

    fn is_shutdown(&self) -> bool {
        match self {
            #[cfg(feature = "http1_1")]
            Self::Http1(h1) => h1.is_shutdown(),

            #[cfg(feature = "http2")]
            Self::Http2(h2) => h2.is_shutdown(),

            #[cfg(feature = "http3")]
            Self::Http3(h3) => h3.is_shutdown(),
        }
    }

    fn is_goaway(&self) -> bool {
        match self {
            #[cfg(feature = "http1_1")]
            Self::Http1(h1) => h1.is_goaway(),

            #[cfg(feature = "http2")]
            Self::Http2(h2) => h2.is_goaway(),

            #[cfg(feature = "http3")]
            Self::Http3(h3) => h3.is_goaway(),
        }
    }
}

pub(crate) enum Conn<S> {
    #[cfg(feature = "http1_1")]
    Http1(http1::Http1Conn<S>),

    #[cfg(feature = "http2")]
    Http2(http2::Http2Conn<S>),

    #[cfg(feature = "http3")]
    Http3(http3::Http3Conn<S>),
}

impl<S: ConnInfo> Conn<S> {
    pub(crate) fn get_detail(&mut self) -> ConnDetail {
        match self {
            #[cfg(feature = "http1_1")]
            Conn::Http1(io) => io.raw_mut().conn_data().detail(),
            #[cfg(feature = "http2")]
            Conn::Http2(io) => io.detail.clone(),
            #[cfg(feature = "http3")]
            Conn::Http3(io) => io.detail.clone(),
        }
    }
}

pub(crate) struct TimeInfoConn<S> {
    conn: Conn<S>,
    time_group: TimeGroup,
}

impl<S> TimeInfoConn<S> {
    pub(crate) fn new(conn: Conn<S>, time_group: TimeGroup) -> Self {
        Self { conn, time_group }
    }

    pub(crate) fn time_group_mut(&mut self) -> &mut TimeGroup {
        &mut self.time_group
    }

    pub(crate) fn time_group(&mut self) -> &TimeGroup {
        &self.time_group
    }

    pub(crate) fn connection(self) -> Conn<S> {
        self.conn
    }
}

#[cfg(feature = "http1_1")]
pub(crate) mod http1 {
    use std::cell::UnsafeCell;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use super::{ConnDispatcher, Dispatcher};
    use crate::runtime::Semaphore;
    #[cfg(feature = "tokio_base")]
    use crate::runtime::SemaphorePermit;
    use crate::util::progress::SpeedController;

    impl<S> ConnDispatcher<S> {
        pub(crate) fn http1(io: S) -> Self {
            Self::Http1(Http1Dispatcher::new(io))
        }
    }

    /// HTTP1-based connection manager, which can dispatch connections to other
    /// threads according to HTTP1 syntax.
    pub(crate) struct Http1Dispatcher<S> {
        inner: Arc<Inner<S>>,
    }

    pub(crate) struct Inner<S> {
        pub(crate) io: UnsafeCell<S>,
        // `occupied` indicates that the connection is occupied. Only one coroutine
        // can get the handle at the same time. Once the handle is fetched, the flag
        // position is true.
        pub(crate) occupied: AtomicBool,
        // `shutdown` indicates that the connection need to be shut down.
        pub(crate) shutdown: AtomicBool,
    }

    unsafe impl<S> Sync for Inner<S> {}

    impl<S> Http1Dispatcher<S> {
        pub(crate) fn new(io: S) -> Self {
            Self {
                inner: Arc::new(Inner {
                    io: UnsafeCell::new(io),
                    occupied: AtomicBool::new(false),
                    shutdown: AtomicBool::new(false),
                }),
            }
        }
    }

    impl<S> Dispatcher for Http1Dispatcher<S> {
        type Handle = Http1Conn<S>;

        fn dispatch(&self) -> Option<Self::Handle> {
            self.inner
                .occupied
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .ok()
                .map(|_| Http1Conn::from_inner(self.inner.clone()))
        }

        fn is_shutdown(&self) -> bool {
            self.inner.shutdown.load(Ordering::Relaxed)
        }

        fn is_goaway(&self) -> bool {
            false
        }
    }

    /// Handle returned to other threads for I/O operations.
    pub(crate) struct Http1Conn<S> {
        pub(crate) speed_controller: SpeedController,
        pub(crate) sem: Option<WrappedSemPermit>,
        pub(crate) inner: Arc<Inner<S>>,
    }

    impl<S> Http1Conn<S> {
        pub(crate) fn from_inner(inner: Arc<Inner<S>>) -> Self {
            Self {
                speed_controller: SpeedController::none(),
                sem: None,
                inner,
            }
        }

        pub(crate) fn occupy_sem(&mut self, sem: WrappedSemPermit) {
            self.sem = Some(sem);
        }

        pub(crate) fn raw_mut(&mut self) -> &mut S {
            // SAFETY: In the case of `HTTP1`, only one coroutine gets the handle
            // at the same time.
            unsafe { &mut *self.inner.io.get() }
        }

        pub(crate) fn shutdown(&self) {
            self.inner.shutdown.store(true, Ordering::Release);
        }

        pub(crate) fn cancel_guard(&self) -> CancelGuard<S> {
            CancelGuard {
                inner: self.inner.clone(),
                running: true,
            }
        }
    }

    impl<S> Drop for Http1Conn<S> {
        fn drop(&mut self) {
            self.inner.occupied.store(false, Ordering::Release)
        }
    }

    /// Http1 cancel guard
    pub(crate) struct CancelGuard<S> {
        inner: Arc<Inner<S>>,
        /// Default true
        running: bool,
    }

    impl<S> CancelGuard<S> {
        pub(crate) fn normal_end(&mut self) {
            self.running = false
        }
    }

    impl<S> Drop for CancelGuard<S> {
        fn drop(&mut self) {
            // When a drop occurs, if running is still true, it means a cancel has occurred,
            // and the IO needs to be shutdown to prevent the reuse of dirty data
            if self.running {
                self.inner.shutdown.store(true, Ordering::Release);
            }
        }
    }

    pub(crate) struct WrappedSemaphore {
        sem: Arc<Semaphore>,
    }

    impl WrappedSemaphore {
        pub(crate) fn new(permits: usize) -> Self {
            Self {
                #[cfg(feature = "tokio_base")]
                sem: Arc::new(tokio::sync::Semaphore::new(permits)),
                #[cfg(feature = "ylong_base")]
                sem: Arc::new(ylong_runtime::sync::Semaphore::new(permits).unwrap()),
            }
        }

        pub(crate) async fn acquire(&self) -> WrappedSemPermit {
            #[cfg(feature = "ylong_base")]
            {
                let semaphore = self.sem.clone();
                let _permit = semaphore.acquire().await.unwrap();
                WrappedSemPermit { sem: semaphore }
            }

            #[cfg(feature = "tokio_base")]
            {
                let permit = self.sem.clone().acquire_owned().await.unwrap();
                WrappedSemPermit { permit }
            }
        }
    }

    impl Clone for WrappedSemaphore {
        fn clone(&self) -> Self {
            Self {
                sem: self.sem.clone(),
            }
        }
    }

    pub(crate) struct WrappedSemPermit {
        #[cfg(feature = "ylong_base")]
        pub(crate) sem: Arc<Semaphore>,
        #[cfg(feature = "tokio_base")]
        #[allow(dead_code)]
        pub(crate) permit: SemaphorePermit,
    }

    #[cfg(feature = "ylong_base")]
    impl Drop for WrappedSemPermit {
        fn drop(&mut self) {
            self.sem.release();
        }
    }
}

#[cfg(feature = "http2")]
pub(crate) mod http2 {
    use std::collections::HashMap;
    use std::future::Future;
    use std::marker::PhantomData;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll};

    use ylong_http::error::HttpError;
    use ylong_http::h2::{
        ErrorCode, Frame, FrameDecoder, FrameEncoder, FrameFlags, Goaway, H2Error, Payload,
        RstStream, Settings, SettingsBuilder, StreamId,
    };

    use crate::runtime::{
        bounded_channel, unbounded_channel, AsyncRead, AsyncWrite, AsyncWriteExt, BoundedReceiver,
        BoundedSender, SendError, UnboundedReceiver, UnboundedSender, WriteHalf,
    };
    use crate::util::config::H2Config;
    use crate::util::dispatcher::{ConnDispatcher, Dispatcher};
    use crate::util::h2::{
        ConnManager, FlowControl, H2StreamState, RecvData, RequestWrapper, SendData,
        StreamEndState, Streams,
    };
    use crate::util::progress::SpeedController;
    use crate::ErrorKind::Request;
    use crate::{ConnDetail, ErrorKind, HttpClientError};

    const DEFAULT_MAX_FRAME_SIZE: usize = 2 << 13;
    const DEFAULT_WINDOW_SIZE: u32 = 65535;

    pub(crate) type ManagerSendFut =
        Pin<Box<dyn Future<Output = Result<(), SendError<RespMessage>>> + Send + Sync>>;

    pub(crate) enum RespMessage {
        Output(Frame),
        OutputExit(DispatchErrorKind),
    }

    pub(crate) enum OutputMessage {
        Output(Frame),
        OutputExit(DispatchErrorKind),
    }

    pub(crate) struct ReqMessage {
        pub(crate) sender: BoundedSender<RespMessage>,
        pub(crate) request: RequestWrapper,
    }

    #[derive(Debug, Eq, PartialEq, Copy, Clone)]
    pub(crate) enum DispatchErrorKind {
        H2(H2Error),
        Io(std::io::ErrorKind),
        ChannelClosed,
        Disconnect,
    }

    // HTTP2-based connection manager, which can dispatch connections to other
    // threads according to HTTP2 syntax.
    pub(crate) struct Http2Dispatcher<S> {
        pub(crate) detail: ConnDetail,
        pub(crate) allowed_cache: usize,
        pub(crate) sender: UnboundedSender<ReqMessage>,
        pub(crate) io_shutdown: Arc<AtomicBool>,
        pub(crate) io_goaway: Arc<AtomicBool>,
        pub(crate) handles: Vec<crate::runtime::JoinHandle<()>>,
        pub(crate) _mark: PhantomData<S>,
    }

    pub(crate) struct Http2Conn<S> {
        pub(crate) speed_controller: SpeedController,
        pub(crate) allow_cached_frames: usize,
        // Sends frame to StreamController
        pub(crate) sender: UnboundedSender<ReqMessage>,
        pub(crate) receiver: RespReceiver,
        pub(crate) io_shutdown: Arc<AtomicBool>,
        pub(crate) detail: ConnDetail,
        pub(crate) _mark: PhantomData<S>,
    }

    pub(crate) struct StreamController {
        // The connection close flag organizes new stream commits to the current connection when
        // closed.
        pub(crate) io_shutdown: Arc<AtomicBool>,
        pub(crate) io_goaway: Arc<AtomicBool>,
        // The senders of all connected stream channels of response.
        pub(crate) senders: HashMap<StreamId, BoundedSender<RespMessage>>,
        pub(crate) curr_message: HashMap<StreamId, ManagerSendFut>,
        // Stream information on the connection.
        pub(crate) streams: Streams,
        // Received GO_AWAY frame.
        pub(crate) go_away_error_code: Option<u32>,
        // The last GO_AWAY frame sent by the client.
        pub(crate) go_away_sync: GoAwaySync,
    }

    #[derive(Default)]
    pub(crate) struct GoAwaySync {
        pub(crate) going_away: Option<Goaway>,
    }

    #[derive(Default)]
    pub(crate) struct SettingsSync {
        pub(crate) settings: SettingsState,
    }

    #[derive(Default, Clone)]
    pub(crate) enum SettingsState {
        Acknowledging(Settings),
        #[default]
        Synced,
    }

    #[derive(Default)]
    pub(crate) struct RespReceiver {
        receiver: Option<BoundedReceiver<RespMessage>>,
    }

    impl<S> ConnDispatcher<S>
    where
        S: AsyncRead + AsyncWrite + Sync + Send + Unpin + 'static,
    {
        pub(crate) fn http2(detail: ConnDetail, config: H2Config, io: S) -> Self {
            Self::Http2(Http2Dispatcher::new(detail, config, io))
        }
    }

    impl<S> Http2Dispatcher<S>
    where
        S: AsyncRead + AsyncWrite + Sync + Send + Unpin + 'static,
    {
        pub(crate) fn new(detail: ConnDetail, config: H2Config, io: S) -> Self {
            let mut flow = FlowControl::new(DEFAULT_WINDOW_SIZE, DEFAULT_WINDOW_SIZE);
            flow.setup_recv_window(config.conn_window_size());

            let streams = Streams::new(config.stream_window_size(), DEFAULT_WINDOW_SIZE, flow);
            let shutdown_flag = Arc::new(AtomicBool::new(false));
            let goaway_flag = Arc::new(AtomicBool::new(false));
            let mut controller =
                StreamController::new(streams, shutdown_flag.clone(), goaway_flag.clone());

            let (input_tx, input_rx) = unbounded_channel();
            let (req_tx, req_rx) = unbounded_channel();

            let settings = create_initial_settings(&config);

            // Error is not possible, so it is not handled for the time
            // being.
            let mut handles = Vec::with_capacity(3);
            // send initial settings and update conn recv window
            if input_tx.send(settings).is_ok()
                && controller
                    .streams
                    .release_conn_recv_window(0, &input_tx)
                    .is_ok()
            {
                Self::launch(
                    config.allowed_cache_frame_size(),
                    config.use_huffman_coding(),
                    controller,
                    (input_tx, input_rx),
                    req_rx,
                    &mut handles,
                    io,
                );
            }
            Self {
                detail,
                allowed_cache: config.allowed_cache_frame_size(),
                sender: req_tx,
                io_shutdown: shutdown_flag,
                io_goaway: goaway_flag,
                handles,
                _mark: PhantomData,
            }
        }

        fn launch(
            allow_num: usize,
            use_huffman: bool,
            controller: StreamController,
            input_channel: (UnboundedSender<Frame>, UnboundedReceiver<Frame>),
            req_rx: UnboundedReceiver<ReqMessage>,
            handles: &mut Vec<crate::runtime::JoinHandle<()>>,
            io: S,
        ) {
            let (resp_tx, resp_rx) = bounded_channel(allow_num);
            let (read, write) = crate::runtime::split(io);
            let settings_sync = Arc::new(Mutex::new(SettingsSync::default()));
            let send_settings_sync = settings_sync.clone();
            let send = crate::runtime::spawn(async move {
                let mut writer = write;
                if async_send_preface(&mut writer).await.is_ok() {
                    let encoder = FrameEncoder::new(DEFAULT_MAX_FRAME_SIZE, use_huffman);
                    let mut send =
                        SendData::new(encoder, send_settings_sync, writer, input_channel.1);
                    let _ = Pin::new(&mut send).await;
                }
            });
            handles.push(send);

            let recv_settings_sync = settings_sync.clone();
            let recv = crate::runtime::spawn(async move {
                let decoder = FrameDecoder::new();
                let mut recv = RecvData::new(decoder, recv_settings_sync, read, resp_tx);
                let _ = Pin::new(&mut recv).await;
            });
            handles.push(recv);

            let manager = crate::runtime::spawn(async move {
                let mut conn_manager =
                    ConnManager::new(settings_sync, input_channel.0, resp_rx, req_rx, controller);
                let _ = Pin::new(&mut conn_manager).await;
            });
            handles.push(manager);
        }
    }

    impl<S> Dispatcher for Http2Dispatcher<S> {
        type Handle = Http2Conn<S>;

        fn dispatch(&self) -> Option<Self::Handle> {
            let sender = self.sender.clone();
            let handle = Http2Conn::new(
                self.allowed_cache,
                self.io_shutdown.clone(),
                sender,
                self.detail.clone(),
            );
            Some(handle)
        }

        fn is_shutdown(&self) -> bool {
            self.io_shutdown.load(Ordering::Relaxed)
        }

        fn is_goaway(&self) -> bool {
            self.io_goaway.load(Ordering::Relaxed)
        }
    }

    impl<S> Drop for Http2Dispatcher<S> {
        fn drop(&mut self) {
            for handle in &self.handles {
                #[cfg(feature = "ylong_base")]
                handle.cancel();
                #[cfg(feature = "tokio_base")]
                handle.abort();
            }
        }
    }

    impl<S> Http2Conn<S> {
        pub(crate) fn new(
            allow_cached_num: usize,
            io_shutdown: Arc<AtomicBool>,
            sender: UnboundedSender<ReqMessage>,
            detail: ConnDetail,
        ) -> Self {
            Self {
                speed_controller: SpeedController::none(),
                allow_cached_frames: allow_cached_num,
                sender,
                receiver: RespReceiver::default(),
                io_shutdown,
                detail,
                _mark: PhantomData,
            }
        }

        pub(crate) fn send_frame_to_controller(
            &mut self,
            request: RequestWrapper,
        ) -> Result<(), HttpClientError> {
            let (tx, rx) = bounded_channel::<RespMessage>(self.allow_cached_frames);
            self.receiver.set_receiver(rx);
            self.sender
                .send(ReqMessage {
                    sender: tx,
                    request,
                })
                .map_err(|_| {
                    HttpClientError::from_str(ErrorKind::Request, "Request Sender Closed !")
                })
        }
    }

    impl StreamController {
        pub(crate) fn new(
            streams: Streams,
            shutdown: Arc<AtomicBool>,
            goaway: Arc<AtomicBool>,
        ) -> Self {
            Self {
                io_shutdown: shutdown,
                io_goaway: goaway,
                senders: HashMap::new(),
                curr_message: HashMap::new(),
                streams,
                go_away_error_code: None,
                go_away_sync: GoAwaySync::default(),
            }
        }

        pub(crate) fn shutdown(&self) {
            self.io_shutdown.store(true, Ordering::Release);
        }

        pub(crate) fn goaway(&self) {
            self.io_goaway.store(true, Ordering::Release);
        }

        pub(crate) fn get_unsent_streams(
            &mut self,
            last_stream_id: StreamId,
        ) -> Result<Vec<StreamId>, H2Error> {
            // The last-stream-id in the subsequent GO_AWAY frame
            // cannot be greater than the last-stream-id in the previous GO_AWAY frame.
            if self.streams.max_send_id < last_stream_id {
                return Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
            }
            self.streams.max_send_id = last_stream_id;
            Ok(self.streams.get_unset_streams(last_stream_id))
        }

        pub(crate) fn send_message_to_stream(
            &mut self,
            cx: &mut Context<'_>,
            stream_id: StreamId,
            message: RespMessage,
        ) -> Poll<Result<(), H2Error>> {
            if let Some(sender) = self.senders.get(&stream_id) {
                // If the client coroutine has exited, this frame is skipped.
                let mut tx = {
                    let sender = sender.clone();
                    let ft = async move { sender.send(message).await };
                    Box::pin(ft)
                };

                match tx.as_mut().poll(cx) {
                    Poll::Ready(Ok(_)) => Poll::Ready(Ok(())),
                    // The current coroutine sending the request exited prematurely.
                    Poll::Ready(Err(_)) => {
                        self.senders.remove(&stream_id);
                        Poll::Ready(Err(H2Error::StreamError(stream_id, ErrorCode::NoError)))
                    }
                    Poll::Pending => {
                        self.curr_message.insert(stream_id, tx);
                        Poll::Pending
                    }
                }
            } else {
                Poll::Ready(Err(H2Error::StreamError(stream_id, ErrorCode::NoError)))
            }
        }

        pub(crate) fn poll_blocked_message(
            &mut self,
            cx: &mut Context<'_>,
            input_tx: &UnboundedSender<Frame>,
        ) -> Poll<()> {
            let keys: Vec<StreamId> = self.curr_message.keys().cloned().collect();
            let mut blocked = false;

            for key in keys {
                if let Some(mut task) = self.curr_message.remove(&key) {
                    match task.as_mut().poll(cx) {
                        Poll::Ready(Ok(_)) => {}
                        // The current coroutine sending the request exited prematurely.
                        Poll::Ready(Err(_)) => {
                            self.senders.remove(&key);
                            if let Some(state) = self.streams.stream_state(key) {
                                if !matches!(state, H2StreamState::Closed(_)) {
                                    if let StreamEndState::OK = self.streams.send_local_reset(key) {
                                        let rest_payload =
                                            RstStream::new(ErrorCode::NoError.into_code());
                                        let frame = Frame::new(
                                            key,
                                            FrameFlags::empty(),
                                            Payload::RstStream(rest_payload),
                                        );
                                        // ignore the send error occurs here in order to finish all
                                        // tasks.
                                        let _ = input_tx.send(frame);
                                    }
                                }
                            }
                        }
                        Poll::Pending => {
                            self.curr_message.insert(key, task);
                            blocked = true;
                        }
                    }
                }
            }
            if blocked {
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        }
    }

    impl RespReceiver {
        pub(crate) fn set_receiver(&mut self, receiver: BoundedReceiver<RespMessage>) {
            self.receiver = Some(receiver);
        }

        pub(crate) async fn recv(&mut self) -> Result<Frame, HttpClientError> {
            match self.receiver {
                Some(ref mut receiver) => {
                    #[cfg(feature = "tokio_base")]
                    match receiver.recv().await {
                        None => err_from_msg!(Request, "Response Sender Closed !"),
                        Some(message) => match message {
                            RespMessage::Output(frame) => Ok(frame),
                            RespMessage::OutputExit(e) => Err(dispatch_client_error(e)),
                        },
                    }

                    #[cfg(feature = "ylong_base")]
                    match receiver.recv().await {
                        Err(err) => Err(HttpClientError::from_error(ErrorKind::Request, err)),
                        Ok(message) => match message {
                            RespMessage::Output(frame) => Ok(frame),
                            RespMessage::OutputExit(e) => Err(dispatch_client_error(e)),
                        },
                    }
                }
                // this will not happen.
                None => Err(HttpClientError::from_str(
                    ErrorKind::Request,
                    "Invalid Frame Receiver !",
                )),
            }
        }

        pub(crate) fn poll_recv(
            &mut self,
            cx: &mut Context<'_>,
        ) -> Poll<Result<Frame, HttpClientError>> {
            if let Some(ref mut receiver) = self.receiver {
                #[cfg(feature = "tokio_base")]
                match receiver.poll_recv(cx) {
                    Poll::Ready(None) => {
                        Poll::Ready(err_from_msg!(Request, "Response Sender Closed !"))
                    }
                    Poll::Ready(Some(message)) => match message {
                        RespMessage::Output(frame) => Poll::Ready(Ok(frame)),
                        RespMessage::OutputExit(e) => Poll::Ready(Err(dispatch_client_error(e))),
                    },
                    Poll::Pending => Poll::Pending,
                }

                #[cfg(feature = "ylong_base")]
                match receiver.poll_recv(cx) {
                    Poll::Ready(Err(e)) => {
                        Poll::Ready(Err(HttpClientError::from_error(ErrorKind::Request, e)))
                    }
                    Poll::Ready(Ok(message)) => match message {
                        RespMessage::Output(frame) => Poll::Ready(Ok(frame)),
                        RespMessage::OutputExit(e) => Poll::Ready(Err(dispatch_client_error(e))),
                    },
                    Poll::Pending => Poll::Pending,
                }
            } else {
                Poll::Ready(err_from_msg!(Request, "Invalid Frame Receiver !"))
            }
        }
    }

    async fn async_send_preface<S>(writer: &mut WriteHalf<S>) -> Result<(), DispatchErrorKind>
    where
        S: AsyncWrite + Unpin,
    {
        const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
        writer
            .write_all(PREFACE)
            .await
            .map_err(|e| DispatchErrorKind::Io(e.kind()))
    }

    pub(crate) fn create_initial_settings(config: &H2Config) -> Frame {
        let settings = SettingsBuilder::new()
            .max_header_list_size(config.max_header_list_size())
            .max_frame_size(config.max_frame_size())
            .header_table_size(config.header_table_size())
            .enable_push(config.enable_push())
            .initial_window_size(config.stream_window_size())
            .build();

        Frame::new(0, FrameFlags::new(0), Payload::Settings(settings))
    }

    impl From<std::io::Error> for DispatchErrorKind {
        fn from(value: std::io::Error) -> Self {
            DispatchErrorKind::Io(value.kind())
        }
    }

    impl From<H2Error> for DispatchErrorKind {
        fn from(err: H2Error) -> Self {
            DispatchErrorKind::H2(err)
        }
    }

    pub(crate) fn dispatch_client_error(dispatch_error: DispatchErrorKind) -> HttpClientError {
        match dispatch_error {
            DispatchErrorKind::H2(e) => HttpClientError::from_error(Request, HttpError::from(e)),
            DispatchErrorKind::Io(e) => {
                HttpClientError::from_io_error(Request, std::io::Error::from(e))
            }
            DispatchErrorKind::ChannelClosed => {
                HttpClientError::from_str(Request, "Coroutine channel closed.")
            }
            DispatchErrorKind::Disconnect => {
                HttpClientError::from_str(Request, "remote peer closed.")
            }
        }
    }
}

#[cfg(feature = "http3")]
pub(crate) mod http3 {
    use std::marker::PhantomData;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use ylong_http::error::HttpError;
    use ylong_http::h3::{Frame, FrameDecoder, H3Error};

    use crate::async_impl::QuicConn;
    use crate::runtime::{
        bounded_channel, unbounded_channel, AsyncRead, AsyncWrite, BoundedReceiver, BoundedSender,
        UnboundedSender,
    };
    use crate::util::config::H3Config;
    use crate::util::data_ref::BodyDataRef;
    use crate::util::dispatcher::{ConnDispatcher, Dispatcher};
    use crate::util::h3::io_manager::IOManager;
    use crate::util::h3::stream_manager::StreamManager;
    use crate::util::progress::SpeedController;
    use crate::ErrorKind::Request;
    use crate::{ConnDetail, ConnInfo, ErrorKind, HttpClientError};

    pub(crate) struct Http3Dispatcher<S> {
        pub(crate) detail: ConnDetail,
        pub(crate) req_tx: UnboundedSender<ReqMessage>,
        pub(crate) handles: Vec<crate::runtime::JoinHandle<()>>,
        pub(crate) _mark: PhantomData<S>,
        pub(crate) io_shutdown: Arc<AtomicBool>,
        pub(crate) io_goaway: Arc<AtomicBool>,
    }

    pub(crate) struct Http3Conn<S> {
        pub(crate) speed_controller: SpeedController,
        pub(crate) sender: UnboundedSender<ReqMessage>,
        pub(crate) resp_receiver: BoundedReceiver<RespMessage>,
        pub(crate) resp_sender: BoundedSender<RespMessage>,
        pub(crate) io_shutdown: Arc<AtomicBool>,
        pub(crate) detail: ConnDetail,
        pub(crate) _mark: PhantomData<S>,
    }

    pub(crate) struct RequestWrapper {
        pub(crate) header: Frame,
        pub(crate) data: BodyDataRef,
    }

    #[derive(Debug, Clone)]
    pub(crate) enum DispatchErrorKind {
        H3(H3Error),
        Io(std::io::ErrorKind),
        Quic(quiche::Error),
        ChannelClosed,
        StreamFinished,
        // todo: retry?
        GoawayReceived,
        Disconnect,
    }

    pub(crate) enum RespMessage {
        Output(Frame),
        OutputExit(DispatchErrorKind),
    }

    pub(crate) struct ReqMessage {
        pub(crate) request: RequestWrapper,
        pub(crate) frame_tx: BoundedSender<RespMessage>,
    }

    impl<S> Http3Dispatcher<S>
    where
        S: AsyncRead + AsyncWrite + ConnInfo + Sync + Send + Unpin + 'static,
    {
        pub(crate) fn new(
            detail: ConnDetail,
            config: H3Config,
            io: S,
            quic_connection: QuicConn,
        ) -> Self {
            let (req_tx, req_rx) = unbounded_channel();
            let (io_manager_tx, io_manager_rx) = unbounded_channel();
            let (stream_manager_tx, stream_manager_rx) = unbounded_channel();
            let mut handles = Vec::with_capacity(2);
            let conn = Arc::new(Mutex::new(quic_connection));
            let io_shutdown = Arc::new(AtomicBool::new(false));
            let io_goaway = Arc::new(AtomicBool::new(false));
            let mut stream_manager = StreamManager::new(
                conn.clone(),
                io_manager_tx,
                stream_manager_rx,
                req_rx,
                FrameDecoder::new(
                    config.qpack_blocked_streams() as usize,
                    config.qpack_max_table_capacity() as usize,
                ),
                io_shutdown.clone(),
                io_goaway.clone(),
            );
            let stream_handle = crate::runtime::spawn(async move {
                if stream_manager.init(config).is_err() {
                    return;
                }
                let _ = Pin::new(&mut stream_manager).await;
            });
            handles.push(stream_handle);

            let io_handle = crate::runtime::spawn(async move {
                let mut io_manager = IOManager::new(io, conn, io_manager_rx, stream_manager_tx);
                let _ = Pin::new(&mut io_manager).await;
            });
            handles.push(io_handle);
            // read_rx gets readable stream ids and writable client channels, then read
            // stream and send to the corresponding channel
            Self {
                detail,
                req_tx,
                handles,
                _mark: PhantomData,
                io_shutdown,
                io_goaway,
            }
        }
    }

    impl<S> Http3Conn<S> {
        pub(crate) fn new(
            detail: ConnDetail,
            sender: UnboundedSender<ReqMessage>,
            io_shutdown: Arc<AtomicBool>,
        ) -> Self {
            const CHANNEL_SIZE: usize = 3;
            let (resp_sender, resp_receiver) = bounded_channel(CHANNEL_SIZE);
            Self {
                speed_controller: SpeedController::none(),
                sender,
                resp_sender,
                resp_receiver,
                _mark: PhantomData,
                io_shutdown,
                detail,
            }
        }

        pub(crate) fn send_frame_to_reader(
            &mut self,
            request: RequestWrapper,
        ) -> Result<(), HttpClientError> {
            self.sender
                .send(ReqMessage {
                    request,
                    frame_tx: self.resp_sender.clone(),
                })
                .map_err(|_| {
                    HttpClientError::from_str(ErrorKind::Request, "Request Sender Closed !")
                })
        }

        pub(crate) async fn recv_resp(&mut self) -> Result<Frame, HttpClientError> {
            #[cfg(feature = "tokio_base")]
            match self.resp_receiver.recv().await {
                None => err_from_msg!(Request, "Response Sender Closed !"),
                Some(message) => match message {
                    RespMessage::Output(frame) => Ok(frame),
                    RespMessage::OutputExit(e) => Err(dispatch_client_error(e)),
                },
            }

            #[cfg(feature = "ylong_base")]
            match self.resp_receiver.recv().await {
                Err(err) => Err(HttpClientError::from_error(ErrorKind::Request, err)),
                Ok(message) => match message {
                    RespMessage::Output(frame) => Ok(frame),
                    RespMessage::OutputExit(e) => Err(dispatch_client_error(e)),
                },
            }
        }
    }

    impl<S> ConnDispatcher<S>
    where
        S: AsyncRead + AsyncWrite + ConnInfo + Sync + Send + Unpin + 'static,
    {
        pub(crate) fn http3(
            detail: ConnDetail,
            config: H3Config,
            io: S,
            quic_connection: QuicConn,
        ) -> Self {
            Self::Http3(Http3Dispatcher::new(detail, config, io, quic_connection))
        }
    }

    impl<S> Dispatcher for Http3Dispatcher<S> {
        type Handle = Http3Conn<S>;

        fn dispatch(&self) -> Option<Self::Handle> {
            let sender = self.req_tx.clone();
            Some(Http3Conn::new(
                self.detail.clone(),
                sender,
                self.io_shutdown.clone(),
            ))
        }

        fn is_shutdown(&self) -> bool {
            self.io_shutdown.load(Ordering::Relaxed)
        }

        fn is_goaway(&self) -> bool {
            self.io_goaway.load(Ordering::Relaxed)
        }
    }

    impl<S> Drop for Http3Dispatcher<S> {
        fn drop(&mut self) {
            for handle in &self.handles {
                #[cfg(feature = "tokio_base")]
                handle.abort();
                #[cfg(feature = "ylong_base")]
                handle.cancel();
            }
        }
    }

    impl From<std::io::Error> for DispatchErrorKind {
        fn from(value: std::io::Error) -> Self {
            DispatchErrorKind::Io(value.kind())
        }
    }

    impl From<H3Error> for DispatchErrorKind {
        fn from(err: H3Error) -> Self {
            DispatchErrorKind::H3(err)
        }
    }

    impl From<quiche::Error> for DispatchErrorKind {
        fn from(value: quiche::Error) -> Self {
            DispatchErrorKind::Quic(value)
        }
    }

    pub(crate) fn dispatch_client_error(dispatch_error: DispatchErrorKind) -> HttpClientError {
        match dispatch_error {
            DispatchErrorKind::H3(e) => HttpClientError::from_error(Request, HttpError::from(e)),
            DispatchErrorKind::Io(e) => {
                HttpClientError::from_io_error(Request, std::io::Error::from(e))
            }
            DispatchErrorKind::ChannelClosed => {
                HttpClientError::from_str(Request, "Coroutine channel closed.")
            }
            DispatchErrorKind::Quic(e) => HttpClientError::from_error(Request, e),
            DispatchErrorKind::GoawayReceived => {
                HttpClientError::from_str(Request, "received remote goaway.")
            }
            DispatchErrorKind::StreamFinished => {
                HttpClientError::from_str(Request, "stream finished.")
            }
            DispatchErrorKind::Disconnect => {
                HttpClientError::from_str(Request, "remote peer closed.")
            }
        }
    }
}

#[cfg(test)]
mod ut_dispatch {
    use crate::dispatcher::{ConnDispatcher, Dispatcher};

    /// UT test cases for `ConnDispatcher::is_shutdown`.
    ///
    /// # Brief
    /// 1. Creates a `ConnDispatcher`.
    /// 2. Calls `ConnDispatcher::is_shutdown` to get the result.
    /// 3. Calls `ConnDispatcher::dispatch` to get the result.
    /// 4. Checks if the result is false.
    #[test]
    fn ut_is_shutdown() {
        let conn = ConnDispatcher::http1(b"Data");
        let res = conn.is_shutdown();
        assert!(!res);
        let res = conn.dispatch();
        assert!(res.is_some());
    }
}
