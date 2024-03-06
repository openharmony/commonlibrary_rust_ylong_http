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

mod ctx;
mod error;
mod filetype;
mod method;
mod ssl_base;
mod stream;
mod version;

pub(crate) use ctx::{SslContext, SslContextBuilder, SSL_VERIFY_NONE};
pub(crate) use error::{InternalError, SslError, SslErrorCode};
pub(crate) use filetype::SslFiletype;
pub(crate) use method::SslMethod;
pub(crate) use ssl_base::{Ssl, SslRef};
pub(crate) use stream::{MidHandshakeSslStream, ShutdownResult, SslStream};
pub(crate) use version::SslVersion;
