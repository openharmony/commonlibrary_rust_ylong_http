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

//! [Http/2 Protocol] Implementation.
//!
//! [Http/2 Protocol]: https://httpwg.org/specs/rfc9113.html
//! # Introduction
//! The performance of applications using the Hypertext Transfer Protocol
//! ([HTTP]) is linked to how each version of HTTP uses the underlying
//! transport, and the conditions under which the transport operates.
//!
//! Making multiple concurrent requests can reduce latency and improve
//! application performance. HTTP/1.0 allowed only one request to be outstanding
//! at a time on a given [TCP] connection. [HTTP/1.1] added request pipelining,
//! but this only partially addressed request concurrency and still suffers from
//! application-layer head-of-line blocking. Therefore, HTTP/1.0 and HTTP/1.1
//! clients use multiple connections to a server to make concurrent requests.
//!
//! Furthermore, HTTP fields are often repetitive and verbose, causing
//! unnecessary network traffic as well as causing the initial TCP congestion
//! window to quickly fill. This can result in excessive latency when multiple
//! requests are made on a new TCP connection.
//!
//! HTTP/2 addresses these issues by defining an optimized mapping of HTTP's
//! semantics to an underlying connection. Specifically, it allows interleaving
//! of messages on the same connection and uses an efficient coding for HTTP
//! fields. It also allows prioritization of requests, letting more important
//! requests complete more quickly, further improving performance.
//!
//! The resulting protocol is more friendly to the network because fewer TCP
//! connections can be used in comparison to HTTP/1.x. This means less
//! competition with other flows and longer-lived connections, which in turn
//! lead to better utilization of available network capacity. Note, however,
//! that TCP head-of-line blocking is not addressed by this protocol.
//!
//! Finally, HTTP/2 also enables more efficient processing of messages through
//! use of binary message framing.
//!
//! [HTTP]: https://www.rfc-editor.org/rfc/rfc9110.html
//! [HTTP/1.1]: https://www.rfc-editor.org/rfc/rfc9112.html
//! [TCP]: https://www.rfc-editor.org/rfc/rfc793.html

mod decoder;
mod encoder;
mod error;
mod frame;
mod hpack;
mod parts;
mod pseudo;

pub use decoder::{FrameDecoder, FrameKind, Frames, FramesIntoIter};
pub use encoder::FrameEncoder;
pub use error::{ErrorCode, H2Error};
pub use frame::{
    Data, Frame, FrameFlags, Goaway, Headers, Payload, Ping, RstStream, Setting, Settings,
    SettingsBuilder, WindowUpdate,
};
pub(crate) use hpack::{HpackDecoder, HpackEncoder};
pub use parts::Parts;
pub use pseudo::PseudoHeaders;
