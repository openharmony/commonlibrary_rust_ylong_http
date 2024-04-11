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

use std::collections::HashMap;

use libc::c_int;
use ylong_http::request::uri::Uri;

use crate::util::c_openssl::error::ErrorStack;
use crate::util::c_openssl::ffi::x509::{
    EVP_DigestFinal_ex, EVP_DigestInit, EVP_DigestUpdate, EVP_MD_CTX_free, EVP_MD_CTX_new,
    EVP_sha256,
};
use crate::util::c_openssl::ssl::{InternalError, SslError, SslErrorCode};
use crate::ErrorKind::Build;
use crate::HttpClientError;

/// A structure that serves Certificate and Public Key Pinning.
/// The map key is server authority(host:port), value is Base64(sha256(Server's
/// Public Key)).
///
/// # Examples
///
/// ```
/// use ylong_http_client::PubKeyPins;
///
/// let pins = PubKeyPins::builder()
///     .add(
///         "https://example.com",
///         "sha256//VHQAbNl67nmkZJNESeYKvTxb5bTmd1maWnMKG/tjcAY=",
///     )
///     .build()
///     .unwrap();
/// ```
#[derive(Clone)]
pub struct PubKeyPins {
    pub(crate) pub_keys: HashMap<String, String>,
}

/// A builder which is used to construct `PubKeyPins`.
///
/// # Examples
///
/// ```
/// use ylong_http_client::PubKeyPinsBuilder;
///
/// let builder = PubKeyPinsBuilder::new();
/// ```
pub struct PubKeyPinsBuilder {
    pub_keys: Result<HashMap<String, String>, HttpClientError>,
}

impl PubKeyPinsBuilder {
    /// Creates a new `PubKeyPinsBuilder`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::PubKeyPinsBuilder;
    ///
    /// let builder = PubKeyPinsBuilder::new();
    /// ```
    pub fn new() -> Self {
        Self {
            pub_keys: Ok(HashMap::new()),
        }
    }

    /// Sets a tuple of (server, public key digest) for `PubKeyPins`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::PubKeyPinsBuilder;
    ///
    /// let pins = PubKeyPinsBuilder::new()
    ///     .add(
    ///         "https://example.com",
    ///         "sha256//VHQAbNl67nmkZJNESeYKvTxb5bTmd1maWnMKG/tjcAY=",
    ///     )
    ///     .build()
    ///     .unwrap();
    /// ```
    pub fn add(mut self, uri: &str, digest: &str) -> Self {
        self.pub_keys = self.pub_keys.and_then(move |mut keys| {
            let parsed = Uri::try_from(uri).map_err(|e| HttpClientError::from_error(Build, e))?;
            let auth = match (parsed.host(), parsed.port()) {
                (None, _) => {
                    return err_from_msg!(Build, "uri has no host");
                }
                (Some(host), Some(port)) => {
                    format!("{}:{}", host.as_str(), port.as_str())
                }
                (Some(host), None) => {
                    format!("{}:443", host.as_str())
                }
            };
            let pub_key = String::from(digest);
            let _ = keys.insert(auth, pub_key);
            Ok(keys)
        });
        self
    }

    /// Builds a `PubKeyPins`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::PubKeyPinsBuilder;
    ///
    /// let pins = PubKeyPinsBuilder::new()
    ///     .add(
    ///         "https://example.com",
    ///         "sha256//VHQAbNl67nmkZJNESeYKvTxb5bTmd1maWnMKG/tjcAY=",
    ///     )
    ///     .build()
    ///     .unwrap();
    /// ```
    pub fn build(self) -> Result<PubKeyPins, HttpClientError> {
        Ok(PubKeyPins {
            pub_keys: self.pub_keys?,
        })
    }
}

impl PubKeyPins {
    /// Creates a new builder for  `PubKeyPins`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::PubKeyPins;
    ///
    /// let builder = PubKeyPins::builder();
    /// ```
    pub fn builder() -> PubKeyPinsBuilder {
        PubKeyPinsBuilder::new()
    }
    pub(crate) fn get_pin(&self, domain: &str) -> Option<String> {
        self.pub_keys.get(&String::from(domain)).cloned()
    }
}

/// The Default implement of `PubKeyPinsBuilder`.
impl Default for PubKeyPinsBuilder {
    /// Creates a new builder for  `PubKeyPins`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_http_client::PubKeyPinsBuilder;
    ///
    /// let builder = PubKeyPinsBuilder::default();
    /// ```
    fn default() -> Self {
        Self::new()
    }
}

