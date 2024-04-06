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

//! Connector configure module.

use crate::util::proxy::Proxies;

#[derive(Default)]
pub(crate) struct ConnectorConfig {
    pub(crate) proxies: Proxies,

    #[cfg(feature = "__tls")]
    pub(crate) tls: crate::util::TlsConfig,
}

#[cfg(test)]
mod ut_connector_config {
    use ylong_http::request::uri::Uri;

    use crate::util::config::ConnectorConfig;

    /// UT test cases for `ConnectorConfig::default`.
    ///
    /// # Brief
    /// 1. Creates a `ConnectorConfig` by calling `ConnectorConfig::default`.
    /// 2. Checks if the result is as expected.
    #[test]
    fn ut_connector_config_default() {
        let config = ConnectorConfig::default();
        let uri = Uri::from_bytes(b"http://127.0.0.1").unwrap();
        assert!(config.proxies.match_proxy(&uri).is_none())
    }
}
