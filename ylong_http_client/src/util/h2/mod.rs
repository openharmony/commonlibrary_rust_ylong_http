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

//! Http2 Protocol module.
//!
//! A module that manages frame transport over HTTP2 protocol.
//!
//! -[`SendData`] is used to control io write half for send frames.
//! -[`RecvData`] is used to control io read half for recv frames.
//! -[`Streams`] is used to manage the state of individual streams.
//! -[`ConnManager`] is used to coordinate the Request sending and Response
//! receiving of multiple streams.

mod buffer;
mod data_ref;
mod input;
mod manager;
mod output;
mod streams;

#[cfg(feature = "ylong_base")]
mod io;

pub(crate) use buffer::FlowControl;
pub(crate) use data_ref::BodyDataRef;
pub(crate) use input::SendData;
#[cfg(feature = "ylong_base")]
pub(crate) use io::{split, Reader, Writer};
pub(crate) use manager::ConnManager;
pub(crate) use output::RecvData;
pub(crate) use streams::{RequestWrapper, Streams};

pub const MAX_FLOW_CONTROL_WINDOW: u32 = (1 << 31) - 1;
