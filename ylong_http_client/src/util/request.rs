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

//! `ylong_http_client` `Request` reference.

use std::cell::UnsafeCell;
use std::sync::Arc;

use crate::async_impl::Request;

pub(crate) struct ReqCell {
    request: UnsafeCell<Request>,
}

impl ReqCell {
    pub(crate) fn new(request: Request) -> Self {
        Self {
            request: UnsafeCell::new(request),
        }
    }
}

unsafe impl Sync for ReqCell {}

pub(crate) struct RequestArc {
    pub(crate) cell: Arc<ReqCell>,
}

impl RequestArc {
    pub(crate) fn new(request: Request) -> Self {
        Self {
            cell: Arc::new(ReqCell::new(request)),
        }
    }

    pub(crate) fn ref_mut(&mut self) -> &mut Request {
        // SAFETY: In the case of `HTTP`, only one coroutine gets the handle
        // at the same time.
        unsafe { &mut *self.cell.request.get() }
    }
}

impl Clone for RequestArc {
    fn clone(&self) -> Self {
        Self {
            cell: self.cell.clone(),
        }
    }
}
