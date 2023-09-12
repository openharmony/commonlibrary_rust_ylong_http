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

pub(crate) trait Dispatcher {
    type Handle;

    fn dispatch(&self) -> Option<Self::Handle>;

    fn is_shutdown(&self) -> bool;
}

pub(crate) enum ConnDispatcher<S> {
    #[cfg(feature = "http1_1")]
    Http1(http1::Http1Dispatcher<S>),

    #[cfg(feature = "http2")]
    Http2(http2::Http2Dispatcher<S>),
}

impl<S> Dispatcher for ConnDispatcher<S> {
    type Handle = Conn<S>;

    fn dispatch(&self) -> Option<Self::Handle> {
        match self {
            #[cfg(feature = "http1_1")]
            Self::Http1(h1) => h1.dispatch().map(Conn::Http1),

            #[cfg(feature = "http2")]
            Self::Http2(h2) => h2.dispatch().map(Conn::Http2),
        }
    }

    fn is_shutdown(&self) -> bool {
        match self {
            #[cfg(feature = "http1_1")]
            Self::Http1(h1) => h1.is_shutdown(),

            #[cfg(feature = "http2")]
            Self::Http2(h2) => h2.is_shutdown(),
        }
    }
}

pub(crate) enum Conn<S> {
    #[cfg(feature = "http1_1")]
    Http1(http1::Http1Conn<S>),

    #[cfg(feature = "http2")]
    Http2(http2::Http2Conn<S>),
}

#[cfg(feature = "http1_1")]
pub(crate) mod http1 {
    use std::cell::UnsafeCell;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use super::{ConnDispatcher, Dispatcher};

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
                .map(|_| Http1Conn {
                    inner: self.inner.clone(),
                })
        }

        fn is_shutdown(&self) -> bool {
            self.inner.shutdown.load(Ordering::Relaxed)
        }
    }

    /// Handle returned to other threads for I/O operations.
    pub(crate) struct Http1Conn<S> {
        pub(crate) inner: Arc<Inner<S>>,
    }

    impl<S> Http1Conn<S> {
        pub(crate) fn raw_mut(&mut self) -> &mut S {
            // SAFETY: In the case of `HTTP1`, only one coroutine gets the handle
            // at the same time.
            unsafe { &mut *self.inner.io.get() }
        }

        pub(crate) fn shutdown(&self) {
            self.inner.shutdown.store(true, Ordering::Release);
        }
    }

    impl<S> Drop for Http1Conn<S> {
        fn drop(&mut self) {
            self.inner.occupied.store(false, Ordering::Release)
        }
    }
}

