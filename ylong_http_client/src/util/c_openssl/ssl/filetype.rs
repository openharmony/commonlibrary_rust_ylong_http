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

use libc::c_int;

const X509_FILETYPE_PEM: c_int = 1;
const X509_FILETYPE_ASN1: c_int = 2;

pub(crate) struct SslFiletype(c_int);
impl SslFiletype {
    pub(crate) const PEM: SslFiletype = SslFiletype(X509_FILETYPE_PEM);
    pub(crate) const ASN1: SslFiletype = SslFiletype(X509_FILETYPE_ASN1);

    pub(crate) fn as_raw(&self) -> c_int {
        self.0
    }
}
