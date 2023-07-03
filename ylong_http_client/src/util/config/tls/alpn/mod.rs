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

/// TLS Application-Layer Protocol Negotiation (ALPN) Protocol is defined in [`RFC7301`].
/// `AlpnProtocol` contains some protocols used in HTTP, which registered in [`IANA`].
///
/// [`RFC7301`]: https://www.rfc-editor.org/rfc/rfc7301.html#section-3
/// [`IANA`]: https://www.iana.org/assignments/tls-extensiontype-values/tls-extensiontype-values.xhtml#alpn-protocol-ids
///
/// # Examples
/// ```
/// use ylong_http_client::util::AlpnProtocol;
///
/// let alpn = AlpnProtocol::HTTP11;
/// assert_eq!(alpn.as_use_bytes(), b"\x08http/1.1");
/// assert_eq!(alpn.id_sequence(), b"http/1.1");
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AlpnProtocol(Inner);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Inner {
    HTTP09,
    HTTP10,
    HTTP11,
    SPDY1,
    SPDY2,
    SPDY3,
    H2,
    H2C,
    H3,
}

impl AlpnProtocol {
    /// `HTTP/0.9` in [`IANA Registration`].
    ///
    /// [`IANA Registration`]: https://www.iana.org/assignments/tls-extensiontype-values/tls-extensiontype-values.xhtml#alpn-protocol-ids
    pub const HTTP09: Self = Self(Inner::HTTP09);

    /// `HTTP/1.0` in [`IANA Registration`].
    ///
    /// [`IANA Registration`]: https://www.iana.org/assignments/tls-extensiontype-values/tls-extensiontype-values.xhtml#alpn-protocol-ids
    pub const HTTP10: Self = Self(Inner::HTTP10);

    /// `HTTP/1.1` in [`IANA Registration`].
    ///
    /// [`IANA Registration`]: https://www.iana.org/assignments/tls-extensiontype-values/tls-extensiontype-values.xhtml#alpn-protocol-ids
    pub const HTTP11: Self = Self(Inner::HTTP11);

    /// `SPDY/1` in [`IANA Registration`].
    ///
    /// [`IANA Registration`]: https://www.iana.org/assignments/tls-extensiontype-values/tls-extensiontype-values.xhtml#alpn-protocol-ids
    pub const SPDY1: Self = Self(Inner::SPDY1);

    /// `SPDY/2` in [`IANA Registration`].
    ///
    /// [`IANA Registration`]: https://www.iana.org/assignments/tls-extensiontype-values/tls-extensiontype-values.xhtml#alpn-protocol-ids
    pub const SPDY2: Self = Self(Inner::SPDY2);

    /// `SPDY/3` in [`IANA Registration`].
    ///
    /// [`IANA Registration`]: https://www.iana.org/assignments/tls-extensiontype-values/tls-extensiontype-values.xhtml#alpn-protocol-ids
    pub const SPDY3: Self = Self(Inner::SPDY3);

    /// `HTTP/2 over TLS` in [`IANA Registration`].
    ///
    /// [`IANA Registration`]: https://www.iana.org/assignments/tls-extensiontype-values/tls-extensiontype-values.xhtml#alpn-protocol-ids
    pub const H2: Self = Self(Inner::H2);

    /// `HTTP/2 over TCP` in [`IANA Registration`].
    ///
    /// [`IANA Registration`]: https://www.iana.org/assignments/tls-extensiontype-values/tls-extensiontype-values.xhtml#alpn-protocol-ids
    pub const H2C: Self = Self(Inner::H2C);

    /// `HTTP/3` in [`IANA Registration`].
    ///
    /// [`IANA Registration`]: https://www.iana.org/assignments/tls-extensiontype-values/tls-extensiontype-values.xhtml#alpn-protocol-ids
    pub const H3: Self = Self(Inner::H3);

    /// Gets ALPN “wire format”, which consists protocol name prefixed by its byte length.
    pub fn as_use_bytes(&self) -> &[u8] {
        match *self {
            AlpnProtocol::HTTP09 => b"\x08http/0.9",
            AlpnProtocol::HTTP10 => b"\x08http/1.0",
            AlpnProtocol::HTTP11 => b"\x08http/1.1",
            AlpnProtocol::SPDY1 => b"\x06spdy/1",
            AlpnProtocol::SPDY2 => b"\x06spdy/2",
            AlpnProtocol::SPDY3 => b"\x06spdy/3",
            AlpnProtocol::H2 => b"\x02h2",
            AlpnProtocol::H2C => b"\x03h2c",
            AlpnProtocol::H3 => b"\x02h3",
        }
    }

