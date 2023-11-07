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

#![rustfmt::skip]

use crate::h3::qpack::table::Field;
use crate::h3::pseudo::PseudoHeaders;
use crate::headers::Headers;

/// HTTP2 HEADERS frame payload implementation.
#[derive(PartialEq, Eq, Clone)]
pub struct Parts {
    pub(crate) pseudo: PseudoHeaders,
    pub(crate) map: Headers,
}

impl Parts {
    /// The constructor of `Parts`
    pub fn new() -> Self {
        Self {
            pseudo: PseudoHeaders::new(),
            map: Headers::new(),
        }
    }

    /// Sets pseudo headers for `Parts`.
    pub fn set_pseudo(&mut self, pseudo: PseudoHeaders) {
        self.pseudo = pseudo;
    }

    /// Sets regular field lines for `Parts`.
    pub fn set_header_lines(&mut self, headers: Headers) {
        self.map = headers;
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.pseudo.is_empty() && self.map.is_empty()
    }

    pub(crate) fn update(&mut self, headers: Field, value: String) {
        match headers {
            Field::Authority => self.pseudo.set_authority(Some(value)),
            Field::Method => self.pseudo.set_method(Some(value)),
            Field::Path => self.pseudo.set_path(Some(value)),
            Field::Scheme => self.pseudo.set_scheme(Some(value)),
            Field::Status => self.pseudo.set_status(Some(value)),
            Field::Other(header) => self.map.append(header.as_str(), value.as_str()).unwrap(),
        }
    }

    pub(crate) fn parts(&self) -> (&PseudoHeaders, &Headers) {
        (&self.pseudo, &self.map)
    }

    pub(crate) fn into_parts(self) -> (PseudoHeaders, Headers) {
        (self.pseudo, self.map)
    }
}

impl Default for Parts {
    fn default() -> Self {
        Self::new()
    }
}
