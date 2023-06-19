/*
 * Copyright (c) 2023 Huawei Device Co., Ltd.
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

// TODO: Remove this file later.

use crate::{ErrorKind, HttpClientError, Uri};
use ylong_http::request::uri::Scheme;
use ylong_http::request::Request;

pub(crate) struct RequestFormatter<'a, T> {
    part: &'a mut Request<T>,
}

impl<'a, T> RequestFormatter<'a, T> {
    pub(crate) fn new(part: &'a mut Request<T>) -> Self {
        Self { part }
    }

    pub(crate) fn normalize(&mut self) -> Result<(), HttpClientError> {
        let uri_formatter = UriFormatter::new();
        uri_formatter.format(self.part.uri_mut())?;

        let host_value = self.part.uri().authority().unwrap().to_str();

        if self.part.headers_mut().get("Accept").is_none() {
            let _ = self.part.headers_mut().insert("Accept", "*/*");
        }

        if self.part.headers_mut().get("Host").is_none() {
            let _ = self
                .part
                .headers_mut()
                .insert("Host", host_value.as_bytes());
        }

        Ok(())
    }
}

pub(crate) struct UriFormatter;

impl UriFormatter {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn format(&self, uri: &mut Uri) -> Result<(), HttpClientError> {
        let host = match uri.host() {
            Some(host) => host.clone(),
            None => {
                return Err(HttpClientError::new_with_message(
                    ErrorKind::Request,
                    "No host in url",
                ))
            }
        };

        #[cfg(feature = "__tls")]
        let mut scheme = Scheme::HTTPS;

        #[cfg(not(feature = "__tls"))]
        let mut scheme = Scheme::HTTP;

        if let Some(req_scheme) = uri.scheme() {
            scheme = req_scheme.clone()
        };

        let port;

        if let Some(req_port) = uri.port().and_then(|port| port.as_u16().ok()) {
            port = req_port;
        } else {
            match scheme {
                Scheme::HTTPS => port = 443,
                Scheme::HTTP => port = 80,
            }
        }

        let mut new_uri = Uri::builder();
        new_uri = new_uri.scheme(scheme);
        new_uri = new_uri.authority(format!("{}:{}", host.as_str(), port).as_bytes());

        if let Some(path) = uri.path() {
            new_uri = new_uri.path(path.clone());
        }

        if let Some(query) = uri.query() {
            new_uri = new_uri.query(query.clone());
        }

        *uri = new_uri.build().map_err(|_| {
            HttpClientError::new_with_message(ErrorKind::Request, "Normalize url failed")
        })?;

        Ok(())
    }
}
