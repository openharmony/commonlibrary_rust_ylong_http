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

// TODO: `HTTP/3` Module.

mod decoder;
mod encoder;
mod error;
mod frame;
mod octets;
mod parts;
mod qpack;
// mod octets;
mod stream;

pub use decoder::FrameDecoder;
pub use encoder::FrameEncoder;
pub use error::{DecodeError, EncodeError, H3Error, H3ErrorCode};
pub use frame::{
    Data, Frame, Headers, Payload, Settings, DATA_FRAME_TYPE, HEADERS_FRAME_TYPE,
    SETTINGS_FRAME_TYPE,
};
pub use parts::Parts;
pub use stream::{
    FrameKind, Frames, StreamMessage, CONTROL_STREAM_TYPE, QPACK_DECODER_STREAM_TYPE,
    QPACK_ENCODER_STREAM_TYPE,
};

pub use crate::pseudo::PseudoHeaders;

pub(crate) fn is_bidirectional(id: u64) -> bool {
    (id & 0x02) == 0
}
