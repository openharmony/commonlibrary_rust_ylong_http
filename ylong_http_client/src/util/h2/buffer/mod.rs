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

//! Http2 Flow Control module.
//!
//! A module that provides Flow-Control capability for the HTTP2 protocol.
//!
//! -[`FlowControl`] is used to Provides connection-level Flow-Control
//! capability. -[`RecvWindow`] is the structure of the client response
//! receiving window. -[`SendWindow`] is the structure of the client request
//! sending window.

mod settings;
mod window;

pub(crate) use settings::FlowControl;
pub(crate) use window::{RecvWindow, SendWindow};
