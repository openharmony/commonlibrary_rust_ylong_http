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

use libc::{c_char, c_int, c_long, c_void};

pub(crate) enum BIO {}

// for `BIO`
extern "C" {
    /// Creates a memory BIO using len bytes of data at buf, if len is -1 then
    /// the buf is assumed to be nul terminated and its length is determined
    /// by strlen.
    pub(crate) fn BIO_new_mem_buf(buf: *const c_void, len: c_int) -> *mut BIO;

    pub(crate) fn BIO_set_data(a: *mut BIO, data: *mut c_void);

    pub(crate) fn BIO_get_data(a: *mut BIO) -> *mut c_void;

    pub(crate) fn BIO_free_all(bio: *mut BIO);

    pub(crate) fn BIO_new(b_type: *const BIO_METHOD) -> *mut BIO;

    pub(crate) fn BIO_set_init(a: *mut BIO, init: c_int);

    pub(crate) fn BIO_set_flags(b: *mut BIO, flags: c_int);

    pub(crate) fn BIO_clear_flags(b: *mut BIO, flags: c_int);
}

pub(crate) enum BIO_METHOD {}

// for `BIO_METHOD`
extern "C" {
    pub(crate) fn BIO_meth_new(type_: c_int, name: *const c_char) -> *mut BIO_METHOD;

    pub(crate) fn BIO_meth_free(biom: *mut BIO_METHOD);

    pub(crate) fn BIO_meth_set_write(
        biom: *mut BIO_METHOD,
        write: unsafe extern "C" fn(*mut BIO, *const c_char, c_int) -> c_int,
    ) -> c_int;

    pub(crate) fn BIO_meth_set_read(
        biom: *mut BIO_METHOD,
        read: unsafe extern "C" fn(*mut BIO, *mut c_char, c_int) -> c_int,
    ) -> c_int;

    pub(crate) fn BIO_meth_set_puts(
        biom: *mut BIO_METHOD,
        read: unsafe extern "C" fn(*mut BIO, *const c_char) -> c_int,
    ) -> c_int;

    pub(crate) fn BIO_meth_set_ctrl(
        biom: *mut BIO_METHOD,
        read: unsafe extern "C" fn(*mut BIO, c_int, c_long, *mut c_void) -> c_long,
    ) -> c_int;

    pub(crate) fn BIO_meth_set_create(
        biom: *mut BIO_METHOD,
        create: unsafe extern "C" fn(*mut BIO) -> c_int,
    ) -> c_int;

    pub(crate) fn BIO_meth_set_destroy(
        biom: *mut BIO_METHOD,
        destroy: unsafe extern "C" fn(*mut BIO) -> c_int,
    ) -> c_int;
}