    /// Gets ALPN protocol name, which also called identification sequence.
    pub fn id_sequence(&self) -> &[u8] {
        &self.as_use_bytes()[1..]
    }
}

/// `AlpnProtocolList` consists of a sequence of supported protocol names
/// prefixed by their byte length.
///
/// # Examples
/// ```
/// use ylong_http_client::util::{AlpnProtocol, AlpnProtocolList};
///
/// let list = AlpnProtocolList::new()
///     .extend(AlpnProtocol::SPDY1)
///     .extend(AlpnProtocol::HTTP11);
/// assert_eq!(list.as_slice(), b"\x06spdy/1\x08http/1.1");
/// ```
#[derive(Debug, Default)]
pub struct AlpnProtocolList(Vec<u8>);

impl AlpnProtocolList {
    /// Creates a new `AlpnProtocolList`.
    pub fn new() -> Self {
        AlpnProtocolList(vec![])
    }

    fn extend_from_slice(&mut self, other: &[u8]) {
        self.0.extend_from_slice(other);
    }

    /// Adds an `AlpnProtocol`.
    pub fn extend(mut self, protocol: AlpnProtocol) -> Self {
        self.extend_from_slice(protocol.as_use_bytes());
        self
    }

    /// Gets `Vec<u8>` of ALPN “wire format”, which consists of a sequence of
    /// supported protocol names prefixed by their byte length.
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    /// Gets `&[u8]` of ALPN “wire format”, which consists of a sequence of
    /// supported protocol names prefixed by their byte length.
    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }
}

#[cfg(test)]
mod ut_alpn {
    use crate::util::{AlpnProtocol, AlpnProtocolList};

    /// UT test cases for `AlpnProtocol::as_use_bytes`.
    ///
    /// # Brief
    /// 1. Creates a `AlpnProtocol`.
    /// 2. Gets `&[u8]` by AlpnProtocol::as_use_bytes.
    /// 3. Checks whether the result is correct.
    #[test]
    fn ut_alpn_as_use_bytes() {
        assert_eq!(AlpnProtocol::HTTP09.as_use_bytes(), b"\x08http/0.9");
    }

    /// UT test cases for `AlpnProtocol::id_sequence`.
    ///
    /// # Brief
    /// 1. Creates a `AlpnProtocol`.
    /// 2. Gets `&[u8]` by AlpnProtocol::id_sequence.
    /// 3. Checks whether the result is correct.
    #[test]
    fn ut_alpn_id_sequence() {
        assert_eq!(AlpnProtocol::HTTP09.id_sequence(), b"http/0.9");
    }

    /// UT test cases for `AlpnProtocolList::new`.
    ///
    /// # Brief
    /// 1. Creates a `AlpnProtocolList` by `AlpnProtocolList::new`.
    /// 2. Checks whether the result is correct.
    #[test]
    fn ut_alpn_list_new() {
        assert_eq!(AlpnProtocolList::new().as_slice(), b"");
    }

    /// UT test cases for `AlpnProtocolList::add`.
    ///
    /// # Brief
    /// 1. Creates a `AlpnProtocolList` by `AlpnProtocolList::new`.
    /// 2. Adds several `AlpnProtocol`s.
    /// 3. Checks whether the result is correct.
    #[test]
    fn ut_alpn_list_add() {
        assert_eq!(
            AlpnProtocolList::new()
                .extend(AlpnProtocol::SPDY1)
                .extend(AlpnProtocol::HTTP11)
                .as_slice(),
            b"\x06spdy/1\x08http/1.1"
        );
    }

    /// UT test cases for `AlpnProtocolList::as_slice`.
    ///
    /// # Brief
    /// 1. Creates a `AlpnProtocolList` and adds several `AlpnProtocol`s.
    /// 2. Gets slice by `AlpnProtocolList::as_slice`.
    /// 3. Checks whether the result is correct.
    #[test]
    fn ut_alpn_list_as_slice() {
        assert_eq!(
            AlpnProtocolList::new()
                .extend(AlpnProtocol::HTTP09)
                .as_slice(),
            b"\x08http/0.9"
        );
    }

    /// UT test cases for `AlpnProtocolList::to_bytes`.
    ///
    /// # Brief
    /// 1. Creates a `AlpnProtocolList` and adds several `AlpnProtocol`s.
    /// 2. Gets bytes by `AlpnProtocolList::to_bytes`.
    /// 3. Checks whether the result is correct.
    #[test]
    fn ut_alpn_list_to_bytes() {
        assert_eq!(
            AlpnProtocolList::new()
                .extend(AlpnProtocol::SPDY1)
                .extend(AlpnProtocol::HTTP11)
                .into_bytes(),
            b"\x06spdy/1\x08http/1.1".to_vec()
        );
    }
}
