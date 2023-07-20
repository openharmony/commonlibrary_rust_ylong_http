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

use libc::{c_char, c_int, c_ulong};

extern "C" {
    /// ERR_lib_error_string() and ERR_reason_error_string() return the library
    /// name and reason string respectively. \
    /// If there is no text string registered for the given error code,
    /// the error string will contain the numeric code.
    pub(crate) fn ERR_lib_error_string(err: c_ulong) -> *const c_char;
    pub(crate) fn ERR_reason_error_string(err: c_ulong) -> *const c_char;

    pub(crate) fn ERR_peek_last_error() -> c_ulong;

    pub(crate) fn ERR_clear_error();

    /// Returns the earliest error code from the thread's error queue and
    /// removes the entry. This function can be called repeatedly until
    /// there are no more error codes to return.
    #[cfg(feature = "c_openssl_3_0")]
    pub(crate) fn ERR_get_error_all(
        file: *mut *const c_char,
        line: *mut c_int,
        function: *mut *const c_char,
        data: *mut *const c_char,
        flags: *mut c_int,
    ) -> c_ulong;

    #[cfg(feature = "c_openssl_1_1")]
    pub(crate) fn ERR_get_error_line_data(
        file: *mut *const c_char,
        line: *mut c_int,
        data: *mut *const c_char,
        flags: *mut c_int,
    ) -> c_ulong;

    #[cfg(feature = "c_openssl_1_1")]
    pub(crate) fn ERR_func_error_string(err: c_ulong) -> *const c_char;
}
