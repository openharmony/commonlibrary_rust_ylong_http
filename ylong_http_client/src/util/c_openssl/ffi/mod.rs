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

#![allow(clippy::upper_case_acronyms)]
#![allow(non_camel_case_types)]

pub(crate) mod bio;
pub(crate) mod callback;
pub(crate) mod err;
pub(crate) mod pem;
pub(crate) mod ssl;
pub(crate) mod stack;
pub(crate) mod x509;

use libc::c_int;
pub(crate) enum OPENSSL_INIT_SETTINGS {}

extern "C" {
    /// Calls this function will explicitly initialise BOTH libcrypto and
    /// libssl.
    pub(crate) fn OPENSSL_init_ssl(opts: u64, settings: *const OPENSSL_INIT_SETTINGS) -> c_int;
}
