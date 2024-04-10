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

use core::ffi::CStr;
use core::{ptr, str};
use std::borrow::Cow;
use std::error::Error;
#[cfg(feature = "c_openssl_3_0")]
use std::ffi::CString;
use std::fmt;

#[cfg(feature = "c_openssl_1_1")]
use libc::c_char;
use libc::{c_int, c_ulong};

use super::ssl_init;
#[cfg(feature = "c_openssl_3_0")]
use crate::util::c_openssl::ffi::err::ERR_get_error_all;
#[cfg(feature = "c_openssl_1_1")]
use crate::util::c_openssl::ffi::err::{ERR_func_error_string, ERR_get_error_line_data};
use crate::util::c_openssl::ffi::err::{ERR_lib_error_string, ERR_reason_error_string};

const ERR_TXT_MALLOCED: c_int = 0x01;
const ERR_TXT_STRING: c_int = 0x02;

/// An error reported from OpenSSL.
#[derive(Debug)]
pub(crate) struct StackError {
    code: c_ulong,
    #[cfg(feature = "c_openssl_1_1")]
    file: *const c_char,
    #[cfg(feature = "c_openssl_3_0")]
    file: CString,
    line: c_int,
    #[cfg(feature = "c_openssl_3_0")]
    func: Option<CString>,
    data: Option<Cow<'static, str>>,
}

impl Clone for StackError {
    fn clone(&self) -> Self {
        Self {
            code: self.code,
            #[cfg(feature = "c_openssl_1_1")]
            file: self.file,
            #[cfg(feature = "c_openssl_3_0")]
            file: self.file.clone(),
            line: self.line,
            #[cfg(feature = "c_openssl_3_0")]
            func: self.func.clone(),
            data: self.data.clone(),
        }
    }
}

impl StackError {
    /// Returns the first error on the OpenSSL error stack.
    fn get() -> Option<StackError> {
        unsafe {
            ssl_init();

            let mut file = ptr::null();
            let mut line = 0;
            #[cfg(feature = "c_openssl_3_0")]
            let mut func = ptr::null();
            let mut data = ptr::null();
            let mut flags = 0;

            #[cfg(feature = "c_openssl_1_1")]
            match ERR_get_error_line_data(&mut file, &mut line, &mut data, &mut flags) {
                0 => None,
                code => {
                    let data = if flags & ERR_TXT_STRING != 0 {
                        let bytes = CStr::from_ptr(data as *const _).to_bytes();
                        let data = str::from_utf8(bytes).unwrap_or("");
                        let data = if flags & ERR_TXT_MALLOCED != 0 {
                            Cow::Owned(data.to_string())
                        } else {
                            Cow::Borrowed(data)
                        };
                        Some(data)
                    } else {
                        None
                    };
                    Some(StackError {
                        code,
                        file,
                        line,
                        data,
                    })
                }
            }

            #[cfg(feature = "c_openssl_3_0")]
            match ERR_get_error_all(&mut file, &mut line, &mut func, &mut data, &mut flags) {
                0 => None,
                code => {
                    let data = if flags & ERR_TXT_STRING != 0 {
                        let bytes = CStr::from_ptr(data as *const _).to_bytes();
                        let data = str::from_utf8(bytes).unwrap();
                        let data = if flags & ERR_TXT_MALLOCED != 0 {
                            Cow::Owned(data.to_string())
                        } else {
                            Cow::Borrowed(data)
                        };
                        Some(data)
                    } else {
                        None
                    };

                    let file = CStr::from_ptr(file).to_owned();
                    let func = if func.is_null() {
                        None
                    } else {
                        Some(CStr::from_ptr(func).to_owned())
                    };
                    Some(StackError {
                        code,
                        file,
                        line,
                        func,
                        data,
                    })
                }
            }
        }
    }

    /// Returns the raw OpenSSL error code for this error.
    fn code(&self) -> c_ulong {
        self.code
    }
}

impl fmt::Display for StackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error:{:08X}", self.code())?;
        unsafe {
            let lib_error = ERR_lib_error_string(self.code);
            if !lib_error.is_null() {
                let bytes = CStr::from_ptr(lib_error as *const _).to_bytes();
                write!(f, "lib: ({}), ", str::from_utf8(bytes).unwrap_or_default())?;
            } else {
                write!(f, "lib: ({}), ", error_get_lib(self.code))?;
            }
        }

        #[cfg(feature = "c_openssl_1_1")]
        {
            let func_error = unsafe { ERR_func_error_string(self.code) };
            if !func_error.is_null() {
                let bytes = unsafe { core::ffi::CStr::from_ptr(func_error as *const _).to_bytes() };
                write!(f, "func: ({}), ", str::from_utf8(bytes).unwrap_or_default())?;
            } else {
                write!(f, "func: ({}), ", error_get_func(self.code))?;
            }
        }

        #[cfg(feature = "c_openssl_3_0")]
        {
            let func_error = self.func.as_ref().map(|s| s.to_str().unwrap_or_default());
            match func_error {
                Some(s) => write!(f, ":{s}")?,
                None => write!(f, ":func({})", error_get_func(self.code))?,
            }
        }

        unsafe {
            let reason_error = ERR_reason_error_string(self.code);
            if !reason_error.is_null() {
                let bytes = CStr::from_ptr(reason_error as *const _).to_bytes();
                write!(
                    f,
                    "reason: ({}), ",
                    str::from_utf8(bytes).unwrap_or_default()
                )?;
            } else {
                write!(f, "reason: ({}), ", error_get_reason(self.code))?;
            }
        }
        write!(
            f,
            ":{:?}:{}:{}",
            self.file,
            self.line,
            self.data.as_deref().unwrap_or("")
        )
    }
}

