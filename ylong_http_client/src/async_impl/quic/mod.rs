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

//! `ConnDetail` trait and `HttpStream` implementation.

use std::ffi::c_void;
use std::net::SocketAddr;
use std::ops::{Deref, DerefMut};
use std::ptr;

use libc::{
    in6_addr, in_addr, sa_family_t, size_t, sockaddr, sockaddr_in, sockaddr_in6, sockaddr_storage,
    socklen_t, AF_INET, AF_INET6,
};
use ylong_runtime::fastrand::fast_random;
use ylong_runtime::time::timeout;

use crate::c_openssl::ssl::{verify_server_cert, verify_server_root_cert};
use crate::runtime::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use crate::util::c_openssl::ssl::Ssl;
use crate::util::ConnInfo;
use crate::{ErrorKind, HttpClientError, TlsConfig};

const MAX_DATAGRAM_SIZE: usize = 1350;
const UDP_BUF_SIZE: usize = 65535;
const MAX_STREAM_DATA: u64 = 1_000_000;
const MAX_TOTAL_DATA: u64 = 10_000_000;
const MAX_STREAM_NUM: u64 = 100;
const MAX_IDLE_TIME: u64 = 5000;

pub struct QuicConn {
    inner: quiche::Connection,
}

impl QuicConn {
    fn quic_config() -> Result<quiche::Config, quiche::Error> {
        let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION)?;
        config.verify_peer(true);
        config.set_application_protos(quiche::h3::APPLICATION_PROTOCOL)?;
        config.set_max_idle_timeout(MAX_IDLE_TIME);
        config.set_max_recv_udp_payload_size(MAX_DATAGRAM_SIZE);
        config.set_max_send_udp_payload_size(MAX_DATAGRAM_SIZE);
        config.set_initial_max_data(MAX_TOTAL_DATA);
        config.set_initial_max_stream_data_bidi_local(MAX_STREAM_DATA);
        config.set_initial_max_stream_data_bidi_remote(MAX_STREAM_DATA);
        config.set_initial_max_stream_data_uni(MAX_STREAM_DATA);
        config.set_initial_max_streams_bidi(MAX_STREAM_NUM);
        config.set_initial_max_streams_uni(MAX_STREAM_NUM);
        config.set_disable_active_migration(true);
        Ok(config)
    }

    pub(crate) async fn connect<S>(
        stream: &mut S,
        tls_config: &TlsConfig,
        host: &str,
    ) -> Result<QuicConn, HttpClientError>
    where
        S: AsyncRead + AsyncWrite + ConnInfo + Unpin + Sync + Send + 'static,
    {
        let config = Self::quic_config()
            .map_err(|_| HttpClientError::from_str(ErrorKind::Connect, "Quic init error"))?;
        // Generate a random source connection ID for the connection.
        let mut scid = [0; quiche::MAX_CONN_ID_LEN];
        for byte in scid.iter_mut() {
            *byte = fast_random() as u8;
        }
        let scid = quiche::ConnectionId::from_ref(&scid);

        let local = stream.conn_data().detail().local();
        let peer = stream.conn_data().detail().peer();
        let mut c_local: sockaddr_storage = unsafe { std::mem::zeroed() };
        let c_local_size = Self::std_addr_to_c(&local, &mut c_local);
        let mut c_peer: sockaddr_storage = unsafe { std::mem::zeroed() };
        let c_peer_size = Self::std_addr_to_c(&peer, &mut c_peer);
        let mut new_ssl = tls_config.ssl_new(host).unwrap().into_inner();

        let conn = unsafe {
            quiche_conn_new_with_tls(
                scid.as_ptr(),
                scid.len() as size_t,
                ptr::null_mut(),
                0,
                &c_local as *const _ as *const sockaddr,
                c_local_size,
                &c_peer as *const _ as *const sockaddr,
                c_peer_size,
                &config as *const _ as *const c_void,
                new_ssl.get_raw_ptr() as *mut c_void,
                false,
            ) as *mut quiche::Connection
        };
        let mut conn = QuicConn {
            inner: unsafe { *Box::from_raw(conn) },
        };
        if let Err(e) = conn.connect_inner(stream, &mut new_ssl, tls_config).await {
            std::mem::forget(new_ssl);
            return Err(e);
        }
        std::mem::forget(new_ssl);
        if conn.is_established() {
            Ok(conn)
        } else {
            Err(HttpClientError::from_str(
                ErrorKind::Connect,
                "Quic connect error",
            ))
        }
    }

    async fn connect_inner<S>(
        &mut self,
        stream: &mut S,
        ssl: &mut Ssl,
        tls_config: &TlsConfig,
    ) -> Result<(), HttpClientError>
    where
        S: AsyncRead + AsyncWrite + ConnInfo + Unpin + Sync + Send + 'static,
    {
        let mut buf = [0; UDP_BUF_SIZE];
        let mut out = [0; MAX_DATAGRAM_SIZE];
        let (write, _send_info) = self.send(&mut out).expect("initial send failed");
        let mut e: Result<(), HttpClientError> = Ok(());
        stream
            .write_all(&out[..write])
            .await
            .map_err(|e| HttpClientError::from_io_error(crate::ErrorKind::Connect, e))?;
        loop {
            self.conn_recv(stream, &mut buf).await?;

            if self.is_closed() {
                break;
            }
            if self.is_established() {
                let Some(pins_info) =
                    tls_config.pinning_host_match(stream.conn_data().detail().addr())
                else {
                    break;
                };

                // cert pins verify
                let verify_result = if pins_info.is_root() {
                    verify_server_root_cert(ssl.get_raw_ptr(), pins_info.get_digest())
                } else {
                    verify_server_cert(ssl.get_raw_ptr(), pins_info.get_digest())
                };
                if verify_result.is_ok() {
                    return Ok(());
                }

                e = Err(HttpClientError::from_str(
                    ErrorKind::Connect,
                    "verify server cert failed",
                ));
                if let Err(quiche::Error::Done) =
                    self.close(false, 0x1, b"verify server cert failed")
                {
                    return e;
                }
            }

            loop {
                let (write, _send_info) = match self.send(&mut out) {
                    Ok(v) => v,
                    Err(quiche::Error::Done) => {
                        break;
                    }
                    Err(err) => {
                        if e.is_ok() {
                            e = Err(HttpClientError::from_error(ErrorKind::Connect, err));
                        }
                        self.close(false, 0x1, b"fail").ok();
                        break;
                    }
                };
                stream
                    .write_all(&out[..write])
                    .await
                    .map_err(|e| HttpClientError::from_io_error(crate::ErrorKind::Connect, e))?;
            }
        }
        e
    }

    async fn conn_recv<S>(&mut self, stream: &mut S, buf: &mut [u8]) -> Result<(), HttpClientError>
    where
        S: AsyncRead + AsyncWrite + ConnInfo + Unpin + Sync + Send + 'static,
    {
        let recv_info = quiche::RecvInfo {
            to: stream.conn_data().detail().local(),
            from: stream.conn_data().detail().peer(),
        };
        let mut recv_size = 0;
        let mut len = 0;
        loop {
            if len != 0 && recv_size != len {
                match self.recv(&mut buf[recv_size..len], recv_info) {
                    Ok(size) => {
                        recv_size += size;
                        if recv_size == len {
                            return Ok(());
                        } else {
                            continue;
                        }
                    }
                    Err(quiche::Error::Done) => {
                        return Ok(());
                    }
                    Err(e) => {
                        return Err(HttpClientError::from_error(ErrorKind::Connect, e));
                    }
                }
            }
            len = match self.timeout() {
                Some(dur) => {
                    if let Ok(res) = timeout(dur, stream.read(buf)).await {
                        res
                    } else {
                        self.on_timeout();
                        return Ok(());
                    }
                }
                None => stream.read(buf).await,
            }
            .map_err(|e| HttpClientError::from_io_error(crate::ErrorKind::Connect, e))?;
        }
    }

    fn std_addr_to_c(addr: &SocketAddr, c_addr: &mut sockaddr_storage) -> socklen_t {
        let sin_port = addr.port().to_be();

        match addr {
            SocketAddr::V4(addr) => unsafe {
                let sa_len = std::mem::size_of::<sockaddr_in>();
                let c_addr_in = c_addr as *mut _ as *mut sockaddr_in;
                let s_addr = u32::from_ne_bytes(addr.ip().octets());
                let sin_addr = in_addr { s_addr };
                *c_addr_in = sockaddr_in {
                    sin_family: AF_INET as sa_family_t,
                    sin_addr,
                    sin_port,
                    sin_zero: std::mem::zeroed(),
                };
                sa_len as socklen_t
            },
            SocketAddr::V6(addr) => unsafe {
                let sa_len = std::mem::size_of::<sockaddr_in6>();
                let c_addr_in6 = c_addr as *mut _ as *mut sockaddr_in6;
                let sin6_addr = in6_addr {
                    s6_addr: addr.ip().octets(),
                };
                *c_addr_in6 = sockaddr_in6 {
                    sin6_family: AF_INET6 as sa_family_t,
                    sin6_addr,
                    sin6_port: sin_port,
                    sin6_flowinfo: addr.flowinfo(),
                    sin6_scope_id: addr.scope_id(),
                };
                sa_len as socklen_t
            },
        }
    }
}

impl Deref for QuicConn {
    type Target = quiche::Connection;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for QuicConn {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

extern "C" {
    pub(crate) fn quiche_conn_new_with_tls(
        scid: *const u8,
        scid_len: size_t,
        odcid: *const u8,
        odcid_len: size_t,
        local: *const sockaddr,
        local_len: socklen_t,
        peer: *const sockaddr,
        peer_len: socklen_t,
        config: *const c_void,
        ssl: *mut c_void,
        is_server: bool,
    ) -> *mut c_void;
}
