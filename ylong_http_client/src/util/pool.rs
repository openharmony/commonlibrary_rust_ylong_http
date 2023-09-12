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

//! Connection pool implementation.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Arc, Mutex};

use ylong_http::request::uri::{Authority, Scheme};

pub(crate) struct Pool<K, V> {
    pool: Arc<Mutex<HashMap<K, V>>>,
}

impl<K, V> Pool<K, V> {
    pub(crate) fn new() -> Self {
        Self {
            pool: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl<K: Eq + Hash, V: Clone> Pool<K, V> {
    pub(crate) fn get<F>(&self, key: K, create_fn: F) -> V
    where
        F: FnOnce() -> V,
    {
        let mut inner = self.pool.lock().unwrap();
        match (*inner).entry(key) {
            Entry::Occupied(conns) => conns.get().clone(),
            Entry::Vacant(e) => e.insert(create_fn()).clone(),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub(crate) struct PoolKey(Scheme, Authority);

impl PoolKey {
    pub(crate) fn new(scheme: Scheme, authority: Authority) -> Self {
        Self(scheme, authority)
    }
}

#[cfg(test)]
mod ut_pool {
    use ylong_http::request::uri::Uri;

    use crate::pool::{Pool, PoolKey};

    /// UT test cases for `Pool::get`.
    ///
    /// # Brief
    /// 1. Creates a `pool` by calling `Pool::new()`.
    /// 2. Uses `pool::get` to get connection.
    /// 3. Checks if the results are correct.
    #[test]
    fn ut_pool_get() {
        let uri = Uri::from_bytes(b"http://example1.com:80/foo?a=1").unwrap();
        let key = PoolKey::new(
            uri.scheme().unwrap().clone(),
            uri.authority().unwrap().clone(),
        );
        let data = String::from("Data info");
        let consume_and_return_data = move || data;
        let pool = Pool::new();
        let res = pool.get(key, consume_and_return_data);
        assert_eq!(res, "Data info".to_string());
    }
}
