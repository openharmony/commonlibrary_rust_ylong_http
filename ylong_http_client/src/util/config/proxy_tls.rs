// Copyright (c) 2026 Huawei Device Co., Ltd.
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

//! TLS configuration used by HTTPS proxy connections.

use crate::util::{TlsConfig, TlsConfigBuilder, TlsFileType, TlsVersion};
use crate::HttpClientError;

/// TLS configuration for connecting to an HTTPS proxy server.
#[derive(Clone, Default)]
pub(crate) struct ProxyTlsConfig {
    pub(crate) ca_file: Option<String>,
    pub(crate) identity: Option<ProxyIdentity>,
    pub(crate) cipher_list: Option<String>,
    pub(crate) min_version: Option<TlsVersion>,
    pub(crate) max_version: Option<TlsVersion>,
    pub(crate) accept_invalid_certs: bool,
    pub(crate) accept_invalid_hostnames: bool,
}

impl ProxyTlsConfig {
    pub(crate) fn builder(&self) -> TlsConfigBuilder {
        let mut builder = TlsConfig::builder();

        if let Some(ca_file) = self.ca_file.as_deref() {
            builder = builder.ca_file(ca_file);
        }

        if let Some(identity) = &self.identity {
            builder = identity.apply_to_builder(builder);
        }

        if let Some(cipher_list) = self.cipher_list.as_deref() {
            builder = builder.cipher_list(cipher_list);
        }

        if let Some(version) = self.min_version {
            builder = builder.min_proto_version(version);
        }

        if let Some(version) = self.max_version {
            builder = builder.max_proto_version(version);
        }

        if self.accept_invalid_certs {
            builder = builder.danger_accept_invalid_certs(true);
        }

        if self.accept_invalid_hostnames {
            builder = builder.danger_accept_invalid_hostnames(true);
        }

        builder
    }

    pub(crate) fn build(&self) -> Result<TlsConfig, HttpClientError> {
        self.builder().build()
    }
}

/// Client certificate/private-key pair for proxy mTLS.
#[derive(Clone)]
pub(crate) struct ProxyIdentity {
    cert_path: String,
    key_path: String,
    file_type: TlsFileType,
}

impl ProxyIdentity {
    pub(crate) fn new(cert_path: &str, key_path: &str, file_type: TlsFileType) -> Self {
        Self {
            cert_path: cert_path.to_string(),
            key_path: key_path.to_string(),
            file_type,
        }
    }

    fn apply_to_builder(&self, builder: TlsConfigBuilder) -> TlsConfigBuilder {
        builder
            .certificate_file(&self.cert_path, self.file_type)
            .private_key_file(&self.key_path, self.file_type)
    }
}
