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

#[cfg(feature = "__c_openssl")]
mod c_ssl_stream;
mod mix;
mod wrapper;

#[cfg(feature = "__c_openssl")]
pub use c_ssl_stream::AsyncSslStream;
pub use mix::MixStream;
pub(crate) use wrapper::{check_io_to_poll, Wrapper};
