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

/// [Pseudo-Header fields] that may appear in http2 header fields.
///
/// [Pseudo-Header fields]: https://httpwg.org/specs/rfc9113.html#PseudoHeaderFields
///
/// # Note
/// The current structure is not responsible for checking every value.
// TODO: Consider Split PseudoHeaders into `RequestPseudo` and `ResponsePseudo`.
#[derive(Clone, PartialEq, Eq)]
pub struct PseudoHeaders {
    pub(crate) authority: Option<String>,
    pub(crate) method: Option<String>,
    pub(crate) path: Option<String>,
    pub(crate) scheme: Option<String>,
    pub(crate) status: Option<String>,
}

impl PseudoHeaders {
    /// Create a new `PseudoHeaders`.
    pub(crate) fn new() -> Self {
        Self {
            authority: None,
            method: None,
            path: None,
            scheme: None,
            status: None,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.authority.is_none()
            && self.method.is_none()
            && self.path.is_none()
            && self.scheme.is_none()
            && self.status.is_none()
    }

    /// Check if it contains `Authority`.
    pub(crate) fn contains_authority(&self) -> bool {
        self.authority.is_some()
    }

    /// Take the `String` value of `Authority`.
    pub(crate) fn take_authority(&mut self) -> Option<String> {
        self.authority.take()
    }

    /// Check if it contains `Method`.
    pub(crate) fn contains_method(&self) -> bool {
        self.method.is_some()
    }

    /// Take the `String` value of `Method`.
    pub(crate) fn take_method(&mut self) -> Option<String> {
        self.method.take()
    }

    /// Check if it contains `Path`.
    pub(crate) fn contains_path(&self) -> bool {
        self.path.is_some()
    }

    /// Take the `String` value of `Path`.
    pub(crate) fn take_path(&mut self) -> Option<String> {
        self.path.take()
    }

    /// Check if it contains `Scheme`.
    pub(crate) fn contains_scheme(&self) -> bool {
        self.scheme.is_some()
    }

    /// Take the `String` value of `Scheme`.
    pub(crate) fn take_scheme(&mut self) -> Option<String> {
        self.scheme.take()
    }

    /// Check if it contains `Status`.
    pub(crate) fn contains_status(&self) -> bool {
        self.status.is_some()
    }

    /// Take the `String` value of `Status`.
    pub(crate) fn take_status(&mut self) -> Option<String> {
        self.status.take()
    }
}

impl Default for PseudoHeaders {
    fn default() -> Self {
        PseudoHeaders::new()
    }
}

#[cfg(test)]
mod ut_pseudo_headers {
    use crate::pseudo::PseudoHeaders;

    /// UT test cases for `PseudoHeaders::new`.
    ///
    /// # Brief
    /// 1. Calls `PseudoHeaders::new` to create a `PseudoHeaders`.
    /// 2. Checks if the result has a default value.
    #[test]
    fn ut_pseudo_headers_new() {
        let pseudo = PseudoHeaders::new();
        assert!(pseudo.authority.is_none());
        assert!(pseudo.method.is_none());
        assert!(pseudo.path.is_none());
        assert!(pseudo.scheme.is_none());
        assert!(pseudo.status.is_none());
    }

    /// UT test cases for `PseudoHeaders::contains_authority`.
    ///
    /// # Brief
    /// 1. Creates a `PseudoHeaders`.
    /// 2. Calls `PseudoHeaders::contains_authority` of it.
    /// 3. Calls `PseudoHeaders::contains_authority` of it after its `authority`
    ///    is set.
    /// 4. Checks the results.
    #[test]
    fn ut_pseudo_headers_contains_authority() {
        let mut pseudo = PseudoHeaders::new();
        assert!(!pseudo.contains_authority());

        pseudo.authority = Some(String::from("authority"));
        assert!(pseudo.contains_authority());
    }

    /// UT test cases for `PseudoHeaders::authority`.
    ///
    /// # Brief
    /// 1. Creates a `PseudoHeaders`.
    /// 2. Calls `PseudoHeaders::authority` of it.
    /// 3. Calls `PseudoHeaders::authority` of it after its `authority` is set.
    /// 4. Checks the results.
    #[test]
    fn ut_pseudo_headers_authority() {
        let mut pseudo = PseudoHeaders::new();
        assert!(pseudo.authority.is_none());

        pseudo.authority = Some(String::from("authority"));
        assert_eq!(pseudo.authority, Some(String::from("authority")));
    }

    /// UT test cases for `PseudoHeaders::set_authority`.
    ///
    /// # Brief
    /// 1. Creates a `PseudoHeaders`.
    /// 2. Calls `PseudoHeaders::set_authority` of it to set `authority` a
    ///    value.
    /// 3. Checks the results.
    #[test]
    fn ut_pseudo_headers_set_authority() {
        let mut pseudo = PseudoHeaders::new();
        assert!(pseudo.authority.is_none());

        pseudo.authority = Some(String::from("authority"));
        assert_eq!(pseudo.authority, Some(String::from("authority")));

        pseudo.authority = None;
        assert!(pseudo.authority.is_none());
    }

