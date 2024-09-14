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

#[cfg(feature = "http3")]
use ylong_http::request::uri::Authority;
#[cfg(feature = "http2")]
use ylong_http::request::uri::Scheme;
use ylong_http::request::uri::Uri;

use crate::async_impl::connector::ConnInfo;
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
use crate::util::dispatcher::{Conn, ConnDispatcher, Dispatcher};
use crate::util::pool::{Pool, PoolKey};
#[cfg(feature = "http3")]
use crate::util::request::RequestArc;

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

    pub(crate) async fn connect_to(&self, uri: &Uri) -> Result<Conn<C::Stream>, HttpClientError> {
        let key = PoolKey::new(
            uri.scheme().unwrap().clone(),
            uri.authority().unwrap().clone(),
        );

        #[cfg(feature = "http3")]
        let alt_svc = self.alt_svcs.get_alt_svcs(&key);

        self.pool
            .get(key, Conns::new)
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

pub(crate) struct Conns<S> {
    list: Arc<Mutex<Vec<ConnDispatcher<S>>>>,
    #[cfg(feature = "http2")]
    h2_conn: Arc<crate::runtime::AsyncMutex<Vec<ConnDispatcher<S>>>>,
    #[cfg(feature = "http3")]
    h3_conn: Arc<crate::runtime::AsyncMutex<Vec<ConnDispatcher<S>>>>,
}

impl<S> Conns<S> {
    fn new() -> Self {
        Self {
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
    ) -> Result<Conn<S>, HttpClientError>
    where
        C: Connector<Stream = S>,
    {
        match config.version {
            #[cfg(feature = "http3")]
            HttpVersion::Http3 => self.conn_h3(connector, url, config.http3_config).await,
            #[cfg(feature = "http2")]
            HttpVersion::Http2 => self.conn_h2(connector, url, config.http2_config).await,
            #[cfg(feature = "http1_1")]
            HttpVersion::Http1 => self.conn_h1(connector, url).await,
            HttpVersion::Negotiate => {
                #[cfg(feature = "http3")]
                if let Some(conn) = self
                    .conn_alt_svc(&connector, url, alt_svc, config.http3_config)
                    .await
                {
                    return Ok(conn);
                }

                #[cfg(all(feature = "http1_1", not(feature = "http2")))]
                return self.conn_h1(connector, url).await;

                #[cfg(all(feature = "http2", feature = "http1_1"))]
                return self
                    .conn_negotiate(connector, url, config.http2_config)
                    .await;
            }
        }
    }

    async fn conn_h1<C>(&self, connector: Arc<C>, url: &Uri) -> Result<Conn<S>, HttpClientError>
    where
        C: Connector<Stream = S>,
    {
        if let Some(conn) = self.exist_h1_conn() {
            return Ok(conn);
        }
        let dispatcher = ConnDispatcher::http1(connector.connect(url, HttpVersion::Http1).await?);
        Ok(self.dispatch_h1_conn(dispatcher))
    }

    #[cfg(feature = "http2")]
    async fn conn_h2<C>(
        &self,
        connector: Arc<C>,
        url: &Uri,
        config: H2Config,
    ) -> Result<Conn<S>, HttpClientError>
    where
        C: Connector<Stream = S>,
    {
        // The lock `h2_occupation` is used to prevent multiple coroutines from sending
        // Requests at the same time under concurrent conditions,
        // resulting in the creation of multiple tcp connections
        let mut lock = self.h2_conn.lock().await;

        if let Some(conn) = Self::exist_h2_conn(&mut lock) {
            return Ok(conn);
        }
        let stream = connector.connect(url, HttpVersion::Http2).await?;
        let details = stream.conn_detail();
        let tls = if let Some(scheme) = url.scheme() {
            *scheme == Scheme::HTTPS
        } else {
            false
        };
        match details.alpn() {
            None if tls => return err_from_msg!(Connect, "The peer does not support http/2."),
            Some(protocol) if protocol != b"h2" => {
                return err_from_msg!(Connect, "Alpn negotiate a wrong protocol version.")
            }
            _ => {}
        }

        Ok(Self::dispatch_h2_conn(config, stream, &mut lock))
    }

    #[cfg(feature = "http3")]
    async fn conn_h3<C>(
        &self,
        connector: Arc<C>,
        url: &Uri,
        config: H3Config,
    ) -> Result<Conn<S>, HttpClientError>
    where
        C: Connector<Stream = S>,
    {
        let mut lock = self.h3_conn.lock().await;

        if let Some(conn) = Self::exist_h3_conn(&mut lock) {
            return Ok(conn);
        }
        let mut stream = connector.connect(url, HttpVersion::Http3).await?;
        let quic_conn = stream.quic_conn().ok_or(HttpClientError::from_str(
            crate::ErrorKind::Connect,
            "QUIC connect failed",
        ))?;

        Ok(Self::dispatch_h3_conn(config, stream, quic_conn, &mut lock))
    }

    #[cfg(all(feature = "http2", feature = "http1_1"))]
    async fn conn_negotiate<C>(
        &self,
        connector: Arc<C>,
        url: &Uri,
        h2_config: H2Config,
    ) -> Result<Conn<S>, HttpClientError>
    where
        C: Connector<Stream = S>,
    {
        match *url.scheme().unwrap() {
            Scheme::HTTPS => {
                let mut lock = self.h2_conn.lock().await;
                if let Some(conn) = Self::exist_h2_conn(&mut lock) {
                    return Ok(conn);
                }

                if let Some(conn) = self.exist_h1_conn() {
                    return Ok(conn);
                }

                let stream = connector.connect(url, HttpVersion::Negotiate).await?;
                let details = stream.conn_detail();

                let protocol = if let Some(bytes) = details.alpn() {
                    bytes
                } else {
                    let dispatcher = ConnDispatcher::http1(stream);
                    return Ok(self.dispatch_h1_conn(dispatcher));
                };

                if protocol == b"http/1.1" {
                    let dispatcher = ConnDispatcher::http1(stream);
                    Ok(self.dispatch_h1_conn(dispatcher))
                } else if protocol == b"h2" {
                    Ok(Self::dispatch_h2_conn(h2_config, stream, &mut lock))
                } else {
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
    ) -> Option<Conn<S>>
    where
        C: Connector<Stream = S>,
    {
        let mut lock = self.h3_conn.lock().await;
        if let Some(conn) = Self::exist_h3_conn(&mut lock) {
            return Some(conn);
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
                return Some(Self::dispatch_h3_conn(
                    h3_config.clone(),
                    stream,
                    quic_conn,
                    &mut lock,
                ));
            }
        }
        None
    }

    fn dispatch_h1_conn(&self, dispatcher: ConnDispatcher<S>) -> Conn<S> {
        // We must be able to get the `Conn` here.
        let conn = dispatcher.dispatch().unwrap();
        let mut list = self.list.lock().unwrap();
        list.push(dispatcher);

        conn
    }

    #[cfg(feature = "http2")]
    fn dispatch_h2_conn(
        config: H2Config,
        stream: S,
        lock: &mut crate::runtime::MutexGuard<Vec<ConnDispatcher<S>>>,
    ) -> Conn<S> {
        let dispatcher = ConnDispatcher::http2(config, stream);
        let conn = dispatcher.dispatch().unwrap();
        lock.push(dispatcher);
        conn
    }

    #[cfg(feature = "http3")]
    fn dispatch_h3_conn(
        config: H3Config,
        stream: S,
        quic_connection: QuicConn,
        lock: &mut crate::runtime::MutexGuard<Vec<ConnDispatcher<S>>>,
    ) -> Conn<S> {
        let dispatcher = ConnDispatcher::http3(config, stream, quic_connection);
        let conn = dispatcher.dispatch().unwrap();
        lock.push(dispatcher);
        conn
    }

    fn exist_h1_conn(&self) -> Option<Conn<S>> {
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
        conn
    }

    #[cfg(feature = "http2")]
    fn exist_h2_conn(
        lock: &mut crate::runtime::MutexGuard<Vec<ConnDispatcher<S>>>,
    ) -> Option<Conn<S>> {
        if let Some(dispatcher) = lock.pop() {
            // todo: shutdown and goaway
            if !dispatcher.is_shutdown() {
                if let Some(conn) = dispatcher.dispatch() {
                    lock.push(dispatcher);
                    return Some(conn);
                }
            }
        }
        None
    }

    #[cfg(feature = "http3")]
    fn exist_h3_conn(
        lock: &mut crate::runtime::MutexGuard<Vec<ConnDispatcher<S>>>,
    ) -> Option<Conn<S>> {
        if let Some(dispatcher) = lock.pop() {
            if dispatcher.is_shutdown() {
                return None;
            }
            if !dispatcher.is_goaway() {
                if let Some(conn) = dispatcher.dispatch() {
                    lock.push(dispatcher);
                    return Some(conn);
                }
            }
            // Not all requests have been processed yet
            lock.push(dispatcher);
        }
        None
    }
}
