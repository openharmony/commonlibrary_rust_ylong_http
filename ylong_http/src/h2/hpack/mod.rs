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

//! [HPACK] implementation of the [HTTP/2 protocol].
//!
//! [HPACK]: https://httpwg.org/specs/rfc7541.html
//! [HTTP/2 protocol]: https://httpwg.org/specs/rfc9113.html
//!
//! # Introduction
//! In [HTTP/1.1], header fields are not compressed. As web pages have grown
//! to require dozens to hundreds of requests, the redundant header fields in
//! these requests unnecessarily consume bandwidth, measurably increasing
//! latency.
//!
//! [SPDY] initially addressed this redundancy by compressing header fields
//! using the [DEFLATE] format, which proved very effective at efficiently
//! representing the redundant header fields. However, that approach exposed a
//! security risk as demonstrated by the
//! [CRIME (Compression Ratio Info-leak Made Easy)] attack.
//!
//! This specification defines HPACK, a new compressor that eliminates redundant
//! header fields, limits vulnerability to known security attacks, and has a
//! bounded memory requirement for use in constrained environments. Potential
//! security concerns for HPACK are described in Section 7.
//!
//! The HPACK format is intentionally simple and inflexible. Both
//! characteristics reduce the risk of interoperability or security issues due
//! to implementation error. No extensibility mechanisms are defined; changes
//! to the format are only possible by defining a complete replacement.
//!
//! [HTTP/1.1]: https://www.rfc-editor.org/rfc/rfc9112.html
//! [SPDY]: https://datatracker.ietf.org/doc/html/draft-mbelshe-httpbis-spdy-00
//! [DEFLATE]: https://www.rfc-editor.org/rfc/rfc1951.html
//! [CRIME (Compression Ratio Info-leak Made Easy)]: https://en.wikipedia.org/w/index.php?title=CRIME&oldid=660948120

mod decoder;
mod encoder;
mod integer;
mod representation;
pub(crate) mod table;

pub(crate) use decoder::HpackDecoder;
pub(crate) use encoder::HpackEncoder;
