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

//! Client configure module.

use crate::util::{Redirect, Retry, Timeout};

/// Options and flags which can be used to configure a client.
pub(crate) struct ClientConfig {
    pub(crate) redirect: Redirect,
    pub(crate) retry: Retry,
    pub(crate) connect_timeout: Timeout,
    pub(crate) request_timeout: Timeout,
}

impl ClientConfig {
    /// Creates a new and default `ClientConfig`.
    pub(crate) fn new() -> Self {
        Self {
            redirect: Redirect::no_limit(),
            retry: Retry::none(),
            connect_timeout: Timeout::none(),
            request_timeout: Timeout::none(),
        }
    }
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self::new()
    }
}
