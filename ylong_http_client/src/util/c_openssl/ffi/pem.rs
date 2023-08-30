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

use libc::{c_char, c_int, c_void};

use super::bio::BIO;
use super::x509::C_X509;

// callback func
pub(crate) type PemPasswordCb = Option<
    unsafe extern "C" fn(
        buf: *mut c_char,
        size: c_int,
        rwflag: c_int,
        user_data: *mut c_void,
    ) -> c_int,
>;

extern "C" {
    /// The PEM functions read structures in PEM format.
    pub(crate) fn PEM_read_bio_X509(
        bio: *mut BIO,
        out: *mut *mut C_X509,
        callback: PemPasswordCb,
        user_data: *mut c_void,
    ) -> *mut C_X509;
}
