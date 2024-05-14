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

use ylong_http::request::uri::Uri;

use crate::async_impl::connector::ConnInfo;
use crate::async_impl::Connector;
use crate::error::{ErrorKind, HttpClientError};
use crate::runtime::{AsyncRead, AsyncWrite};
#[cfg(feature = "http2")]
use crate::util::config::H2Config;
use crate::util::config::{HttpConfig, HttpVersion};
use crate::util::dispatcher::{Conn, ConnDispatcher, Dispatcher};
use crate::util::pool::{Pool, PoolKey};

pub(crate) struct ConnPool<C, S> {
    pool: Pool<PoolKey, Conns<S>>,
    connector: Arc<C>,
    config: HttpConfig,
}

impl<C: Connector> ConnPool<C, C::Stream> {
    pub(crate) fn new(config: HttpConfig, connector: C) -> Self {
        Self {
            pool: Pool::new(),
            connector: Arc::new(connector),
            config,
        }
    }

    pub(crate) async fn connect_to(&self, uri: &Uri) -> Result<Conn<C::Stream>, HttpClientError> {
        let key = PoolKey::new(
            uri.scheme().unwrap().clone(),
            uri.authority().unwrap().clone(),
        );

        self.pool
            .get(key, Conns::new)
            .conn(self.config.clone(), self.connector.clone(), uri)
            .await
    }
}

pub(crate) struct Conns<S> {
    list: Arc<Mutex<Vec<ConnDispatcher<S>>>>,
    #[cfg(feature = "http2")]
    h2_conn: Arc<crate::runtime::AsyncMutex<Vec<ConnDispatcher<S>>>>,
}

impl<S> Conns<S> {
    fn new() -> Self {
        Self {
            list: Arc::new(Mutex::new(Vec::new())),

            #[cfg(feature = "http2")]
            h2_conn: Arc::new(crate::runtime::AsyncMutex::new(Vec::with_capacity(1))),
        }
    }
}

impl<S> Clone for Conns<S> {
    fn clone(&self) -> Self {
        Self {
            list: self.list.clone(),

            #[cfg(feature = "http2")]
            h2_conn: self.h2_conn.clone(),
        }
    }
}

impl<S: AsyncRead + AsyncWrite + ConnInfo + Unpin + Send + Sync + 'static> Conns<S> {
    async fn conn<C>(
        &mut self,
        config: HttpConfig,
        connector: Arc<C>,
        url: &Uri,
    ) -> Result<Conn<S>, HttpClientError>
    where
        C: Connector<Stream = S>,
    {
        #[cfg(feature = "http2")]
        use ylong_http::request::uri::Scheme;

        match config.version {
            #[cfg(feature = "http2")]
            HttpVersion::Http2 => {
                {
                    // The lock `h2_occupation` is used to prevent multiple coroutines from sending
                    // Requests at the same time under concurrent conditions,
                    // resulting in the creation of multiple tcp connections
                    let mut lock = self.h2_conn.lock().await;

                    if let Some(conn) = Self::exist_h2_conn(&mut lock) {
                        return Ok(conn);
                    }
                    let stream = connector
                        .connect(url)
                        .await
                        .map_err(|e| HttpClientError::from_error(ErrorKind::Connect, e))?;
                    let details = stream.conn_detail();
                    let tls = if let Some(scheme) = url.scheme() {
                        *scheme == Scheme::HTTPS
                    } else {
                        false
                    };
                    match details.alpn() {
                        None if tls => {
                            return err_from_msg!(Connect, "The peer does not support http/2.")
                        }
                        Some(protocol) if protocol != b"h2" => {
                            return err_from_msg!(
                                Connect,
                                "Alpn negotiate a wrong protocol version."
                            )
                        }
                        _ => {}
                    }

                    Ok(Self::dispatch_h2_conn(
                        config.http2_config,
                        stream,
                        &mut lock,
                    ))
                }
            }
            #[cfg(feature = "http1_1")]
            HttpVersion::Http1 => self.conn_h1(connector, url).await,
            HttpVersion::Negotiate => {
                #[cfg(all(feature = "http2", feature = "http1_1"))]
                match *url.scheme().unwrap() {
                    Scheme::HTTPS => {
                        let mut lock = self.h2_conn.lock().await;

                        if let Some(conn) = Self::exist_h2_conn(&mut lock) {
                            return Ok(conn);
                        }

                        if let Some(conn) = self.get_exist_conn() {
                            return Ok(conn);
                        }

                        let stream = connector
                            .connect(url)
                            .await
                            .map_err(|e| HttpClientError::from_error(ErrorKind::Connect, e))?;
                        let details = stream.conn_detail();
                        match details.alpn() {
                            None => {
                                let dispatcher = ConnDispatcher::http1(stream);
                                Ok(self.dispatch_h1_conn(dispatcher))
                            }
                            Some(protocol) => {
                                if protocol == b"http/1.1" {
                                    let dispatcher = ConnDispatcher::http1(stream);
                                    Ok(self.dispatch_h1_conn(dispatcher))
                                } else if protocol == b"h2" {
                                    Ok(Self::dispatch_h2_conn(
                                        config.http2_config,
                                        stream,
                                        &mut lock,
                                    ))
                                } else {
                                    err_from_msg!(
                                        Connect,
                                        "Alpn negotiate a wrong protocol version."
                                    )
                                }
                            }
                        }
                    }
                    Scheme::HTTP => self.conn_h1(connector, url).await,
                }

                #[cfg(all(feature = "http1_1", not(feature = "http2")))]
                self.conn_h1(connector, url).await
            }
        }
    }

    async fn conn_h1<C>(&self, connector: Arc<C>, url: &Uri) -> Result<Conn<S>, HttpClientError>
    where
        C: Connector<Stream = S>,
    {
        if let Some(conn) = self.get_exist_conn() {
            return Ok(conn);
        }
        let dispatcher = ConnDispatcher::http1(
            connector
                .connect(url)
                .await
                .map_err(|e| HttpClientError::from_error(ErrorKind::Connect, e))?,
        );
        Ok(self.dispatch_h1_conn(dispatcher))
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

    fn get_exist_conn(&self) -> Option<Conn<S>> {
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
            if !dispatcher.is_shutdown() {
                if let Some(conn) = dispatcher.dispatch() {
                    lock.push(dispatcher);
                    return Some(conn);
                }
            }
        }
        None
    }
}
