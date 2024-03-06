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

#[cfg(test)]
#[macro_use]
#[rustfmt::skip]
pub(crate) mod mime_test_macro;

mod common;
mod decode;
mod encode;
mod mimetype;
mod simple;

pub(crate) use common::{
    DecodeHeaders, EncodeHeaders, HeaderStatus, MixFrom, PartStatus, CR, CRLF, HTAB, LF, SP,
};
pub use common::{MimeMulti, MimeMultiBuilder, MimePart, MimePartBuilder, TokenStatus, XPart};
pub use decode::MimeMultiDecoder;
pub(crate) use decode::MimePartDecoder;
pub use encode::{MimeMultiEncoder, MimePartEncoder};
pub use mimetype::MimeType;
pub use simple::{MultiPart, MultiPartBase, Part};
