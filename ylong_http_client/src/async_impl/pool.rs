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

use std::mem::take;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[cfg(feature = "http3")]
use ylong_http::request::uri::Authority;
#[cfg(any(feature = "http2", feature = "http3"))]
use ylong_http::request::uri::Scheme;
use ylong_http::request::uri::Uri;

#[cfg(feature = "http3")]
use crate::async_impl::quic::QuicConn;
use crate::async_impl::Connector;
#[cfg(feature = "http3")]
use crate::async_impl::Response;
use crate::error::HttpClientError;
use crate::runtime::{AsyncRead, AsyncWrite};
#[cfg(feature = "http3")]
use crate::util::alt_svc::{AltService, AltServiceMap};
#[cfg(feature = "http2")]
use crate::util::config::H2Config;
#[cfg(feature = "http3")]
use crate::util::config::H3Config;
use crate::util::config::{HttpConfig, HttpVersion};
use crate::util::dispatcher::http1::{WrappedSemPermit, WrappedSemaphore};
use crate::util::dispatcher::{Conn, ConnDispatcher, Dispatcher, TimeInfoConn};
use crate::util::pool::{Pool, PoolKey};
use crate::util::progress::SpeedConfig;
#[cfg(feature = "http3")]
use crate::util::request::RequestArc;
use crate::util::ConnInfo;
#[cfg(feature = "http2")]
use crate::ConnDetail;
use crate::TimeGroup;

pub(crate) struct ConnPool<C, S> {
    pool: Pool<PoolKey, Conns<S>>,
    #[cfg(feature = "http3")]
    alt_svcs: AltServiceMap,
    connector: Arc<C>,
    config: HttpConfig,
}

impl<C: Connector> ConnPool<C, C::Stream> {
    pub(crate) fn new(config: HttpConfig, connector: C) -> Self {
        Self {
            pool: Pool::new(),
            #[cfg(feature = "http3")]
            alt_svcs: AltServiceMap::new(),
            connector: Arc::new(connector),
            config,
        }
    }

    pub(crate) async fn connect_to(
        &self,
        uri: &Uri,
    ) -> Result<TimeInfoConn<C::Stream>, HttpClientError> {
        let key = PoolKey::new(
            uri.scheme().unwrap().clone(),
            uri.authority().unwrap().clone(),
        );

        #[cfg(feature = "http3")]
        let alt_svc = self.alt_svcs.get_alt_svcs(&key);
        self.pool
            .get(
                key,
                Conns::new,
                self.config.http1_config.max_conn_num(),
                self.config.speed_config,
            )
            .conn(
                self.config.clone(),
                self.connector.clone(),
                uri,
                #[cfg(feature = "http3")]
                alt_svc,
            )
            .await
    }

    #[cfg(feature = "http3")]
    pub(crate) fn set_alt_svcs(&self, request: RequestArc, response: &Response) {
        self.alt_svcs.set_alt_svcs(request, response);
    }
}

pub(crate) enum H1ConnOption<T> {
    Some(T),
    None(WrappedSemPermit),
}

pub(crate) struct Conns<S> {
    speed_config: SpeedConfig,
    usable: WrappedSemaphore,
    list: Arc<Mutex<Vec<ConnDispatcher<S>>>>,
    #[cfg(feature = "http2")]
    h2_conn: Arc<crate::runtime::AsyncMutex<Vec<ConnDispatcher<S>>>>,
    #[cfg(feature = "http3")]
    h3_conn: Arc<crate::runtime::AsyncMutex<Vec<ConnDispatcher<S>>>>,
}

impl<S> Conns<S> {
    fn new(max_conn_num: usize, speed_config: SpeedConfig) -> Self {
        Self {
            speed_config,
            usable: WrappedSemaphore::new(max_conn_num),

            list: Arc::new(Mutex::new(Vec::new())),

            #[cfg(feature = "http2")]
            h2_conn: Arc::new(crate::runtime::AsyncMutex::new(Vec::with_capacity(1))),

            #[cfg(feature = "http3")]
            h3_conn: Arc::new(crate::runtime::AsyncMutex::new(Vec::with_capacity(1))),
        }
    }

    // fn get_alt_svcs
}

impl<S> Clone for Conns<S> {
    fn clone(&self) -> Self {
        Self {
            speed_config: self.speed_config,
            usable: self.usable.clone(),
            list: self.list.clone(),

            #[cfg(feature = "http2")]
            h2_conn: self.h2_conn.clone(),

            #[cfg(feature = "http3")]
            h3_conn: self.h3_conn.clone(),
        }
    }
}

