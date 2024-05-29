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

//! Ylong http client utility module.
//!
//! A tool module that supports various functions of the http client.
//!
//! -[`ClientConfig`] is used to configure a client with options and flags.
//! -[`HttpConfig`] is used to configure `HTTP` related logic.
//! -[`HttpVersion`] is used to provide Http Version.

pub(crate) mod base64;
pub(crate) mod config;
pub(crate) mod normalizer;
pub(crate) mod pool;
pub(crate) mod proxy;
pub(crate) mod redirect;

#[cfg(feature = "async")]
pub(crate) mod request;

#[cfg(feature = "__c_openssl")]
pub(crate) mod c_openssl;

#[cfg(any(feature = "http1_1", feature = "http2"))]
pub(crate) mod dispatcher;

#[cfg(feature = "http2")]
pub(crate) mod h2;
#[cfg(all(test, feature = "ylong_base"))]
pub(crate) mod test_utils;

#[cfg(feature = "__c_openssl")]
pub use c_openssl::{
    Cert, Certificate, PubKeyPins, PubKeyPinsBuilder, TlsConfig, TlsConfigBuilder, TlsFileType,
    TlsVersion,
};
#[cfg(feature = "__c_openssl")]
pub(crate) use config::{AlpnProtocol, AlpnProtocolList};
#[cfg(feature = "__tls")]
pub use config::{CertVerifier, ServerCerts};
pub use config::{Proxy, ProxyBuilder, Redirect, Retry, SpeedLimit, Timeout};
#[cfg(all(feature = "async", feature = "ylong_base", feature = "http2"))]
pub(crate) use h2::{split, Reader, Writer};
