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

use libc::{c_int, c_void};

#[cfg(not(feature = "c_boringssl"))]
pub(crate) enum OPENSSL_STACK {}

#[cfg(not(feature = "c_boringssl"))]
extern "C" {
    fn OPENSSL_sk_free(st: *mut OPENSSL_STACK);

    fn OPENSSL_sk_pop(st: *mut OPENSSL_STACK) -> *mut c_void;

    fn OPENSSL_sk_value(stack: *const OPENSSL_STACK, idx: c_int) -> *mut c_void;

    fn OPENSSL_sk_num(stack: *const OPENSSL_STACK) -> c_int;
}

#[cfg(feature = "c_boringssl")]
pub(crate) type STACK = *mut c_void;
#[cfg(not(feature = "c_boringssl"))]
pub(crate) type STACK = *mut OPENSSL_STACK;

#[cfg(feature = "c_boringssl")]
extern "C" {
    fn sk_free(st: STACK);
    fn sk_pop(st: STACK) -> *mut c_void;
    fn sk_value(st: STACK, idx: c_int) -> *mut c_void;
    fn sk_num(st: STACK) -> c_int;
}

/// Frees a stack allocated by OpenSSL or BoringSSL.
///
/// # Safety
/// - `st` must be a valid pointer to a stack allocated by the same library
///   (OpenSSL or BoringSSL) used in this crate.
/// - The stack must not be accessed or freed elsewhere after this call.
pub(crate) unsafe fn unified_sk_free(st: STACK) {
    #[cfg(feature = "c_boringssl")]
    {
        sk_free(st);
    }
    #[cfg(not(feature = "c_boringssl"))]
    {
        OPENSSL_sk_free(st);
    }
}

/// Retrieves a pointer to a stack element from a stack allocated by OpenSSL or
/// BoringSSL at the specified index.
///
/// # Safety
/// - `st` must be a valid pointer to a stack allocated by the same library
///   (OpenSSL or BoringSSL) used in this crate.
/// - `idx` must be a valid index within the bounds of the stack. if the index
///   is out of range, the function will return `null`.
pub(crate) unsafe fn unified_sk_value(st: STACK, idx: c_int) -> *mut c_void {
    #[cfg(feature = "c_boringssl")]
    {
        sk_value(st, idx)
    }
    #[cfg(not(feature = "c_boringssl"))]
    {
        OPENSSL_sk_value(st, idx)
    }
}

/// Returns the number of elements in a stack allocated by OpenSSL or BoringSSL.
///
/// # Safety
/// - `st` must be a valid pointer to a stack allocated by the same library
///   (OpenSSL or BoringSSL) used in this crate.
/// - The caller must ensure that `st` is not a null pointer. If `st` is `NULL`,
///   the function will return `-1`.
pub(crate) unsafe fn unified_sk_num(st: STACK) -> c_int {
    #[cfg(feature = "c_boringssl")]
    {
        sk_num(st)
    }
    #[cfg(not(feature = "c_boringssl"))]
    {
        OPENSSL_sk_num(st)
    }
}

/// Pops a value from the top of a stack allocated by OpenSSL or BoringSSL.
/// If the stack is empty, this function returns `null`.
///
/// # Safety
/// - `st` must be a valid pointer to a stack allocated by the same library
///   (OpenSSL or BoringSSL) used in this crate.
/// - The caller must check the return value of this function. If the stack is
///   empty, the function will return `null`. The caller must handle this case
///   appropriately.
pub(crate) unsafe fn unified_sk_pop(st: STACK) -> *mut c_void {
    #[cfg(feature = "c_boringssl")]
    {
        sk_pop(st)
    }
    #[cfg(not(feature = "c_boringssl"))]
    {
        OPENSSL_sk_pop(st)
    }
}
