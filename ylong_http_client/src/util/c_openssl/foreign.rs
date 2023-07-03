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

use core::cell::UnsafeCell;

pub struct ForeignRefWrapper(UnsafeCell<()>);

pub trait Foreign: Sized {
    /// The raw C struct.
    type CStruct;
    /// A reference to the rust type.
    type Ref: ForeignRef<CStruct = Self::CStruct>;

    /// The raw C struct pointer to rust type.
    fn from_ptr(ptr: *mut Self::CStruct) -> Self;
    /// Returns a raw pointer to the C struct.
    fn as_ptr(&self) -> *mut Self::CStruct;
}

pub trait ForeignRef: Sized {
    /// The raw C struct.
    type CStruct;

    /// # Safety
    /// Dereference of raw pointer.
    #[inline]
    unsafe fn from_ptr<'a>(ptr: *mut Self::CStruct) -> &'a Self {
        &*(ptr as *mut _)
    }

    /// # Safety
    /// Dereference of raw pointer.
    #[inline]
    unsafe fn from_ptr_mut<'a>(ptr: *mut Self::CStruct) -> &'a mut Self {
        &mut *(ptr as *mut _)
    }

    /// Returns a raw pointer to the C struct.
    #[inline]
    fn as_ptr(&self) -> *mut Self::CStruct {
        self as *const _ as *mut _
    }
}

macro_rules! foreign_type {
    (
        type CStruct = $ctype:ty;
        fn drop = $drop:expr;

        $(#[$own_attr:meta])*
        pub(crate) struct $owned:ident;

        $(#[$borrow_attr:meta])*
        pub(crate) struct $borrowed:ident;
    ) => {
        // Wraps * mut C struct.
        $(#[$own_attr])*
        pub(crate) struct $owned(*mut $ctype);

        impl crate::util::c_openssl::foreign::Foreign for $owned {
            type CStruct = $ctype;
            type Ref = $borrowed;

            #[inline]
            fn from_ptr(ptr: *mut $ctype) -> $owned {
                $owned(ptr)
            }

            #[inline]
            fn as_ptr(&self) -> *mut $ctype {
                self.0
            }
        }

        impl Drop for $owned {
            #[inline]
            fn drop(&mut self) {
                unsafe { $drop(self.0) }
            }
        }

        // * owned -> * Deref::deref(&owned) -> * &borrowed -> borrowed
        impl core::ops::Deref for $owned {
            type Target = $borrowed;

            #[inline]
            fn deref(&self) -> &$borrowed {
                unsafe{ crate::util::c_openssl::foreign::ForeignRef::from_ptr(self.0) }
            }
        }

        // * owned -> * DerefMut::deref_mut(&mut owned) -> * &mut borrowed -> mut borrowed
        impl core::ops::DerefMut for $owned {
            #[inline]
            fn deref_mut(&mut self) -> &mut $borrowed {
                unsafe{ crate::util::c_openssl::foreign::ForeignRef::from_ptr_mut(self.0) }
            }
        }

        // owned.borrow -> & borrowed
        impl std::borrow::Borrow<$borrowed> for $owned {
            #[inline]
            fn borrow(&self) -> &$borrowed {
                &**self
            }
        }

        // owned.as_ref -> & borrowed
        impl AsRef<$borrowed> for $owned {
            #[inline]
            fn as_ref(&self) -> &$borrowed {
                &**self
            }
        }

        // A type implementing `ForeignRef` should simply be a newtype wrapper around `ForeignRefWrapper`.
        $(#[$borrow_attr:meta])*
        pub(crate) struct $borrowed(crate::util::c_openssl::foreign::ForeignRefWrapper);

        impl crate::util::c_openssl::foreign::ForeignRef for $borrowed {
            type CStruct = $ctype;
        }

        // Unsate Send and Sync mark.
        unsafe impl Send for $owned {}
        unsafe impl Send for $borrowed {}
        unsafe impl Sync for $owned {}
        unsafe impl Sync for $borrowed {}
    };
}
