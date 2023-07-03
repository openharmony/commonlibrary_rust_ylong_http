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

pub(crate) enum OPENSSL_STACK {}

extern "C" {
    pub(crate) fn OPENSSL_sk_free(st: *mut OPENSSL_STACK);

    pub(crate) fn OPENSSL_sk_pop(st: *mut OPENSSL_STACK) -> *mut c_void;

    pub(crate) fn OPENSSL_sk_value(stack: *const OPENSSL_STACK, idx: c_int) -> *mut c_void;

    pub(crate) fn OPENSSL_sk_num(stack: *const OPENSSL_STACK) -> c_int;
}
