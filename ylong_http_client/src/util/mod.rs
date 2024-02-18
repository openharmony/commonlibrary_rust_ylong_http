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

//! Http client Util module.
//!
//! A tool module that supports various functions of the http client.
//!
//! -[`ClientConfig`] is used to configure a client with options and flags.
//! -[`HttpConfig`] is used to configure `HTTP` related logic.
//! -[`HttpVersion`] is used to provide Http Version.

#![allow(dead_code)]
#![allow(unused_imports)]

pub(crate) mod config;
#[cfg(feature = "__tls")]
pub use config::{AlpnProtocol, AlpnProtocolList, CertVerifier, ServerCerts};
pub(crate) use config::{ClientConfig, ConnectorConfig, HttpConfig, HttpVersion};
pub use config::{Proxy, ProxyBuilder, Redirect, Retry, SpeedLimit, Timeout};

#[cfg(feature = "__c_openssl")]
pub(crate) mod c_openssl;
#[cfg(feature = "__c_openssl")]
pub use c_openssl::{Cert, Certificate, TlsConfig, TlsConfigBuilder, TlsFileType, TlsVersion};
#[cfg(feature = "http2")]
pub use config::H2Config;

#[cfg(any(feature = "http1_1", feature = "http2"))]
pub(crate) mod dispatcher;

pub(crate) mod normalizer;
pub(crate) mod pool;

pub(crate) mod base64;
pub(crate) mod proxy;
pub(crate) mod redirect;
