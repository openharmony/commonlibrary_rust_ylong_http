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

//! HTTP2 io split module.
//!
//! The module defines the io split method, which splits the input and output of
//! io into separate objects.
//!
//! - [`Reader`]: The io output object.
//!
//! - [`Writer`]: The io input object.
//!
//! - [`split`]: io split function.

mod split;
pub(crate) use split::{split, Reader, Writer};
