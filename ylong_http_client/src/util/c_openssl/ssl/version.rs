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

const TLS1_VERSION: c_int = 0x301;
const TLS1_1_VERSION: c_int = 0x302;
const TLS1_2_VERSION: c_int = 0x303;
const TLS1_3_VERSION: c_int = 0x304;

pub(crate) struct SslVersion(pub c_int);

impl SslVersion {
    /// TLSv1.0
    pub(crate) const TLS_1_0: SslVersion = SslVersion(TLS1_VERSION);

    /// TLSv1.1
    pub(crate) const TLS_1_1: SslVersion = SslVersion(TLS1_1_VERSION);

    /// TLSv1.2
    pub(crate) const TLS_1_2: SslVersion = SslVersion(TLS1_2_VERSION);

    /// TLSv1.3
    pub(crate) const TLS_1_3: SslVersion = SslVersion(TLS1_3_VERSION);
}
