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

//! HTTP dns resolver module.
//!
//! This module defines the dns resolver trait accepted by http clients and
//! provides a default dns resolver implementation.
//!
//! - [`Resolver`]: The dns resolver trait, which users can implement to provide
//!   a custom dns resolver.
//!
//! - [`DefaultDnsResolver`]: Default dns resolver.

mod default;
#[cfg(feature = "__c_openssl")]
mod doh;
mod happy_eyeballs;
mod resolver;

pub use default::DefaultDnsResolver;
#[cfg(feature = "__c_openssl")]
pub use doh::DohResolver;
pub(crate) use happy_eyeballs::{EyeBallConfig, HappyEyeballs};
pub use resolver::{Addrs, Resolver, SocketFuture, StdError};
