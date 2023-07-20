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

use core::any::Any;
use core::marker::PhantomData;
use core::panic::AssertUnwindSafe;
use core::{ptr, slice};
use std::io::{self, Read, Write};
use std::panic::catch_unwind;

use libc::{c_char, c_int, c_long, c_void, strlen};

use super::error::ErrorStack;
use super::ffi::bio::{
    BIO_clear_flags, BIO_free_all, BIO_get_data, BIO_meth_free, BIO_meth_new, BIO_meth_set_create,
    BIO_meth_set_ctrl, BIO_meth_set_destroy, BIO_meth_set_puts, BIO_meth_set_read,
    BIO_meth_set_write, BIO_new, BIO_new_mem_buf, BIO_set_data, BIO_set_flags, BIO_set_init, BIO,
    BIO_METHOD,
};
use super::{check_ptr, ssl_init};

#[derive(Debug)]
pub struct Bio(*mut BIO);

impl Drop for Bio {
    fn drop(&mut self) {
        unsafe {
            BIO_free_all(self.0);
        }
    }
}

#[derive(Debug)]
pub struct BioSlice<'a>(*mut BIO, PhantomData<&'a [u8]>);

impl<'a> BioSlice<'a> {
    pub(crate) fn from_byte(buf: &'a [u8]) -> Result<BioSlice<'a>, ErrorStack> {
        unsafe {
            ssl_init();
            let bio = check_ptr(BIO_new_mem_buf(
                buf.as_ptr() as *const _,
                buf.len() as c_int,
            ))?;
            Ok(BioSlice(bio, PhantomData))
        }
    }

    pub(crate) fn as_ptr(&self) -> *mut BIO {
        self.0
    }
}

impl<'a> Drop for BioSlice<'a> {
    fn drop(&mut self) {
        unsafe { BIO_free_all(self.0) }
    }
}

const BIO_TYPE_NONE: c_int = 0;

const BIO_CTRL_FLUSH: c_int = 11;
const BIO_CTRL_DGRAM_QUERY: c_int = 40;

const BIO_FLAGS_READ: c_int = 0x01;
const BIO_FLAGS_WRITE: c_int = 0x02;
const BIO_FLAGS_IO_SPECIAL: c_int = 0x04;
const BIO_FLAGS_SHOULD_RETRY: c_int = 0x08;
const BIO_FLAGS_RWS: c_int = BIO_FLAGS_READ | BIO_FLAGS_WRITE | BIO_FLAGS_IO_SPECIAL;

#[derive(Debug)]
pub struct BioMethodInner(*mut BIO_METHOD);

impl BioMethodInner {
    fn new<S: Read + Write>() -> Result<BioMethodInner, ErrorStack> {
        unsafe {
            let ptr = check_ptr(BIO_meth_new(BIO_TYPE_NONE, b"rust\0".as_ptr() as *const _))?;
            let bio_method = BioMethodInner(ptr);

            BIO_meth_set_write(ptr, bwrite::<S>);
            BIO_meth_set_read(ptr, bread::<S>);
            BIO_meth_set_puts(ptr, bputs::<S>);
            BIO_meth_set_ctrl(ptr, ctrl::<S>);
            BIO_meth_set_create(ptr, create);
            BIO_meth_set_destroy(ptr, destroy::<S>);

            Ok(bio_method)
        }
    }

    fn get(&self) -> *mut BIO_METHOD {
        self.0
    }
}

unsafe impl Sync for BioMethod {}
unsafe impl Send for BioMethod {}

impl Drop for BioMethodInner {
    fn drop(&mut self) {
        unsafe { BIO_meth_free(self.0) }
    }
}

#[derive(Debug)]
pub struct BioMethod(BioMethodInner);

impl BioMethod {
    fn new<S: Read + Write>() -> Result<BioMethod, ErrorStack> {
        let method = BioMethodInner::new::<S>()?;
        Ok(BioMethod(method))
    }

    fn get(&self) -> *mut BIO_METHOD {
        self.0.get()
    }
}

pub(crate) struct StreamState<S> {
    pub(crate) stream: S,
    pub(crate) error: Option<io::Error>,
    pub(crate) panic: Option<Box<dyn Any + Send>>,
    pub(crate) dtls_mtu_size: c_long,
}
unsafe fn get_state<'a, S: 'a>(bio: *mut BIO) -> &'a mut StreamState<S> {
    &mut *(BIO_get_data(bio) as *mut _)
}

pub(crate) unsafe fn get_error<S>(bio: *mut BIO) -> Option<io::Error> {
    let state = get_state::<S>(bio);
    state.error.take()
}

