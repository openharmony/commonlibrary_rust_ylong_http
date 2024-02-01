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

use std::error::Error;
use std::future::Future;
use std::mem::take;
use std::sync::{Arc, Mutex};

use crate::async_impl::Connector;
use crate::error::HttpClientError;
use crate::util::dispatcher::{Conn, ConnDispatcher, Dispatcher};
use crate::util::pool::{Pool, PoolKey};
use crate::{AsyncRead, AsyncWrite, ErrorKind, HttpConfig, HttpVersion, Uri};

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

    pub(crate) async fn connect_to(&self, uri: Uri) -> Result<Conn<C::Stream>, HttpClientError> {
        let key = PoolKey::new(
            uri.scheme().unwrap().clone(),
            uri.authority().unwrap().clone(),
        );

        self.pool
            .get(key, Conns::new)
            .conn(self.config.clone(), self.connector.clone().connect(&uri))
            .await
    }
}

pub(crate) struct Conns<S> {
    list: Arc<Mutex<Vec<ConnDispatcher<S>>>>,

    #[cfg(feature = "http2")]
    h2_occupation: Arc<crate::AsyncMutex<()>>,
}

impl<S> Conns<S> {
    fn new() -> Self {
        Self {
            list: Arc::new(Mutex::new(Vec::new())),

            #[cfg(feature = "http2")]
            h2_occupation: Arc::new(crate::AsyncMutex::new(())),
        }
    }
}

impl<S> Clone for Conns<S> {
    fn clone(&self) -> Self {
        Self {
            list: self.list.clone(),

            #[cfg(feature = "http2")]
            h2_occupation: self.h2_occupation.clone(),
        }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send + Sync> Conns<S> {
    async fn conn<F, E>(
        &self,
        config: HttpConfig,
        connect_fut: F,
    ) -> Result<Conn<S>, HttpClientError>
    where
        F: Future<Output = Result<S, E>>,
        E: Into<Box<dyn Error + Send + Sync>>,
    {
        match config.version {
            #[cfg(feature = "http2")]
            HttpVersion::Http2PriorKnowledge => {
                {
                    // The lock `h2_occupation` is used to prevent multiple coroutines from sending
                    // Requests at the same time under concurrent conditions,
                    // resulting in the creation of multiple tcp connections
                    let _lock = self.h2_occupation.lock().await;
                    if let Some(conn) = self.get_exist_conn() {
                        return Ok(conn);
                    }
                    // create tcp connection.
                    let dispatcher = ConnDispatcher::http2(
                        config.http2_config,
                        connect_fut.await.map_err(|e| {
                            HttpClientError::new_with_cause(ErrorKind::Connect, Some(e))
                        })?,
                    );
                    Ok(self.dispatch_conn(dispatcher))
                }
            }
            #[cfg(feature = "http1_1")]
            HttpVersion::Http1 => {
                if let Some(conn) = self.get_exist_conn() {
                    return Ok(conn);
                }
                let dispatcher =
                    ConnDispatcher::http1(connect_fut.await.map_err(|e| {
                        HttpClientError::new_with_cause(ErrorKind::Connect, Some(e))
                    })?);
                Ok(self.dispatch_conn(dispatcher))
            }
            #[cfg(not(feature = "http1_1"))]
            HttpVersion::Http1 => Err(HttpClientError::new_with_message(
                ErrorKind::Connect,
                "Invalid HTTP VERSION",
            )),
        }
    }

    fn dispatch_conn(&self, dispatcher: ConnDispatcher<S>) -> Conn<S> {
        // We must be able to get the `Conn` here.
        let conn = dispatcher.dispatch().unwrap();
        let mut list = self.list.lock().unwrap();
        list.push(dispatcher);
        conn
    }

    fn get_exist_conn(&self) -> Option<Conn<S>> {
        {
            let mut list = self.list.lock().unwrap();
            let mut conn = None;
            let curr = take(&mut *list);
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
    }
}