// TODO The SSLError thrown here is meaningless and has no information.
pub(crate) unsafe fn sha256_digest(
    pub_key: &[u8],
    len: c_int,
    digest: &mut [u8],
) -> Result<(), SslError> {
    let md_ctx = EVP_MD_CTX_new();
    if md_ctx.is_null() {
        return Err(SslError {
            code: SslErrorCode::SSL,
            internal: Some(InternalError::Ssl(ErrorStack::get())),
        });
    }
    let init = EVP_DigestInit(md_ctx, EVP_sha256());
    if init == 0 {
        EVP_MD_CTX_free(md_ctx);
        return Err(SslError {
            code: SslErrorCode::SSL,
            internal: Some(InternalError::Ssl(ErrorStack::get())),
        });
    }
    EVP_DigestUpdate(md_ctx, pub_key.as_ptr(), len);

    let start = 0;
    EVP_DigestFinal_ex(md_ctx, digest.as_mut_ptr(), &start);

    EVP_MD_CTX_free(md_ctx);

    Ok(())
}

#[cfg(test)]
mod ut_verify_pinning {
    use std::collections::HashMap;

    use libc::c_int;

    use crate::util::c_openssl::verify::sha256_digest;
    use crate::{PubKeyPins, PubKeyPinsBuilder};

    /// UT test cases for `PubKeyPins::clone`.
    ///
    /// # Brief
    /// 1. Creates a `PubKeyPins`.
    /// 2. Calls `PubKeyPins::clone` .
    /// 3. Checks if the assert result is correct.
    #[test]
    fn ut_pubkey_pins_clone() {
        let mut map = HashMap::new();
        let _value = map.insert(
            "ylong_http.com:443".to_string(),
            "sha256//t62CeU2tQiqkexU74Gxa2eg7fRbEgoChTociMee9wno=".to_string(),
        );
        let pins = PubKeyPins { pub_keys: map };
        let pins_clone = pins.clone();
        assert_eq!(pins.pub_keys, pins_clone.pub_keys);
    }

    /// UT test cases for `PubKeyPinsBuilder::add`.
    ///
    /// # Brief
    /// 1. Creates a `PubKeyPinsBuilder`.
    /// 2. Calls `PubKeyPins::add` .
    /// 3. Checks if the assert result is correct.
    #[test]
    fn ut_pubkey_pins_builder_add() {
        let pins = PubKeyPins::builder()
            .add(
                "/data/storage",
                "sha256//t62CeU2tQiqkexU74Gxa2eg7fRbEgoChTociMee9wno=",
            )
            .build()
            .err();
        assert_eq!(
            format!("{:?}", pins.unwrap()),
            "HttpClientError { ErrorKind: Build, Cause: uri has no host }"
        );
        let pins = PubKeyPinsBuilder::default()
            .add(
                "https://ylong_http.com",
                "sha256//t62CeU2tQiqkexU74Gxa2eg7fRbEgoChTociMee9wno=",
            )
            .build()
            .unwrap();
        assert_eq!(
            pins.get_pin("ylong_http.com:443"),
            Some("sha256//t62CeU2tQiqkexU74Gxa2eg7fRbEgoChTociMee9wno=".to_string())
        );
    }

    /// UT test cases for `sha256_digest.
    ///
    /// # Brief
    /// 1. Calls `sha256_digest` .
    /// 2. Checks if the assert result is correct.
    #[test]
    fn ut_pubkey_sha256_digest() {
        let pubkey =
            bytes_from_hex("d0e8b8f11c98f369016eb2ed3c541e1f01382f9d5b3104c9ffd06b6175a46271")
                .unwrap();

        let key_words = Vec::from("Hello, SHA-256!");

        let mut hash = [0u8; 32];
        assert!(unsafe {
            sha256_digest(key_words.as_slice(), key_words.len() as c_int, &mut hash)
        }
        .is_ok());

        assert_eq!(hash.as_slice(), pubkey.as_slice());
    }

    fn bytes_from_hex(str: &str) -> Option<Vec<u8>> {
        if str.len() % 2 != 0 {
            return None;
        }
        let mut vec = Vec::new();
        let mut remained = str;
        while !remained.is_empty() {
            let (left, right) = remained.split_at(2);
            match u8::from_str_radix(left, 16) {
                Ok(num) => vec.push(num),
                Err(_) => return None,
            }
            remained = right;
        }
        Some(vec)
    }
}
