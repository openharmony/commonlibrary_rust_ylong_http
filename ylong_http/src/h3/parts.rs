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

use crate::h3::pseudo::PseudoHeaders;
use crate::h3::qpack::table::NameField;
use crate::headers::Headers;

/// HTTP3 HEADERS frame payload implementation.
#[derive(PartialEq, Eq, Clone, Debug)]
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

    /// Whether the Headers part is empty.
    pub fn is_empty(&self) -> bool {
        self.pseudo.is_empty() && self.map.is_empty()
    }

    /// Updates a field in the Headers part.
    pub fn update(&mut self, headers: NameField, value: String) {
        match headers {
            NameField::Authority => self.pseudo.set_authority(Some(value)),
            NameField::Method => self.pseudo.set_method(Some(value)),
            NameField::Path => self.pseudo.set_path(Some(value)),
            NameField::Scheme => self.pseudo.set_scheme(Some(value)),
            NameField::Status => self.pseudo.set_status(Some(value)),
            NameField::Other(header) => self.map.append(header.as_str(), value.as_str()).unwrap(),
        }
    }

    /// Gets Headers part.
    pub fn parts(&self) -> (&PseudoHeaders, &Headers) {
        (&self.pseudo, &self.map)
    }

    /// Takes ownership of parts and separate Headers and pseudo.
    pub fn into_parts(self) -> (PseudoHeaders, Headers) {
        (self.pseudo, self.map)
    }
}

impl Default for Parts {
    fn default() -> Self {
        Self::new()
    }
}
