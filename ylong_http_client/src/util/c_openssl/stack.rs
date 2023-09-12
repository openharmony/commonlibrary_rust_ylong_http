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

use core::borrow::Borrow;
use core::marker::PhantomData;
use core::mem::forget;
use core::ops::{Deref, DerefMut, Range};

use libc::c_int;

use super::ffi::stack::{
    OPENSSL_sk_free, OPENSSL_sk_num, OPENSSL_sk_pop, OPENSSL_sk_value, OPENSSL_STACK,
};
use crate::c_openssl::foreign::{Foreign, ForeignRef, ForeignRefWrapper};

pub(crate) trait Stackof: Foreign {
    type StackType;
}

pub(crate) struct Stack<T: Stackof>(*mut T::StackType);

pub(crate) struct StackRef<T: Stackof>(ForeignRefWrapper, PhantomData<T>);

unsafe impl<T: Stackof + Send> Send for Stack<T> {}

unsafe impl<T: Stackof + Sync> Sync for Stack<T> {}

unsafe impl<T: Stackof + Send> Send for StackRef<T> {}

unsafe impl<T: Stackof + Sync> Sync for StackRef<T> {}

impl<T: Stackof> Deref for Stack<T> {
    type Target = StackRef<T>;

    fn deref(&self) -> &StackRef<T> {
        unsafe { StackRef::from_ptr(self.0) }
    }
}

impl<T: Stackof> DerefMut for Stack<T> {
    fn deref_mut(&mut self) -> &mut StackRef<T> {
        unsafe { StackRef::from_ptr_mut(self.0) }
    }
}

impl<T: Stackof> AsRef<StackRef<T>> for Stack<T> {
    fn as_ref(&self) -> &StackRef<T> {
        self
    }
}

impl<T: Stackof> Borrow<StackRef<T>> for Stack<T> {
    fn borrow(&self) -> &StackRef<T> {
        self
    }
}

impl<T: Stackof> Drop for Stack<T> {
    fn drop(&mut self) {
        unsafe {
            while self.pop().is_some() {}
            OPENSSL_sk_free(self.0 as *mut _)
        }
    }
}

pub(crate) struct StackRefIter<'a, T: Stackof>
where
    T: 'a,
{
    stack: &'a StackRef<T>,
    index: Range<c_int>,
}

impl<'a, T: Stackof> Iterator for StackRefIter<'a, T> {
    type Item = &'a T::Ref;

    fn next(&mut self) -> Option<&'a T::Ref> {
        unsafe {
            self.index
                .next()
                .map(|i| T::Ref::from_ptr(OPENSSL_sk_value(self.stack.as_stack(), i) as *mut _))
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.index.size_hint()
    }
}

impl<T: Stackof> StackRef<T> {
    #[allow(clippy::len_without_is_empty)]
    pub(crate) fn len(&self) -> usize {
        unsafe { OPENSSL_sk_num(self.as_stack()) as usize }
    }

    pub(crate) fn pop(&mut self) -> Option<T> {
        unsafe {
            let ptr = OPENSSL_sk_pop(self.as_stack());
            match ptr.is_null() {
                true => None,
                false => Some(T::from_ptr(ptr as *mut _)),
            }
        }
    }
}

impl<T: Stackof> ForeignRef for StackRef<T> {
    type CStruct = T::StackType;
}

impl<T: Stackof> StackRef<T> {
    fn as_stack(&self) -> *mut OPENSSL_STACK {
        self.as_ptr() as *mut _
    }
}

impl<T: Stackof> Foreign for Stack<T> {
    type CStruct = T::StackType;
    type Ref = StackRef<T>;

    fn from_ptr(ptr: *mut Self::CStruct) -> Self {
        Stack(ptr)
    }

    fn as_ptr(&self) -> *mut Self::CStruct {
        self.0
    }
}

pub(crate) struct IntoStackIter<T: Stackof> {
    stack: *mut T::StackType,
    index: Range<c_int>,
}

impl<T: Stackof> Iterator for IntoStackIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            self.index
                .next()
                .map(|i| T::from_ptr(OPENSSL_sk_value(self.stack as *mut _, i) as *mut _))
        }
    }
}

impl<T: Stackof> Drop for IntoStackIter<T> {
    fn drop(&mut self) {
        unsafe {
            while self.next().is_some() {}
            OPENSSL_sk_free(self.stack as *mut _);
        }
    }
}

impl<T: Stackof> IntoIterator for Stack<T> {
    type Item = T;
    type IntoIter = IntoStackIter<T>;

    fn into_iter(self) -> IntoStackIter<T> {
        let it = IntoStackIter {
            stack: self.0,
            index: 0..self.len() as c_int,
        };
        forget(self);
        it
    }
}
