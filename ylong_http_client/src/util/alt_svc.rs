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
use std::str;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ylong_http::request::uri::{Host, Port};

use crate::async_impl::Response;
use crate::util::config::HttpVersion;
use crate::util::pool::PoolKey;
use crate::util::request::RequestArc;

const DEFAULT_MAX_AGE: u64 = 24 * 60 * 60;

#[derive(Clone)]
pub(crate) struct AltService {
    pub(crate) http_version: HttpVersion,
    // todo: use this later
    #[allow(unused)]
    pub(crate) src_host: Host,
    pub(crate) host: Option<Host>,
    pub(crate) port: Port,
    pub(crate) lifetime: Instant,
}

pub(crate) struct AltServiceMap {
    inner: Arc<Mutex<HashMap<PoolKey, Vec<AltService>>>>,
}

impl AltServiceMap {
    pub(crate) fn get_alt_svcs(&self, key: &PoolKey) -> Option<Vec<AltService>> {
        let mut lock = self.inner.lock().unwrap();
        let vec = lock.get_mut(key)?;
        vec.retain(|alt_scv| alt_scv.lifetime >= Instant::now());
        Some(vec.clone())
    }

    fn parse_alt_svc(src_host: &Host, values: &[u8]) -> Option<AltService> {
        // The alt_value/parameters are divided by ';'
        let mut value_it = values.split(|c| *c == b';');
        // the first value_it is alpn="[host]:port"
        let alternative = value_it.next()?;
        let mut words = alternative.split(|c| *c == b'=');

        let http_version = words.next()?.try_into().ok()?;
        let mut host_port = words.next()?;
        host_port = &host_port[1..host_port.len() - 1];
        let index = host_port.iter().position(|&x| x == b':')?;
        let (host, port) = if index == 0 {
            (
                None,
                Port::from_str(str::from_utf8(&host_port[1..]).ok()?).ok()?,
            )
        } else {
            (
                Some(Host::from_str(str::from_utf8(&host_port[..index]).ok()?).ok()?),
                Port::from_str(str::from_utf8(&host_port[(index + 1)..]).ok()?).ok()?,
            )
        };

        let mut seconds = DEFAULT_MAX_AGE;

        for para in value_it {
            let para = str::from_utf8(para).ok()?.trim().as_bytes();
            // parameter: token "=" ( token / quoted-string )
            let mut para_it = para.split(|c| *c == b'=');
            // only support ma now
            if para_it.next()? == b"ma" {
                let para = str::from_utf8(para_it.next()?).ok()?;
                seconds = para.parse::<u64>().ok()?;
                break;
            }
        }

        Some(AltService {
            http_version,
            src_host: src_host.clone(),
            host,
            port,
            lifetime: Instant::now().checked_add(Duration::from_secs(seconds))?,
        })
    }

    pub(crate) fn set_alt_svcs(&self, mut request: RequestArc, response: &Response) {
        let mut lock = self.inner.lock().unwrap();
        let uri = request.ref_mut().uri();
        let Some(scheme) = uri.scheme() else {
            return;
        };
        let Some(authority) = uri.authority() else {
            return;
        };
        let Some(host) = uri.host() else {
            return;
        };
        let key = PoolKey::new(scheme.clone(), authority.clone());
        match response.headers().get("Alt-Svc") {
            None => {}
            Some(values) => {
                let mut new_alt_svcs = Vec::new();
                for value in values.iter() {
                    let slice = value.as_slice();
                    if slice == "clear".as_bytes() {
                        lock.remove(&key);
                        return;
                    }
                    // Alt_Svcs are divided by ','
                    for slice in slice.split(|c| *c == b',') {
                        if let Some(alt_svc) = Self::parse_alt_svc(host, slice) {
                            new_alt_svcs.push(alt_svc);
                        }
                    }
                }
                lock.insert(key, new_alt_svcs);
            }
        }
    }

    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}
