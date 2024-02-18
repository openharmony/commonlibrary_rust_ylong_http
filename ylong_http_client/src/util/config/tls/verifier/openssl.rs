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

use crate::util::c_openssl::x509::X509StoreContextRef;

/// ServerCerts is provided to fetch info from X509
pub struct ServerCerts<'a> {
    inner: &'a X509StoreContextRef,
}

impl<'a> ServerCerts<'a> {
    pub(crate) fn new(inner: &'a X509StoreContextRef) -> Self {
        Self { inner }
    }
}

impl AsRef<X509StoreContextRef> for ServerCerts<'_> {
    fn as_ref(&self) -> &X509StoreContextRef {
        self.inner
    }
}