pub(crate) unsafe fn get_panic<S>(bio: *mut BIO) -> Option<Box<dyn Any + Send>> {
    let state = get_state::<S>(bio);
    state.panic.take()
}

pub(crate) unsafe fn get_stream_ref<'a, S: 'a>(bio: *mut BIO) -> &'a S {
    let state: &'a StreamState<S> = &*(BIO_get_data(bio) as *const StreamState<S>);
    &state.stream
}

pub(crate) unsafe fn get_stream_mut<'a, S: 'a>(bio: *mut BIO) -> &'a mut S {
    &mut get_state(bio).stream
}

pub(crate) fn new<S: Read + Write>(stream: S) -> Result<(*mut BIO, BioMethod), ErrorStack> {
    let bio_method = BioMethod::new::<S>()?;

    let stream_state = Box::new(StreamState {
        stream,
        error: None,
        panic: None,
        dtls_mtu_size: 0,
    });

    unsafe {
        let bio = check_ptr(BIO_new(bio_method.get()))?;
        BIO_set_data(bio, Box::into_raw(stream_state) as *mut _);
        BIO_set_init(bio, 1);

        Ok((bio, bio_method))
    }
}

fn retry_error(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::NotConnected
    )
}

unsafe extern "C" fn ctrl<S: Write>(
    bio: *mut BIO,
    ctrl_cmd: c_int,
    _num: c_long,
    _ptr: *mut c_void,
) -> c_long {
    let state = get_state::<S>(bio);

    if ctrl_cmd == BIO_CTRL_FLUSH {
        match catch_unwind(AssertUnwindSafe(|| state.stream.flush())) {
            Ok(Err(err)) => {
                state.error = Some(err);
                0
            }
            Ok(Ok(())) => 1,
            Err(err) => {
                state.panic = Some(err);
                0
            }
        }
    } else if ctrl_cmd == BIO_CTRL_DGRAM_QUERY {
        state.dtls_mtu_size
    } else {
        0
    }
}

#[allow(non_snake_case)]
unsafe fn BIO_set_num(_bio: *mut BIO, _num: c_int) {}

unsafe extern "C" fn create(bio: *mut BIO) -> c_int {
    BIO_set_init(bio, 0);
    BIO_set_flags(bio, 0);
    BIO_set_num(bio, 0);
    BIO_set_data(bio, ptr::null_mut());
    1
}

unsafe extern "C" fn destroy<S>(bio: *mut BIO) -> c_int {
    if bio.is_null() {
        return 0;
    }
    let data = BIO_get_data(bio);
    drop(Box::<StreamState<S>>::from_raw(data as *mut _));
    BIO_set_init(bio, 0);
    BIO_set_data(bio, ptr::null_mut());
    1
}

unsafe extern "C" fn bwrite<S: Write>(bio: *mut BIO, buf: *const c_char, len: c_int) -> c_int {
    BIO_clear_flags(bio, BIO_FLAGS_SHOULD_RETRY | BIO_FLAGS_RWS);

    let state = get_state::<S>(bio);
    if len < 0 {
        state.error = Some(io::Error::from(io::ErrorKind::InvalidInput));
        return -1;
    }

    let buf = slice::from_raw_parts(buf as *const _, len as usize);

    match catch_unwind(AssertUnwindSafe(|| state.stream.write(buf))) {
        Ok(Err(err)) => {
            if retry_error(&err) {
                BIO_set_flags(bio, BIO_FLAGS_SHOULD_RETRY | BIO_FLAGS_WRITE)
            }
            state.error = Some(err);
            -1
        }
        Ok(Ok(len)) => len as c_int,
        Err(err) => {
            state.panic = Some(err);
            -1
        }
    }
}

unsafe extern "C" fn bread<S: Read>(bio: *mut BIO, buf: *mut c_char, len: c_int) -> c_int {
    BIO_clear_flags(bio, BIO_FLAGS_SHOULD_RETRY | BIO_FLAGS_RWS);

    let state = get_state::<S>(bio);
    let buf = slice::from_raw_parts_mut(buf as *mut _, len as usize);

    match catch_unwind(AssertUnwindSafe(|| state.stream.read(buf))) {
        Ok(Err(err)) => {
            if retry_error(&err) {
                BIO_set_flags(bio, BIO_FLAGS_SHOULD_RETRY | BIO_FLAGS_READ)
            }
            state.error = Some(err);
            -1
        }
        Ok(Ok(len)) => len as c_int,
        Err(err) => {
            state.panic = Some(err);
            -1
        }
    }
}

unsafe extern "C" fn bputs<S: Write>(bio: *mut BIO, buf: *const c_char) -> c_int {
    bwrite::<S>(bio, buf, strlen(buf) as c_int)
}
