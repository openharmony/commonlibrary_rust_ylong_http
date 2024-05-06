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

mod openssl;
pub use self::openssl::ServerCerts;

/// used to custom verify certs
pub trait CertVerifier {
    /// provided to users custom certs adapter
    fn verify(&self, certs: &ServerCerts) -> bool;
}

/// default cert verifier
pub struct DefaultCertVerifier {
    inner: Box<dyn CertVerifier + Send + Sync>,
}

impl DefaultCertVerifier {
    pub(crate) fn new<T: CertVerifier + Send + Sync + 'static>(verifier: T) -> Self {
        Self {
            inner: Box::new(verifier),
        }
    }
}

impl CertVerifier for DefaultCertVerifier {
    fn verify(&self, certs: &ServerCerts) -> bool {
        self.inner.verify(certs)
    }
}