    /// UT test cases for `PseudoHeaders::take_authority`.
    ///
    /// # Brief
    /// 1. Creates a `PseudoHeaders`.
    /// 2. Calls `PseudoHeaders::take_authority` of it.
    /// 3. Calls `PseudoHeaders::take_authority` of it after its `authority` is
    ///    set.
    /// 4. Checks the results.
    #[test]
    fn ut_pseudo_headers_take_authority() {
        let mut pseudo = PseudoHeaders::new();
        assert!(pseudo.take_authority().is_none());

        pseudo.authority = Some(String::from("authority"));
        assert_eq!(pseudo.take_authority(), Some(String::from("authority")));
    }

    /// UT test cases for `PseudoHeaders::contains_method`.
    ///
    /// # Brief
    /// 1. Creates a `PseudoHeaders`.
    /// 2. Calls `PseudoHeaders::contains_method` of it.
    /// 3. Calls `PseudoHeaders::contains_method` of it after its `method` is
    ///    set.
    /// 4. Checks the results.
    #[test]
    fn ut_pseudo_headers_contains_method() {
        let mut pseudo = PseudoHeaders::new();
        assert!(!pseudo.contains_method());

        pseudo.method = Some(String::from("method"));
        assert!(pseudo.contains_method());
    }

    /// UT test cases for `PseudoHeaders::contains_path`.
    ///
    /// # Brief
    /// 1. Creates a `PseudoHeaders`.
    /// 2. Calls `PseudoHeaders::contains_path` of it.
    /// 3. Calls `PseudoHeaders::contains_path` of it after its `path` is set.
    /// 4. Checks the results.
    #[test]
    fn ut_pseudo_headers_contains_path() {
        let mut pseudo = PseudoHeaders::new();
        assert!(!pseudo.contains_path());

        pseudo.path = Some(String::from("path"));
        assert!(pseudo.contains_path());
    }

    /// UT test cases for `PseudoHeaders::take_path`.
    ///
    /// # Brief
    /// 1. Creates a `PseudoHeaders`.
    /// 2. Calls `PseudoHeaders::take_path` of it.
    /// 3. Calls `PseudoHeaders::take_path` of it after its `path` is set.
    /// 4. Checks the results.
    #[test]
    fn ut_pseudo_headers_take_path() {
        let mut pseudo = PseudoHeaders::new();
        assert!(pseudo.take_path().is_none());

        pseudo.path = Some(String::from("path"));
        assert_eq!(pseudo.take_path(), Some(String::from("path")));
    }

    /// UT test cases for `PseudoHeaders::contains_scheme`.
    ///
    /// # Brief
    /// 1. Creates a `PseudoHeaders`.
    /// 2. Calls `PseudoHeaders::contains_scheme` of it.
    /// 3. Calls `PseudoHeaders::contains_scheme` of it after its `scheme` is
    ///    set.
    /// 4. Checks the results.
    #[test]
    fn ut_pseudo_headers_contains_scheme() {
        let mut pseudo = PseudoHeaders::new();
        assert!(!pseudo.contains_scheme());

        pseudo.scheme = Some(String::from("scheme"));
        assert!(pseudo.contains_scheme());
    }

    /// UT test cases for `PseudoHeaders::take_scheme`.
    ///
    /// # Brief
    /// 1. Creates a `PseudoHeaders`.
    /// 2. Calls `PseudoHeaders::take_scheme` of it.
    /// 3. Calls `PseudoHeaders::take_scheme` of it after its `scheme` is set.
    /// 4. Checks the results.
    #[test]
    fn ut_pseudo_headers_take_scheme() {
        let mut pseudo = PseudoHeaders::new();
        assert!(pseudo.take_scheme().is_none());

        pseudo.scheme = Some(String::from("scheme"));
        assert_eq!(pseudo.take_scheme(), Some(String::from("scheme")));
    }

    /// UT test cases for `PseudoHeaders::contains_status`.
    ///
    /// # Brief
    /// 1. Creates a `PseudoHeaders`.
    /// 2. Calls `PseudoHeaders::contains_status` of it.
    /// 3. Calls `PseudoHeaders::contains_status` of it after its `status` is
    ///    set.
    /// 4. Checks the results.
    #[test]
    fn ut_pseudo_headers_contains_status() {
        let mut pseudo = PseudoHeaders::new();
        assert!(!pseudo.contains_status());

        pseudo.status = Some(String::from("status"));
        assert!(pseudo.contains_status());
    }

    /// UT test cases for `PseudoHeaders::take_status`.
    ///
    /// # Brief
    /// 1. Creates a `PseudoHeaders`.
    /// 2. Calls `PseudoHeaders::take_status` of it.
    /// 3. Calls `PseudoHeaders::take_status` of it after its `status` is set.
    /// 4. Checks the results.
    #[test]
    fn ut_pseudo_headers_take_status() {
        let mut pseudo = PseudoHeaders::new();
        assert!(pseudo.take_status().is_none());

        pseudo.status = Some(String::from("status"));
        assert_eq!(pseudo.take_status(), Some(String::from("status")));
    }
}