#[cfg(feature = "http2")]
pub(crate) mod http2 {
    use std::collections::{HashMap, VecDeque};
    use std::future::Future;
    use std::mem::take;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};

    use ylong_http::error::HttpError;
    use ylong_http::h2;
    use ylong_http::h2::Payload::Settings;
    use ylong_http::h2::{
        ErrorCode, Frame, FrameDecoder, FrameEncoder, FrameFlags, FrameKind, FramesIntoIter,
        Goaway, H2Error, Payload, RstStream, Setting, SettingsBuilder,
    };

    use super::{ConnDispatcher, Dispatcher};
    use crate::dispatcher::http2::StreamState::Closed;
    use crate::error::{ErrorKind, HttpClientError};
    use crate::runtime::{
        unbounded_channel, AsyncMutex, AsyncRead, AsyncWrite, MutexGuard, ReadBuf, TryRecvError,
        UnboundedReceiver, UnboundedSender,
    };
    use crate::util::config::H2Config;

    impl<S> ConnDispatcher<S> {
        pub(crate) fn http2(config: H2Config, io: S) -> Self {
            Self::Http2(Http2Dispatcher::new(config, io))
        }
    }

    // The data type of the first Frame sent to the `StreamController`.
    type Send2Ctrl = (Option<(u32, UnboundedSender<Frame>)>, Frame);

    const DEFAULT_MAX_STREAM_ID: u32 = u32::MAX >> 1;
    const DEFAULT_MAX_FRAME_SIZE: usize = 2 << 13;
    const DEFAULT_MAX_HEADER_LIST_SIZE: usize = 16 << 20;

    // HTTP2-based connection manager, which can dispatch connections to other
    // threads according to HTTP2 syntax.
    pub(crate) struct Http2Dispatcher<S> {
        pub(crate) controller: Arc<StreamController<S>>,
        pub(crate) next_stream_id: Arc<StreamId>,
        pub(crate) sender: UnboundedSender<Send2Ctrl>,
    }

    pub(crate) struct Http2Conn<S> {
        // Handle id
        pub(crate) id: u32,
        // Sends frame to StreamController
        pub(crate) sender: UnboundedSender<Send2Ctrl>,
        pub(crate) stream_info: StreamInfo<S>,
    }

    pub(crate) struct StreamInfo<S> {
        // Stream id
        pub(crate) id: u32,
        pub(crate) next_stream_id: Arc<StreamId>,
        // Receive the Response frame transmitted from the StreamController
        pub(crate) receiver: FrameReceiver,
        // Used to handle TCP Stream
        pub(crate) controller: Arc<StreamController<S>>,
    }

    pub(crate) struct StreamController<S> {
        // I/O unavailability flag, which prevents the upper layer from using this I/O to create
        // new streams.
        pub(crate) io_shutdown: AtomicBool,
        // Indicates that the dispatcher is occupied. At this time, a user coroutine is already
        // acting as the dispatcher.
        pub(crate) occupied: AtomicU32,
        pub(crate) dispatcher_invalid: AtomicBool,
        pub(crate) manager: AsyncMutex<IoManager<S>>,
        pub(crate) stream_waker: Mutex<StreamWaker>,
    }

    pub(crate) struct StreamWaker {
        waker: HashMap<u32, Waker>,
    }

    pub(crate) struct IoManager<S> {
        inner: Inner<S>,
        senders: HashMap<u32, UnboundedSender<Frame>>,
        frame_receiver: UnboundedReceiver<Send2Ctrl>,
        streams: Streams,
        frame_iter: FrameIter,
        connection_frame: ConnectionFrames,
    }

    #[derive(Default)]
    pub(crate) struct FrameIter {
        iter: Option<FramesIntoIter>,
    }

    pub(crate) struct Streams {
        stream_to_send: VecDeque<u32>,
        buffer: HashMap<u32, StreamBuffer>,
    }

    pub(crate) struct StreamBuffer {
        state: StreamState,
        frames: VecDeque<Frame>,
    }

    pub(crate) struct Inner<S> {
        pub(crate) io: S,
        pub(crate) encoder: FrameEncoder,
        pub(crate) decoder: FrameDecoder,
    }

    pub(crate) enum ReadState {
        EmptyIo,
        CurrentStream,
    }

    enum DispatchState {
        Partial,
        Finish,
    }

    #[derive(Clone)]
    pub(crate) enum ResetReason {
        Local,
        Remote,
        Goaway(u32),
    }

    #[derive(Clone)]
    pub(crate) enum SettingsSync {
        Send(h2::Settings),
        Acknowledging(h2::Settings),
        Synced,
    }

    pub(crate) struct StreamId {
        // TODO Determine the maximum value of id.
        next_id: AtomicU32,
    }

    // TODO Add "open", "half-closed", "reserved" state
    #[derive(Clone)]
    pub(crate) enum StreamState {
        Idle,
        Closed(ResetReason),
    }

    #[derive(Default)]
    pub(crate) struct FrameReceiver {
        receiver: Option<UnboundedReceiver<Frame>>,
    }

    impl<S> StreamController<S> {
        pub(crate) fn new(
            inner: Inner<S>,
            frame_receiver: UnboundedReceiver<Send2Ctrl>,
            connection_frame: ConnectionFrames,
        ) -> Self {
            let manager = IoManager::new(inner, frame_receiver, connection_frame);
            Self {
                io_shutdown: AtomicBool::new(false),
                // 0 means io is not occupied
                occupied: AtomicU32::new(0),
                dispatcher_invalid: AtomicBool::new(false),
                manager: AsyncMutex::new(manager),
                stream_waker: Mutex::new(StreamWaker::new()),
            }
        }

        pub(crate) fn shutdown(&self) {
            self.io_shutdown.store(true, Ordering::Release);
        }

        pub(crate) fn invalid(&self) {
            self.dispatcher_invalid.store(true, Ordering::Release);
        }
    }

    impl Streams {
        pub(crate) fn new() -> Self {
            Self {
                stream_to_send: VecDeque::new(),
                buffer: HashMap::new(),
            }
        }

        pub(crate) fn size(&self) -> usize {
            self.stream_to_send.len()
        }

        pub(crate) fn insert(&mut self, frame: Frame) {
            let id = frame.stream_id() as u32;
            self.stream_to_send.push_back(id);
            match self.buffer.get_mut(&id) {
                Some(sender) => {
                    sender.push_back(frame);
                }
                None => {
                    let mut sender = StreamBuffer::new();
                    sender.push_back(frame);
                    self.buffer.insert(id, sender);
                }
            }
        }

        pub(crate) fn get_goaway_streams(
            &mut self,
            last_stream_id: u32,
        ) -> Result<Vec<u32>, H2Error> {
            let mut ids = vec![];
            for (id, sender) in self.buffer.iter_mut() {
                if *id >= last_stream_id {
                    ids.push(*id);
                    sender.go_away(*id)?;
                }
            }
            Ok(ids)
        }

        pub(crate) fn recv_local_reset(&mut self, id: u32) -> Result<(), H2Error> {
            match self.buffer.get_mut(&id) {
                None => Err(H2Error::ConnectionError(ErrorCode::ProtocolError)),
                Some(sender) => {
                    match sender.state {
                        Closed(ResetReason::Remote | ResetReason::Local) => {}
                        _ => {
                            sender.state = Closed(ResetReason::Local);
                        }
                    }
                    Ok(())
                }
            }
        }

        pub(crate) fn recv_remote_reset(&mut self, id: u32) -> Result<(), H2Error> {
            match self.buffer.get_mut(&id) {
                None => Err(H2Error::ConnectionError(ErrorCode::ProtocolError)),
                Some(sender) => {
                    match sender.state {
                        Closed(ResetReason::Remote) => {}
                        _ => {
                            sender.state = Closed(ResetReason::Remote);
                        }
                    }
                    Ok(())
                }
            }
        }

        // TODO At present, only the state is changed to closed, and other states are
        // not involved, and it needs to be added later
        pub(crate) fn recv_headers(&mut self, id: u32) -> Result<StreamState, H2Error> {
            match self.buffer.get_mut(&id) {
                None => Err(H2Error::ConnectionError(ErrorCode::ProtocolError)),
                Some(sender) => match sender.state {
                    Closed(ResetReason::Goaway(last_id)) => {
                        if id > last_id {
                            return Err(H2Error::ConnectionError(ErrorCode::StreamClosed));
                        }
                        Ok(sender.state.clone())
                    }
                    Closed(ResetReason::Remote) => {
                        Err(H2Error::ConnectionError(ErrorCode::StreamClosed))
                    }
                    _ => Ok(sender.state.clone()),
                },
            }
        }

        pub(crate) fn recv_data(&mut self, id: u32) -> Result<StreamState, H2Error> {
            self.recv_headers(id)
        }

        pub(crate) fn pop_front(&mut self) -> Result<Option<Frame>, H2Error> {
            match self.stream_to_send.pop_front() {
                None => Ok(None),
                Some(id) => {
                    // TODO Subsequent consideration is to delete the corresponding elements in the
                    // map after the status becomes Closed
                    match self.buffer.get_mut(&id) {
                        None => Err(H2Error::ConnectionError(ErrorCode::IntervalError)),
                        Some(sender) => {
                            // TODO For the time being, match state is used here, and the complete
                            // logic should be judged based on the frame type and state
                            match sender.state {
                                Closed(ResetReason::Remote | ResetReason::Local) => Ok(None),
                                _ => Ok(sender.pop_front()),
                            }
                        }
                    }
                }
            }
        }
    }

    impl StreamBuffer {
        pub(crate) fn push_back(&mut self, frame: Frame) {
            self.frames.push_back(frame);
        }

        pub(crate) fn pop_front(&mut self) -> Option<Frame> {
            self.frames.pop_front()
        }

        pub(crate) fn new() -> Self {
            Self {
                state: StreamState::Idle,
                frames: VecDeque::new(),
            }
        }

        pub(crate) fn go_away(&mut self, last_stream_id: u32) -> Result<(), H2Error> {
            match self.state {
                Closed(ResetReason::Local | ResetReason::Remote) => {}
                Closed(ResetReason::Goaway(id)) => {
                    if last_stream_id > id {
                        return Err(H2Error::ConnectionError(ErrorCode::ProtocolError));
                    }
                    self.state = Closed(ResetReason::Goaway(last_stream_id));
                }
                _ => {
                    self.state = Closed(ResetReason::Goaway(last_stream_id));
                }
            }
            Ok(())
        }
    }

    impl SettingsSync {
        pub(crate) fn ack_settings() -> Frame {
            Frame::new(0, FrameFlags::new(0x1), Settings(h2::Settings::new(vec![])))
        }
    }

    pub(crate) struct ConnectionFrames {
        preface: bool,
        settings: SettingsSync,
    }

    impl ConnectionFrames {
        pub(crate) fn new(settings: h2::Settings) -> Self {
            Self {
                preface: true,
                settings: SettingsSync::Send(settings),
            }
        }
    }

    impl StreamWaker {
        pub(crate) fn new() -> Self {
            Self {
                waker: HashMap::new(),
            }
        }
    }

    impl<S> IoManager<S> {
        pub(crate) fn new(
            inner: Inner<S>,
            frame_receiver: UnboundedReceiver<Send2Ctrl>,
            connection_frame: ConnectionFrames,
        ) -> Self {
            Self {
                inner,
                senders: HashMap::new(),
                frame_receiver,
                streams: Streams::new(),
                frame_iter: FrameIter::default(),
                connection_frame,
            }
        }

        fn close_frame_receiver(&mut self) {
            self.frame_receiver.close()
        }
    }

    impl FrameIter {
        pub(crate) fn is_empty(&self) -> bool {
            self.iter.is_none()
        }
    }

    impl StreamId {
        fn stream_id_generate(&self) -> u32 {
            self.next_id.fetch_add(2, Ordering::Relaxed)
        }

        fn get_next_id(&self) -> u32 {
            self.next_id.load(Ordering::Relaxed)
        }
    }

    impl<S> Http2Dispatcher<S> {
        pub(crate) fn new(config: H2Config, io: S) -> Self {
            // send_preface(&mut io).await?;

            let connection_frames = build_connection_frames(config);
            let inner = Inner {
                io,
                encoder: FrameEncoder::new(DEFAULT_MAX_FRAME_SIZE, DEFAULT_MAX_HEADER_LIST_SIZE),
                decoder: FrameDecoder::new(),
            };

            // For each stream to send the frame to the controller
            let (tx, rx) = unbounded_channel::<Send2Ctrl>();

            let stream_controller = Arc::new(StreamController::new(inner, rx, connection_frames));

            // The id of the client stream, starting from 1
            let next_stream_id = StreamId {
                next_id: AtomicU32::new(1),
            };
            Self {
                controller: stream_controller,
                sender: tx,
                next_stream_id: Arc::new(next_stream_id),
            }
        }
    }

    impl<S> Dispatcher for Http2Dispatcher<S> {
        type Handle = Http2Conn<S>;

        // Call this method to get a stream
        fn dispatch(&self) -> Option<Self::Handle> {
            let id = self.next_stream_id.stream_id_generate();
            // TODO Consider how to create a new connection and transfer state
            if id > DEFAULT_MAX_STREAM_ID {
                return None;
            }
            let controller = self.controller.clone();
            let sender = self.sender.clone();
            let handle = Http2Conn::new(id, self.next_stream_id.clone(), sender, controller);
            Some(handle)
        }

        // TODO When the stream id reaches the maximum value, shutdown the current
        // connection
        fn is_shutdown(&self) -> bool {
            self.controller.io_shutdown.load(Ordering::Relaxed)
        }
    }

    impl<S> Http2Conn<S> {
        pub(crate) fn new(
            id: u32,
            next_stream_id: Arc<StreamId>,
            sender: UnboundedSender<Send2Ctrl>,
            controller: Arc<StreamController<S>>,
        ) -> Self {
            let stream_info = StreamInfo {
                id,
                next_stream_id,
                receiver: FrameReceiver::default(),
                controller,
            };
            Self {
                id,
                sender,
                stream_info,
            }
        }

        pub(crate) fn send_frame_to_controller(
            &mut self,
            frame: Frame,
        ) -> Result<(), HttpClientError> {
            if self.stream_info.receiver.is_none() {
                let (tx, rx) = unbounded_channel::<Frame>();
                self.stream_info.receiver.set_receiver(rx);
                self.sender.send((Some((self.id, tx)), frame)).map_err(|_| {
                    HttpClientError::from_error(ErrorKind::Request, String::from("resend"))
                })
            } else {
                self.sender.send((None, frame)).map_err(|_| {
                    HttpClientError::from_error(ErrorKind::Request, String::from("resend"))
                })
            }
        }
    }

    impl<S: AsyncRead + AsyncWrite + Unpin + Sync + Send + 'static> Future for StreamInfo<S> {
        type Output = Result<Frame, HttpError>;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let stream_info = self.get_mut();

            // First, check whether the frame of the current stream is in the Channel.
            // The error cannot occur. Therefore, the error is thrown directly without
            // connection-level processing.
            if let Some(frame) = stream_info.receiver.recv_frame(stream_info.id)? {
                {
                    let mut stream_waker = stream_info
                        .controller
                        .stream_waker
                        .lock()
                        .expect("Blocking get waker lock failed! ");
                    //
                    wakeup_next_stream(&mut stream_waker.waker);
                }
                return Poll::Ready(Ok(frame));
            }

            // If the dispatcher sends a goaway frame, all streams on the current connection
            // are unavailable.
            if stream_info
                .controller
                .dispatcher_invalid
                .load(Ordering::Relaxed)
            {
                return Poll::Ready(Err(H2Error::ConnectionError(ErrorCode::ConnectError).into()));
            }

            // The error cannot occur. Therefore, the error is thrown directly without
            // connection-level processing.
            if is_io_available(&stream_info.controller.occupied, stream_info.id)? {
                {
                    // Second, try to get io and read the frame of the current stream from io.
                    if let Ok(mut io_manager) = stream_info.controller.manager.try_lock() {
                        if stream_info
                            .poll_match_result(cx, &mut io_manager)?
                            .is_pending()
                        {
                            return Poll::Pending;
                        }
                    }
                }
                {
                    let mut stream_waker = stream_info
                        .controller
                        .stream_waker
                        .lock()
                        .expect("Blocking get waker lock failed! ");
                    wakeup_next_stream(&mut stream_waker.waker);
                }
                // The error cannot occur. Therefore, the error is thrown directly without
                // connection-level processing.
                let frame_opt = get_frame(stream_info.receiver.recv_frame(stream_info.id)?);
                return Poll::Ready(frame_opt);
            }

            {
                let mut io_manager = {
                    // Third, wait to acquire the lock of waker, which is used to insert the current
                    // waker, and wait to be awakened by the io stream.
                    let mut stream_waker = stream_info
                        .controller
                        .stream_waker
                        .lock()
                        .expect("Blocking get waker lock failed! ");

                    // Fourth, after obtaining the waker lock,
                    // you need to check the Receiver again to prevent the Receiver from receiving a
                    // frame while waiting for the waker. The error cannot
                    // occur. Therefore, the error is thrown directly without connection-level
                    // processing.
                    if let Some(frame) = stream_info.receiver.recv_frame(stream_info.id)? {
                        wakeup_next_stream(&mut stream_waker.waker);
                        return Poll::Ready(Ok(frame));
                    }

                    // The error cannot occur. Therefore, the error is thrown directly without
                    // connection-level processing.
                    if is_io_available(&stream_info.controller.occupied, stream_info.id)? {
                        // Fifth, get io again to prevent no other streams from controlling io while
                        // waiting for the waker, leaving only the current
                        // stream.
                        match stream_info.controller.manager.try_lock() {
                            Ok(guard) => guard,
                            _ => {
                                stream_waker
                                    .waker
                                    .insert(stream_info.id, cx.waker().clone());
                                return Poll::Pending;
                            }
                        }
                    } else {
                        stream_waker
                            .waker
                            .insert(stream_info.id, cx.waker().clone());
                        return Poll::Pending;
                    }
                };
                if stream_info
                    .poll_match_result(cx, &mut io_manager)?
                    .is_pending()
                {
                    return Poll::Pending;
                }
            }
            {
                {
                    let mut stream_waker = stream_info
                        .controller
                        .stream_waker
                        .lock()
                        .expect("Blocking get waker lock failed! ");
                    wakeup_next_stream(&mut stream_waker.waker);
                }
                // The error cannot occur. Therefore, the error is thrown directly without
                // connection-level processing.
                let frame_opt = get_frame(stream_info.receiver.recv_frame(stream_info.id)?);
                Poll::Ready(frame_opt)
            }
        }
    }

    impl<S: AsyncRead + AsyncWrite + Unpin + Sync + Send + 'static> StreamInfo<S> {
        fn poll_match_result(
            &self,
            cx: &mut Context<'_>,
            io_manager: &mut MutexGuard<IoManager<S>>,
        ) -> Poll<Result<(), HttpError>> {
            loop {
                match self.poll_io(cx, io_manager) {
                    Poll::Ready(Ok(_)) => {
                        return Poll::Ready(Ok(()));
                    }
                    Poll::Ready(Err(h2_error)) => {
                        match h2_error {
                            H2Error::StreamError(id, code) => {
                                let rest_payload = RstStream::new(code.clone().into_code());
                                let frame = Frame::new(
                                    id as usize,
                                    FrameFlags::empty(),
                                    Payload::RstStream(rest_payload),
                                );
                                io_manager.streams.recv_local_reset(id)?;
                                if self
                                    .poll_send_reset(cx, frame.clone(), io_manager)?
                                    .is_pending()
                                {
                                    compare_exchange_occupation(
                                        &self.controller.occupied,
                                        0,
                                        self.id,
                                    )?;
                                    return Poll::Pending;
                                }
                                if self.id == id {
                                    return Poll::Ready(Err(H2Error::StreamError(id, code).into()));
                                } else {
                                    self.controller_send_frame_to_stream(id, frame, io_manager);
                                    {
                                        let mut stream_waker = self
                                            .controller
                                            .stream_waker
                                            .lock()
                                            .expect("Blocking get waker lock failed! ");
                                        // TODO Is there a situation where the result has been
                                        // returned, but the waker has not been inserted into the
                                        // map? how to deal with.
                                        if let Some(waker) = stream_waker.waker.remove(&id) {
                                            waker.wake();
                                        }
                                    }
                                }
                            }
                            H2Error::ConnectionError(code) => {
                                io_manager.close_frame_receiver();
                                self.controller.shutdown();
                                // Since ConnectError may be caused by an io error, so when the
                                // client actively sends a goaway
                                // frame, all streams are shut down and no streams are allowed to
                                // complete. TODO Then consider
                                // separating io errors from frame errors to allow streams whose
                                // stream id is less than last_stream_id to continue
                                self.controller.invalid();
                                // last_stream_id is set to 0 to ensure that all streams are
                                // shutdown.
                                let goaway_payload =
                                    Goaway::new(code.clone().into_code(), 0, vec![]);
                                let frame = Frame::new(
                                    0,
                                    FrameFlags::empty(),
                                    Payload::Goaway(goaway_payload),
                                );
                                // io_manager.connection_frame.going_away(frame);
                                if self
                                    .poll_send_go_away(cx, frame.clone(), io_manager)?
                                    .is_pending()
                                {
                                    compare_exchange_occupation(
                                        &self.controller.occupied,
                                        0,
                                        self.id,
                                    )?;
                                    return Poll::Pending;
                                }

                                self.goaway_unsent_stream(io_manager, 0, frame)?;
                                self.goaway_and_shutdown();
                                return Poll::Ready(Err(H2Error::ConnectionError(code).into()));
                            }
                        }
                    }
                    Poll::Pending => {
                        compare_exchange_occupation(&self.controller.occupied, 0, self.id)?;
                        return Poll::Pending;
                    }
                }
            }
        }

        fn poll_io(
            &self,
            cx: &mut Context<'_>,
            io_manager: &mut MutexGuard<IoManager<S>>,
        ) -> Poll<Result<(), H2Error>> {
            if self.poll_send_preface(cx, io_manager)?.is_pending() {
                return Poll::Pending;
            }
            if self.poll_send_settings(cx, io_manager)?.is_pending() {
                return Poll::Pending;
            }
            match self.poll_dispatch_frame(cx, io_manager)? {
                Poll::Ready(state) => {
                    if let DispatchState::Partial = state {
                        return Poll::Ready(Ok(()));
                    }
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
            // Write and read frames to io in a loop until the frame of the current stream
            // is read and exit the loop.
            loop {
                if self.poll_write_frame(cx, io_manager)?.is_pending() {
                    return Poll::Pending;
                }
                match self.poll_read_frame(cx, io_manager)? {
                    Poll::Ready(ReadState::EmptyIo) => {}
                    Poll::Ready(ReadState::CurrentStream) => {
                        return Poll::Ready(Ok(()));
                    }
                    Poll::Pending => {
                        return Poll::Pending;
                    }
                }
            }
        }

        fn poll_dispatch_frame(
            &self,
            cx: &mut Context<'_>,
            io_manager: &mut MutexGuard<IoManager<S>>,
        ) -> Poll<Result<DispatchState, H2Error>> {
            if io_manager.frame_iter.is_empty() {
                return Poll::Ready(Ok(DispatchState::Finish));
            }
            let iter_option = take(&mut io_manager.frame_iter.iter);
            match iter_option {
                None => Poll::Ready(Err(H2Error::ConnectionError(ErrorCode::IntervalError))),
                Some(iter) => self.dispatch_read_frames(cx, io_manager, iter),
            }
        }

        fn poll_send_preface(
            &self,
            cx: &mut Context<'_>,
            io_manager: &mut MutexGuard<IoManager<S>>,
        ) -> Poll<Result<(), H2Error>> {
            const PREFACE_MSG: &str = "PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
            if io_manager.connection_frame.preface {
                let mut buf = [0u8; PREFACE_MSG.len()];
                buf.copy_from_slice(PREFACE_MSG.as_bytes());

                let mut start_index = 0;
                loop {
                    if start_index == PREFACE_MSG.len() {
                        io_manager.connection_frame.preface = false;
                        break;
                    }
                    match Pin::new(&mut io_manager.inner.io)
                        .poll_write(cx, &buf[start_index..])
                        .map_err(|_| H2Error::ConnectionError(ErrorCode::IntervalError))?
                    {
                        Poll::Ready(written) => {
                            start_index += written;
                        }
                        Poll::Pending => {
                            return Poll::Pending;
                        }
                    }
                }
                return poll_flush_io(cx, &mut io_manager.inner);
            }
            Poll::Ready(Ok(()))
        }

        fn poll_send_go_away(
            &self,
            cx: &mut Context<'_>,
            goaway: Frame,
            io_manager: &mut MutexGuard<IoManager<S>>,
        ) -> Poll<Result<(), H2Error>> {
            let mut buf = [0u8; 1024];
            if write_frame_to_io(cx, &mut buf, goaway, &mut io_manager.inner)?.is_pending() {
                Poll::Pending
            } else {
                poll_flush_io(cx, &mut io_manager.inner)
            }
        }

        fn poll_send_reset(
            &self,
            cx: &mut Context<'_>,
            reset: Frame,
            io_manager: &mut MutexGuard<IoManager<S>>,
        ) -> Poll<Result<(), H2Error>> {
            let mut buf = [0u8; 1024];
            if write_frame_to_io(cx, &mut buf, reset, &mut io_manager.inner)?.is_pending() {
                Poll::Pending
            } else {
                poll_flush_io(cx, &mut io_manager.inner)
            }
        }

        fn poll_send_settings(
            &self,
            cx: &mut Context<'_>,
            io_manager: &mut MutexGuard<IoManager<S>>,
        ) -> Poll<Result<(), H2Error>> {
            if let SettingsSync::Send(settings) = io_manager.connection_frame.settings.clone() {
                let mut buf = [0u8; 1024];
                let frame = Frame::new(0, FrameFlags::empty(), Settings(settings.clone()));
                if write_frame_to_io(cx, &mut buf, frame, &mut io_manager.inner)?.is_pending() {
                    Poll::Pending
                } else {
                    io_manager.connection_frame.settings = SettingsSync::Acknowledging(settings);
                    poll_flush_io(cx, &mut io_manager.inner)
                }
            } else {
                Poll::Ready(Ok(()))
            }
        }

        fn poll_write_frame(
            &self,
            cx: &mut Context<'_>,
            io_manager: &mut MutexGuard<IoManager<S>>,
        ) -> Poll<Result<(), H2Error>> {
            const FRAME_WRITE_NUM: usize = 10;

            // Send 10 frames each time, if there is not enough in the queue, read enough
            // from mpsc::Receiver
            while io_manager.streams.size() < FRAME_WRITE_NUM {
                match io_manager.frame_receiver.try_recv() {
                    // The Frame sent by the Handle for the first time will carry a Sender at the
                    // same time, which is used to send the Response Frame back
                    // to the Handle
                    Ok((Some((id, sender)), frame)) => {
                        if io_manager.senders.insert(id, sender).is_some() {
                            return Poll::Ready(Err(H2Error::ConnectionError(
                                ErrorCode::IntervalError,
                            )));
                        }
                        io_manager.streams.insert(frame);
                    }
                    Ok((None, frame)) => {
                        io_manager.streams.insert(frame);
                    }
                    Err(TryRecvError::Empty) => {
                        break;
                    }
                    Err(TryRecvError::Disconnected) => {
                        return Poll::Ready(Err(H2Error::ConnectionError(ErrorCode::ConnectError)))
                    }
                }
            }
            let mut buf = [0u8; 1024];
            for _i in 0..FRAME_WRITE_NUM {
                match io_manager.streams.pop_front()? {
                    Some(frame) => {
                        if write_frame_to_io(cx, &mut buf, frame, &mut io_manager.inner)?
                            .is_pending()
                        {
                            return Poll::Pending;
                        }
                    }
                    None => {
                        break;
                    }
                }
            }
            poll_flush_io(cx, &mut io_manager.inner)
        }

        fn poll_read_frame(
            &self,
            cx: &mut Context<'_>,
            io_manager: &mut MutexGuard<IoManager<S>>,
        ) -> Poll<Result<ReadState, H2Error>> {
            // Read all the frames in io until the frame of the current stream is read and
            // stop.
            let mut buf = [0u8; 1024];
            loop {
                let mut read_buf = ReadBuf::new(&mut buf);
                match Pin::new(&mut io_manager.inner.io).poll_read(cx, &mut read_buf) {
                    Poll::Ready(Err(_)) => {
                        return Poll::Ready(Err(H2Error::ConnectionError(ErrorCode::ConnectError)))
                    }
                    Poll::Pending => {
                        return Poll::Pending;
                    }
                    _ => {}
                }
                let read = read_buf.filled().len();
                if read == 0 {
                    break;
                }
                let frames = io_manager.inner.decoder.decode(&buf[..read])?;
                let frame_iterator = frames.into_iter();

                match self.dispatch_read_frames(cx, io_manager, frame_iterator)? {
                    Poll::Ready(state) => {
                        if let DispatchState::Partial = state {
                            return Poll::Ready(Ok(ReadState::CurrentStream));
                        }
                    }
                    Poll::Pending => {
                        return Poll::Pending;
                    }
                }
            }
            Poll::Ready(Ok(ReadState::EmptyIo))
        }

        fn dispatch_read_frames(
            &self,
            cx: &mut Context<'_>,
            io_manager: &mut MutexGuard<IoManager<S>>,
            mut frame_iterator: FramesIntoIter,
        ) -> Poll<Result<DispatchState, H2Error>> {
            let mut meet_this = false;
            loop {
                match frame_iterator.next() {
                    None => break,
                    Some(frame_kind) => {
                        if let FrameKind::Complete(frame) = frame_kind {
                            match frame.payload() {
                                Settings(settings) => {
                                    if self
                                        .recv_settings_frame(
                                            cx,
                                            io_manager,
                                            frame.flags().is_ack(),
                                            settings,
                                        )?
                                        .is_pending()
                                    {
                                        return Poll::Pending;
                                    }
                                    continue;
                                }
                                Payload::Ping(ping) => {
                                    if self
                                        .recv_ping_frame(
                                            cx,
                                            io_manager,
                                            frame.flags().is_ack(),
                                            ping,
                                        )?
                                        .is_pending()
                                    {
                                        return Poll::Pending;
                                    }
                                    continue;
                                }
                                Payload::PushPromise(_) => {
                                    // TODO The current settings_enable_push is fixed to false
                                    return Poll::Ready(Err(H2Error::ConnectionError(
                                        ErrorCode::ProtocolError,
                                    )));
                                }
                                Payload::Goaway(goaway) => {
                                    // shutdown io,prevent the creation of new stream
                                    self.controller.shutdown();
                                    io_manager.close_frame_receiver();
                                    let last_stream_id = goaway.get_last_stream_id();
                                    if self.next_stream_id.get_next_id() as usize <= last_stream_id
                                    {
                                        return Poll::Ready(Err(H2Error::ConnectionError(
                                            ErrorCode::ProtocolError,
                                        )));
                                    }
                                    self.goaway_unsent_stream(
                                        io_manager,
                                        last_stream_id as u32,
                                        frame.clone(),
                                    )?;
                                    continue;
                                }
                                Payload::RstStream(_reset) => {
                                    io_manager
                                        .streams
                                        .recv_remote_reset(frame.stream_id() as u32)?;
                                }
                                Payload::Headers(_headers) => {
                                    if let Closed(ResetReason::Local) =
                                        io_manager.streams.recv_headers(frame.stream_id() as u32)?
                                    {
                                        continue;
                                    }
                                }
                                Payload::Data(_data) => {
                                    if let Closed(ResetReason::Local) =
                                        io_manager.streams.recv_data(frame.stream_id() as u32)?
                                    {
                                        continue;
                                    }
                                }
                                // TODO Windows that processes streams and connections separately.
                                Payload::WindowUpdate(_windows) => {
                                    continue;
                                }
                                Payload::Priority(_priority) => continue,
                            }

                            let stream_id = frame.stream_id() as u32;
                            if stream_id == self.id {
                                meet_this = true;
                                self.controller_send_frame_to_stream(stream_id, frame, io_manager);
                                break;
                            } else {
                                self.controller_send_frame_to_stream(stream_id, frame, io_manager);
                                // TODO After adding frames such as Reset/Priority, there may be
                                // problems with the following logic, because the lack of waker
                                // cannot wake up
                                let mut stream_waker = self
                                    .controller
                                    .stream_waker
                                    .lock()
                                    .expect("Blocking get waker lock failed! ");
                                // TODO Is there a situation where the result has been returned, but
                                // the waker has not been inserted into the map? how to deal with.
                                if let Some(waker) = stream_waker.waker.remove(&stream_id) {
                                    waker.wake();
                                }
                            }
                        }
                    }
                }
            }

            if meet_this {
                io_manager.frame_iter.iter = Some(frame_iterator);
                Poll::Ready(Ok(DispatchState::Partial))
            } else {
                Poll::Ready(Ok(DispatchState::Finish))
            }
        }

        fn goaway_unsent_stream(
            &self,
            io_manager: &mut MutexGuard<IoManager<S>>,
            last_stream_id: u32,
            goaway: Frame,
        ) -> Result<(), H2Error> {
            let goaway_streams = io_manager.streams.get_goaway_streams(last_stream_id)?;
            {
                let mut stream_waker = self
                    .controller
                    .stream_waker
                    .lock()
                    .expect("Blocking get waker lock failed! ");
                for goaway_stream in goaway_streams {
                    self.controller_send_frame_to_stream(goaway_stream, goaway.clone(), io_manager);
                    if let Some(waker) = stream_waker.waker.remove(&goaway_stream) {
                        waker.wake();
                    }
                }
            }
            Ok(())
        }

        fn goaway_and_shutdown(&self) {
            {
                let mut waker_guard = self
                    .controller
                    .stream_waker
                    .lock()
                    .expect("Blocking get waker lock failed! ");
                let waker_map = take(&mut waker_guard.waker);
                for (_id, waker) in waker_map.into_iter() {
                    waker.wake()
                }
            }
        }

        fn recv_settings_frame(
            &self,
            cx: &mut Context<'_>,
            guard: &mut MutexGuard<IoManager<S>>,
            is_ack: bool,
            settings: &h2::Settings,
        ) -> Poll<Result<(), H2Error>> {
            if is_ack {
                match guard.connection_frame.settings.clone() {
                    SettingsSync::Acknowledging(local_settings) => {
                        for setting in local_settings.get_settings() {
                            if let Setting::MaxHeaderListSize(size) = setting {
                                guard.inner.decoder.set_max_header_list_size(*size as usize);
                            }
                            if let Setting::MaxFrameSize(size) = setting {
                                guard.inner.decoder.set_max_frame_size(*size)?;
                            }
                        }
                        guard.connection_frame.settings = SettingsSync::Synced;
                        Poll::Ready(Ok(()))
                    }
                    _ => Poll::Ready(Err(H2Error::ConnectionError(ErrorCode::ProtocolError))),
                }
            } else {
                for setting in settings.get_settings() {
                    if let Setting::HeaderTableSize(size) = setting {
                        guard.inner.encoder.update_header_table_size(*size as usize);
                    }
                    if let Setting::MaxFrameSize(size) = setting {
                        guard.inner.encoder.update_max_frame_size(*size as usize);
                    }
                }
                // reply ack Settings
                let mut buf = [0u8; 1024];
                if write_frame_to_io(cx, &mut buf, SettingsSync::ack_settings(), &mut guard.inner)?
                    .is_pending()
                {
                    Poll::Pending
                } else {
                    poll_flush_io(cx, &mut guard.inner)
                }
            }
        }

        fn recv_ping_frame(
            &self,
            cx: &mut Context<'_>,
            guard: &mut MutexGuard<IoManager<S>>,
            is_ack: bool,
            ping: &h2::Ping,
        ) -> Poll<Result<(), H2Error>> {
            if is_ack {
                // TODO The sending logic of ping has not been implemented yet, so there is no
                // processing for ack
                Poll::Ready(Ok(()))
            } else {
                // reply ack Settings
                let ack = Frame::new(0, FrameFlags::new(0x1), Payload::Ping(ping.clone()));
                let mut buf = [0u8; 1024];
                if write_frame_to_io(cx, &mut buf, ack, &mut guard.inner)?.is_pending() {
                    Poll::Pending
                } else {
                    poll_flush_io(cx, &mut guard.inner)
                }
            }
        }

        fn controller_send_frame_to_stream(
            &self,
            stream_id: u32,
            frame: Frame,
            guard: &mut MutexGuard<IoManager<S>>,
        ) {
            // TODO Need to consider when to delete useless Sender after support reset
            // stream
            if let Some(sender) = guard.senders.get(&stream_id) {
                // If the client coroutine has exited, this frame is skipped.
                let _ = sender.send(frame);
            }
        }
    }

    impl FrameReceiver {
        fn set_receiver(&mut self, receiver: UnboundedReceiver<Frame>) {
            self.receiver = Some(receiver);
        }

        fn recv_frame(&mut self, id: u32) -> Result<Option<Frame>, HttpError> {
            if let Some(ref mut receiver) = self.receiver {
                match receiver.try_recv() {
                    Ok(frame) => Ok(Some(frame)),
                    Err(TryRecvError::Disconnected) => {
                        Err(H2Error::StreamError(id, ErrorCode::StreamClosed).into())
                    }
                    Err(TryRecvError::Empty) => Ok(None),
                }
            } else {
                Err(H2Error::StreamError(id, ErrorCode::IntervalError).into())
            }
        }

        fn is_none(&self) -> bool {
            self.receiver.is_none()
        }
    }

    // TODO Temporarily only deal with the Settings frame
    pub(crate) fn build_connection_frames(config: H2Config) -> ConnectionFrames {
        const DEFAULT_ENABLE_PUSH: bool = false;
        let settings = SettingsBuilder::new()
            .max_header_list_size(config.max_header_list_size())
            .max_frame_size(config.max_frame_size())
            .header_table_size(config.header_table_size())
            .enable_push(DEFAULT_ENABLE_PUSH)
            .build();

        ConnectionFrames::new(settings)
    }

    // io write interface
    fn write_frame_to_io<S>(
        cx: &mut Context<'_>,
        buf: &mut [u8],
        frame: Frame,
        inner: &mut Inner<S>,
    ) -> Poll<Result<(), H2Error>>
    where
        S: AsyncRead + AsyncWrite + Unpin + Sync + Send + 'static,
    {
        let mut remain_size = 0;
        inner.encoder.set_frame(frame);
        loop {
            let size = inner
                .encoder
                .encode(&mut buf[remain_size..])
                .map_err(|_| H2Error::ConnectionError(ErrorCode::IntervalError))?;

            let total = size + remain_size;

            // All the bytes of the frame are written
            if total == 0 {
                break;
            }
            match Pin::new(&mut inner.io)
                .poll_write(cx, &buf[..total])
                .map_err(|_| H2Error::ConnectionError(ErrorCode::IntervalError))?
            {
                Poll::Ready(written) => {
                    remain_size = total - written;
                    // written is not necessarily equal to total
                    if remain_size > 0 {
                        for i in 0..remain_size {
                            buf[i] = buf[written + i];
                        }
                    }
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }
        Poll::Ready(Ok(()))
    }

    fn poll_flush_io<S>(cx: &mut Context<'_>, inner: &mut Inner<S>) -> Poll<Result<(), H2Error>>
    where
        S: AsyncRead + AsyncWrite + Unpin + Sync + Send + 'static,
    {
        Pin::new(&mut inner.io)
            .poll_flush(cx)
            .map_err(|_| H2Error::ConnectionError(ErrorCode::ConnectError))
    }

    fn get_frame(frame: Option<Frame>) -> Result<Frame, HttpError> {
        frame.ok_or(H2Error::ConnectionError(ErrorCode::IntervalError).into())
    }

    fn wakeup_next_stream(waker_map: &mut HashMap<u32, Waker>) {
        {
            if !waker_map.is_empty() {
                let mut id = 0;
                if let Some((index, _)) = waker_map.iter().next() {
                    id = *index;
                }
                if let Some(waker) = waker_map.remove(&id) {
                    waker.wake();
                }
            }
        }
    }

    fn is_io_available(occupied: &AtomicU32, id: u32) -> Result<bool, HttpError> {
        let is_occupied = occupied.load(Ordering::Relaxed);
        if is_occupied == 0 {
            return Ok(true);
        }
        if is_occupied == id {
            compare_exchange_occupation(occupied, id, 0)?;
            return Ok(true);
        }
        Ok(false)
    }

    fn compare_exchange_occupation(
        occupied: &AtomicU32,
        current: u32,
        new: u32,
    ) -> Result<(), HttpError> {
        occupied
            .compare_exchange(current, new, Ordering::Acquire, Ordering::Relaxed)
            .map_err(|_| H2Error::ConnectionError(ErrorCode::IntervalError))?;
        Ok(())
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