unsafe impl Sync for StackError {}
unsafe impl Send for StackError {}

#[derive(Clone, Debug)]
pub struct ErrorStack(Vec<StackError>);

impl fmt::Display for ErrorStack {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        if self.0.is_empty() {
            return fmt.write_str("Error happened in OpenSSL");
        }

        for err in &self.0 {
            write!(fmt, "{err} ")?;
        }
        Ok(())
    }
}

impl Error for ErrorStack {}

impl ErrorStack {
    pub(crate) fn get() -> ErrorStack {
        let mut vec = vec![];
        while let Some(err) = StackError::get() {
            vec.push(err);
        }
        ErrorStack(vec)
    }

    pub(crate) fn errors(&self) -> &[StackError] {
        &self.0
    }
}

#[cfg(feature = "c_openssl_3_0")]
const ERR_SYSTEM_FLAG: c_ulong = c_int::max_value() as c_ulong + 1;
#[cfg(feature = "c_openssl_3_0")]
const fn error_system_error(code: c_ulong) -> bool {
    code & ERR_SYSTEM_FLAG != 0
}

pub(crate) const fn error_get_lib(code: c_ulong) -> c_int {
    #[cfg(feature = "c_openssl_1_1")]
    return ((code >> 24) & 0x0FF) as c_int;

    #[cfg(feature = "c_openssl_3_0")]
    return ((2 as c_ulong * (error_system_error(code) as c_ulong))
        | (((code >> 23) & 0xFF) * (!error_system_error(code) as c_ulong))) as c_int;
}

#[allow(unused_variables)]
const fn error_get_func(code: c_ulong) -> c_int {
    #[cfg(feature = "c_openssl_1_1")]
    return ((code >> 12) & 0xFFF) as c_int;

    #[cfg(feature = "c_openssl_3_0")]
    0
}

pub(crate) const fn error_get_reason(code: c_ulong) -> c_int {
    #[cfg(feature = "c_openssl_1_1")]
    return (code & 0xFFF) as c_int;

    #[cfg(feature = "c_openssl_3_0")]
    return ((2 as c_ulong * (error_system_error(code) as c_ulong))
        | ((code & 0x7FFFFF) * (!error_system_error(code) as c_ulong))) as c_int;
}

pub(crate) struct VerifyError {
    kind: VerifyKind,
    cause: Reason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VerifyKind {
    PubKeyPinning,
}

pub(crate) enum Reason {
    Msg(&'static str),
}

impl VerifyError {
    pub(crate) fn from_msg(kind: VerifyKind, msg: &'static str) -> Self {
        Self {
            kind,
            cause: Reason::Msg(msg),
        }
    }
}

impl VerifyKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PubKeyPinning => "Public Key Pinning Error",
        }
    }
}

impl fmt::Debug for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut builder = f.debug_struct("VerifyError");
        builder.field("ErrorKind", &self.kind);
        builder.field("Cause", &self.cause);
        builder.finish()
    }
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.kind.as_str())?;
        write!(f, ": {}", self.cause)?;
        Ok(())
    }
}

impl fmt::Debug for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Msg(msg) => write!(f, "{}", msg),
        }
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Msg(msg) => write!(f, "{}", msg),
        }
    }
}

impl Error for VerifyError {}

#[cfg(test)]
mod ut_c_openssl_error {
    use crate::util::c_openssl::error::{VerifyError, VerifyKind};

    /// UT test cases for `VerifyKind::as_str`.
    ///
    /// # Brief
    /// 1. Transfer ErrorKind to str a by calling `VerifyKind::as_str`.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_verify_err_as_str() {
        assert_eq!(
            VerifyKind::PubKeyPinning.as_str(),
            "Public Key Pinning Error"
        );
    }

    /// UT test cases for `VerifyKind::from` function.
    ///
    /// # Brief
    /// 1. Calls `VerifyKind::from`.
    /// 2. Checks if the results are correct.
    #[test]
    fn ut_verify_err_from() {
        let error = VerifyError::from_msg(VerifyKind::PubKeyPinning, "error");
        assert_eq!(
            format!("{:?}", error),
            "VerifyError { ErrorKind: PubKeyPinning, Cause: error }"
        );
        assert_eq!(format!("{error}"), "Public Key Pinning Error: error");
    }
}
