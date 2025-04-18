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

mod client;
mod connector;
mod http;
mod settings;

pub(crate) use client::ClientConfig;
pub(crate) use connector::ConnectorConfig;
#[cfg(feature = "http2")]
pub(crate) use http::http2::H2Config;
#[cfg(feature = "http3")]
pub(crate) use http::http3::H3Config;
pub(crate) use http::{HttpConfig, HttpVersion};
pub use settings::{Proxy, ProxyBuilder, Redirect, Retry, SpeedLimit, Timeout};
#[cfg(feature = "__tls")]
pub(crate) mod tls;
#[cfg(feature = "__tls")]
pub(crate) use tls::{AlpnProtocol, AlpnProtocolList};
#[cfg(feature = "__tls")]
pub use tls::{CertVerifier, ServerCerts};
#[cfg(feature = "tls_rust_ssl")]
pub use tls::{Certificate, PrivateKey, TlsConfig, TlsConfigBuilder, TlsFileType, TlsVersion};
#[cfg(all(target_os = "linux", feature = "ylong_base", feature = "__tls"))]
mod fchown;
#[cfg(all(target_os = "linux", feature = "ylong_base", feature = "__tls"))]
pub(crate) use fchown::FchownConfig;