impl<S: AsyncRead + AsyncWrite + ConnInfo + Unpin + Send + Sync + 'static> Conns<S> {
    async fn conn<C>(
        &mut self,
        config: HttpConfig,
        connector: Arc<C>,
        url: &Uri,
        #[cfg(feature = "http3")] alt_svc: Option<Vec<AltService>>,
    ) -> Result<TimeInfoConn<S>, HttpClientError>
    where
        C: Connector<Stream = S>,
    {
        let conn_start = Instant::now();
        let mut conn = match config.version {
            #[cfg(feature = "http3")]
            HttpVersion::Http3 => self.conn_h3(connector, url, config.http3_config).await,
            #[cfg(feature = "http2")]
            HttpVersion::Http2 => self.conn_h2(connector, url, config.http2_config).await,
            #[cfg(feature = "http1_1")]
            HttpVersion::Http1 => self.conn_h1(connector, url).await,
            #[cfg(all(feature = "http1_1", not(feature = "http2")))]
            HttpVersion::Negotiate => self.conn_h1(connector, url).await,
            #[cfg(all(feature = "http1_1", feature = "http2"))]
            HttpVersion::Negotiate => {
                #[cfg(feature = "http3")]
                if let Some(mut conn) = self
                    .conn_alt_svc(&connector, url, alt_svc, config.http3_config)
                    .await
                {
                    conn.time_group_mut().set_connect_start(conn_start);
                    conn.time_group_mut().set_connect_end(Instant::now());
                    return Ok(conn);
                }
                self.conn_negotiate(connector, url, config.http2_config)
                    .await
            }
        }?;
        conn.time_group_mut().set_connect_start(conn_start);
        conn.time_group_mut().set_connect_end(Instant::now());
        Ok(conn)
    }

    async fn conn_h1<C>(
        &self,
        connector: Arc<C>,
        url: &Uri,
    ) -> Result<TimeInfoConn<S>, HttpClientError>
    where
        C: Connector<Stream = S>,
    {
        let semaphore = self.usable.acquire().await;
        match self.exist_h1_conn(semaphore) {
            H1ConnOption::Some(conn) => Ok(TimeInfoConn::new(conn, TimeGroup::default())),
            H1ConnOption::None(permit) => {
                let stream = connector.connect(url, HttpVersion::Http1).await?;
                let time_group = take(stream.conn_data().time_group_mut());

                let dispatcher = ConnDispatcher::http1(stream);
                let conn = self.dispatch_h1_conn(dispatcher, permit);
                Ok(TimeInfoConn::new(conn, time_group))
            }
        }
    }

    #[cfg(feature = "http2")]
    async fn conn_h2<C>(
        &self,
        connector: Arc<C>,
        url: &Uri,
        config: H2Config,
    ) -> Result<TimeInfoConn<S>, HttpClientError>
    where
        C: Connector<Stream = S>,
    {
        // The lock `h2_occupation` is used to prevent multiple coroutines from sending
        // Requests at the same time under concurrent conditions,
        // resulting in the creation of multiple tcp connections
        let mut lock = self.h2_conn.lock().await;

        if let Some(conn) = self.exist_h2_conn(&mut lock) {
            return Ok(TimeInfoConn::new(conn, TimeGroup::default()));
        }
        let stream = connector.connect(url, HttpVersion::Http2).await?;
        let mut data = stream.conn_data();
        let tls = if let Some(scheme) = url.scheme() {
            *scheme == Scheme::HTTPS
        } else {
            false
        };
        match data.negotiate().alpn() {
            None if tls => return err_from_msg!(Connect, "The peer does not support http/2."),
            Some(protocol) if protocol != b"h2" => {
                return err_from_msg!(Connect, "Alpn negotiate a wrong protocol version.")
            }
            _ => {}
        }
        let time_group = take(data.time_group_mut());
        let conn = self.dispatch_h2_conn(data.detail(), config, stream, &mut lock);
        Ok(TimeInfoConn::new(conn, time_group))
    }

    #[cfg(feature = "http3")]
    async fn conn_h3<C>(
        &self,
        connector: Arc<C>,
        url: &Uri,
        config: H3Config,
    ) -> Result<TimeInfoConn<S>, HttpClientError>
    where
        C: Connector<Stream = S>,
    {
        let mut lock = self.h3_conn.lock().await;

        if let Some(conn) = self.exist_h3_conn(&mut lock) {
            return Ok(TimeInfoConn::new(conn, TimeGroup::default()));
        }
        let mut stream = connector.connect(url, HttpVersion::Http3).await?;

        let quic_conn = stream.quic_conn().ok_or(HttpClientError::from_str(
            crate::ErrorKind::Connect,
            "QUIC connect failed",
        ))?;

        let mut data = stream.conn_data();
        let time_group = take(data.time_group_mut());
        Ok(TimeInfoConn::new(
            self.dispatch_h3_conn(data.detail(), config, stream, quic_conn, &mut lock),
            time_group,
        ))
    }

    #[cfg(all(feature = "http2", feature = "http1_1"))]
    async fn conn_negotiate<C>(
        &self,
        connector: Arc<C>,
        url: &Uri,
        h2_config: H2Config,
    ) -> Result<TimeInfoConn<S>, HttpClientError>
    where
        C: Connector<Stream = S>,
    {
        match *url.scheme().unwrap() {
            Scheme::HTTPS => {
                let mut lock = self.h2_conn.lock().await;
                if let Some(conn) = self.exist_h2_conn(&mut lock) {
                    return Ok(TimeInfoConn::new(conn, TimeGroup::default()));
                }
                let permit = self.usable.acquire().await;
                let permit = match self.exist_h1_conn(permit) {
                    H1ConnOption::Some(conn) => {
                        return Ok(TimeInfoConn::new(conn, TimeGroup::default()));
                    }
                    H1ConnOption::None(permit) => permit,
                };
                let stream = connector.connect(url, HttpVersion::Negotiate).await?;
                let mut data = stream.conn_data();
                let time_group = take(data.time_group_mut());

                let protocol = data.negotiate().alpn().unwrap_or(b"http/1.1");
                if protocol == b"http/1.1" {
                    let dispatcher = ConnDispatcher::http1(stream);
                    Ok(TimeInfoConn::new(
                        self.dispatch_h1_conn(dispatcher, permit),
                        time_group,
                    ))
                } else if protocol == b"h2" {
                    std::mem::drop(permit);
                    let conn = self.dispatch_h2_conn(data.detail(), h2_config, stream, &mut lock);
                    Ok(TimeInfoConn::new(conn, time_group))
                } else {
                    std::mem::drop(permit);
                    err_from_msg!(Connect, "Alpn negotiate a wrong protocol version.")
                }
            }
            Scheme::HTTP => self.conn_h1(connector, url).await,
        }
    }

    #[cfg(feature = "http3")]
    async fn conn_alt_svc<C>(
        &self,
        connector: &Arc<C>,
        url: &Uri,
        alt_svcs: Option<Vec<AltService>>,
        h3_config: H3Config,
    ) -> Option<TimeInfoConn<S>>
    where
        C: Connector<Stream = S>,
    {
        let mut lock = self.h3_conn.lock().await;
        if let Some(conn) = self.exist_h3_conn(&mut lock) {
            return Some(TimeInfoConn::new(conn, TimeGroup::default()));
        }
        if let Some(alt_svcs) = alt_svcs {
            for alt_svc in alt_svcs {
                // only support h3 alt_svc now
                if alt_svc.http_version != HttpVersion::Http3 {
                    continue;
                }
                let scheme = Scheme::HTTPS;
                let host = match alt_svc.host {
                    Some(ref host) => host.clone(),
                    None => url.host().cloned().unwrap(),
                };
                let port = alt_svc.port.clone();
                let authority =
                    Authority::from_bytes((host.to_string() + ":" + port.as_str()).as_bytes())
                        .ok()?;
                let path = url.path().cloned();
                let query = url.query().cloned();
                let alt_url = Uri::from_raw_parts(Some(scheme), Some(authority), path, query);
                let mut stream = connector.connect(&alt_url, HttpVersion::Http3).await.ok()?;
                let quic_conn = stream.quic_conn().unwrap();
                let mut data = stream.conn_data();
                let time_group = take(data.time_group_mut());
                return Some(TimeInfoConn::new(
                    self.dispatch_h3_conn(
                        data.detail(),
                        h3_config.clone(),
                        stream,
                        quic_conn,
                        &mut lock,
                    ),
                    time_group,
                ));
            }
        }
        None
    }

    fn dispatch_h1_conn(&self, dispatcher: ConnDispatcher<S>, permit: WrappedSemPermit) -> Conn<S> {
        // We must be able to get the `Conn` here.
        let mut conn = dispatcher.dispatch().unwrap();
        let mut list = self.list.lock().unwrap();
        list.push(dispatcher);
        #[cfg(any(feature = "http2", feature = "http3"))]
        if let Conn::Http1(ref mut h1) = conn {
            h1.speed_controller.set_speed_limit(self.speed_config);
            h1.occupy_sem(permit)
        }
        #[cfg(all(not(feature = "http2"), not(feature = "http3")))]
        {
            let Conn::Http1(ref mut h1) = conn;
            h1.speed_controller.set_speed_limit(self.speed_config);
            h1.occupy_sem(permit)
        }
        conn
    }

    #[cfg(feature = "http2")]
    fn dispatch_h2_conn(
        &self,
        detail: ConnDetail,
        config: H2Config,
        stream: S,
        lock: &mut crate::runtime::MutexGuard<Vec<ConnDispatcher<S>>>,
    ) -> Conn<S> {
        let dispatcher = ConnDispatcher::http2(detail, config, stream);
        let mut conn = dispatcher.dispatch().unwrap();
        lock.push(dispatcher);
        if let Conn::Http2(ref mut h2) = conn {
            h2.speed_controller.set_speed_limit(self.speed_config);
        }
        conn
    }

    #[cfg(feature = "http3")]
    fn dispatch_h3_conn(
        &self,
        detail: ConnDetail,
        config: H3Config,
        stream: S,
        quic_connection: QuicConn,
        lock: &mut crate::runtime::MutexGuard<Vec<ConnDispatcher<S>>>,
    ) -> Conn<S> {
        let dispatcher = ConnDispatcher::http3(detail, config, stream, quic_connection);
        let mut conn = dispatcher.dispatch().unwrap();
        lock.push(dispatcher);
        if let Conn::Http3(ref mut h3) = conn {
            h3.speed_controller.set_speed_limit(self.speed_config);
        }
        conn
    }

    fn exist_h1_conn(&self, permit: WrappedSemPermit) -> H1ConnOption<Conn<S>> {
        let mut list = self.list.lock().unwrap();
        let mut conn = None;
        let curr = take(&mut *list);
        // TODO Distinguish between http2 connections and http1 connections.
        for dispatcher in curr.into_iter() {
            // Discard invalid dispatchers.
            if dispatcher.is_shutdown() {
                continue;
            }
            if conn.is_none() {
                conn = dispatcher.dispatch();
            }
            list.push(dispatcher);
        }
        match conn {
            Some(Conn::Http1(mut h1)) => {
                h1.occupy_sem(permit);
                h1.speed_controller.set_speed_limit(self.speed_config);
                H1ConnOption::Some(Conn::Http1(h1))
            }
            _ => H1ConnOption::None(permit),
        }
    }

    #[cfg(feature = "http2")]
    fn exist_h2_conn(
        &self,
        lock: &mut crate::runtime::MutexGuard<Vec<ConnDispatcher<S>>>,
    ) -> Option<Conn<S>> {
        if let Some(dispatcher) = lock.pop() {
            if dispatcher.is_shutdown() {
                return None;
            }
            if !dispatcher.is_goaway() {
                if let Some(Conn::Http2(mut h2)) = dispatcher.dispatch() {
                    lock.push(dispatcher);
                    h2.speed_controller.set_speed_limit(self.speed_config);
                    return Some(Conn::Http2(h2));
                }
            }
            lock.push(dispatcher);
        }
        None
    }

    #[cfg(feature = "http3")]
    fn exist_h3_conn(
        &self,
        lock: &mut crate::runtime::MutexGuard<Vec<ConnDispatcher<S>>>,
    ) -> Option<Conn<S>> {
        if let Some(dispatcher) = lock.pop() {
            if dispatcher.is_shutdown() {
                return None;
            }
            if !dispatcher.is_goaway() {
                if let Some(Conn::Http3(mut h3)) = dispatcher.dispatch() {
                    lock.push(dispatcher);
                    h3.speed_controller.set_speed_limit(self.speed_config);
                    return Some(Conn::Http3(h3));
                }
            }
            // Not all requests have been processed yet
            lock.push(dispatcher);
        }
        None
    }
}
