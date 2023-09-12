// Copyright (c) 2024 Huawei Device Co., Ltd.
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

use std::ffi::{c_int, c_void};

use crate::util::c_openssl::ffi::x509::X509_STORE_CTX;
use crate::util::c_openssl::foreign::ForeignRef;
use crate::util::c_openssl::x509::X509StoreContextRef;
use crate::util::config::tls::DefaultCertVerifier;
use crate::{CertVerifier, ServerCerts};

pub(crate) extern "C" fn cert_verify(ctx: *mut X509_STORE_CTX, arg: *mut c_void) -> c_int {
    unsafe {
        let verifier = &*(arg as *const DefaultCertVerifier);
        let ctx = X509StoreContextRef::from_ptr(ctx);
        verifier.verify(&ServerCerts::new(ctx)) as c_int
    }
}
