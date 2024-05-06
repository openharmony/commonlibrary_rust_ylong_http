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

// TLS Application-Layer Protocol Negotiation (ALPN) Protocol is defined in
// [`RFC7301`]. `AlpnProtocol` contains some protocols used in HTTP, which
// registered in [`IANA`].
//
// [`RFC7301`]: https://www.rfc-editor.org/rfc/rfc7301.html#section-3
// [`IANA`]: https://www.iana.org/assignments/tls-extensiontype-values/tls-extensiontype-values.xhtml#alpn-protocol-ids
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AlpnProtocol(Inner);

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
    pub(crate) const HTTP09: Self = Self(Inner::HTTP09);

    /// `HTTP/1.0` in [`IANA Registration`].
    ///
    /// [`IANA Registration`]: https://www.iana.org/assignments/tls-extensiontype-values/tls-extensiontype-values.xhtml#alpn-protocol-ids
    pub(crate) const HTTP10: Self = Self(Inner::HTTP10);

    /// `HTTP/1.1` in [`IANA Registration`].
    ///
    /// [`IANA Registration`]: https://www.iana.org/assignments/tls-extensiontype-values/tls-extensiontype-values.xhtml#alpn-protocol-ids
    pub(crate) const HTTP11: Self = Self(Inner::HTTP11);

    /// `SPDY/1` in [`IANA Registration`].
    ///
    /// [`IANA Registration`]: https://www.iana.org/assignments/tls-extensiontype-values/tls-extensiontype-values.xhtml#alpn-protocol-ids
    pub(crate) const SPDY1: Self = Self(Inner::SPDY1);

    /// `SPDY/2` in [`IANA Registration`].
    ///
    /// [`IANA Registration`]: https://www.iana.org/assignments/tls-extensiontype-values/tls-extensiontype-values.xhtml#alpn-protocol-ids
    pub(crate) const SPDY2: Self = Self(Inner::SPDY2);

    /// `SPDY/3` in [`IANA Registration`].
    ///
    /// [`IANA Registration`]: https://www.iana.org/assignments/tls-extensiontype-values/tls-extensiontype-values.xhtml#alpn-protocol-ids
    pub(crate) const SPDY3: Self = Self(Inner::SPDY3);

    /// `HTTP/2 over TLS` in [`IANA Registration`].
    ///
    /// [`IANA Registration`]: https://www.iana.org/assignments/tls-extensiontype-values/tls-extensiontype-values.xhtml#alpn-protocol-ids
    pub(crate) const H2: Self = Self(Inner::H2);

    /// `HTTP/2 over TCP` in [`IANA Registration`].
    ///
    /// [`IANA Registration`]: https://www.iana.org/assignments/tls-extensiontype-values/tls-extensiontype-values.xhtml#alpn-protocol-ids
    pub(crate) const H2C: Self = Self(Inner::H2C);

    /// `HTTP/3` in [`IANA Registration`].
    ///
    /// [`IANA Registration`]: https://www.iana.org/assignments/tls-extensiontype-values/tls-extensiontype-values.xhtml#alpn-protocol-ids
    pub(crate) const H3: Self = Self(Inner::H3);

    /// Gets ALPN “wire format”, which consists protocol name prefixed by its
    /// byte length.
    pub(crate) fn wire_format_bytes(&self) -> &[u8] {
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
}

/// `AlpnProtocolList` consists of a sequence of supported protocol names
/// prefixed by their byte length.
#[derive(Debug, Default)]
pub(crate) struct AlpnProtocolList(Vec<u8>);

impl AlpnProtocolList {
    /// Creates a new `AlpnProtocolList`.
    pub(crate) fn new() -> Self {
        AlpnProtocolList(vec![])
    }

    fn extend_from_slice(&mut self, other: &[u8]) {
        self.0.extend_from_slice(other);
    }

    /// Adds an `AlpnProtocol`.
    pub(crate) fn extend(mut self, protocol: AlpnProtocol) -> Self {
        self.extend_from_slice(protocol.wire_format_bytes());
        self
    }

    /// Gets `&[u8]` of ALPN “wire format”, which consists of a sequence of
    /// supported protocol names prefixed by their byte length.
    pub(crate) fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }
}

#[cfg(test)]
mod ut_alpn {
    use crate::util::{AlpnProtocol, AlpnProtocolList};

    /// UT test cases for `AlpnProtocol::wire_format_bytes`.
    ///
    /// # Brief
    /// 1. Creates a `AlpnProtocol`.
    /// 2. Gets `&[u8]` by AlpnProtocol::wire_format_bytes.
    /// 3. Checks whether the result is correct.
    #[test]
    fn ut_alpn_as_use_bytes() {
        assert_eq!(AlpnProtocol::HTTP09.wire_format_bytes(), b"\x08http/0.9");
        assert_eq!(AlpnProtocol::HTTP10.wire_format_bytes(), b"\x08http/1.0");
        assert_eq!(AlpnProtocol::HTTP11.wire_format_bytes(), b"\x08http/1.1");
        assert_eq!(AlpnProtocol::SPDY1.wire_format_bytes(), b"\x06spdy/1");
        assert_eq!(AlpnProtocol::SPDY2.wire_format_bytes(), b"\x06spdy/2");
        assert_eq!(AlpnProtocol::SPDY3.wire_format_bytes(), b"\x06spdy/3");
        assert_eq!(AlpnProtocol::H2.wire_format_bytes(), b"\x02h2");
        assert_eq!(AlpnProtocol::H2C.wire_format_bytes(), b"\x03h2c");
        assert_eq!(AlpnProtocol::H3.wire_format_bytes(), b"\x02h3");
    }

    /// UT test cases for `AlpnProtocol::clone`.
    ///
    /// # Brief
    /// 1. Creates a `AlpnProtocol`.
    /// 2. Compares the cloned values.
    /// 3. Checks whether the result is correct.
    #[test]
    fn ut_alpn_clone() {
        assert_eq!(AlpnProtocol::HTTP09, AlpnProtocol::HTTP09.clone());
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

    /// UT test cases for `AlpnProtocolList::default`.
    ///
    /// # Brief
    /// 1. Creates a `AlpnProtocolList` by `AlpnProtocolList::default`.
    /// 2. Checks whether the result is correct.
    #[test]
    fn ut_alpn_list_default() {
        assert_eq!(AlpnProtocolList::default().as_slice(), b"");
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
}
