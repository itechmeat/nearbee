//! NEAR AI Cloud client: the endpoint attestation reports behind
//! `nearbee_core::evidence::Attestor` (design 6 of the change
//! `repo-audit-and-tiers`, spec `run-evidence`).
//!
//! One report is `GET {base}/attestation/report?model=..&signing_algo=ecdsa&nonce=..`
//! with the bearer key, no redirects, a 2 MiB body cap, at most two
//! attempts (9 s each, 1 s apart, retried only on a timeout, a connect
//! error or a 5xx) inside a 20-second bound for the whole fetch. A 2xx
//! with a JSON object body is a report; anything else is a failure whose
//! text never holds the key. Reports are collected, never verified.
//!
//! [`CloudClient`] (module `client`, design 1 of the change
//! `doc-digest-and-paid-demo`) makes the model calls of Nearbee's own
//! commands: the exact-bytes completion, its signature, the privacy
//! classifier and a plain JSON completion.

mod client;

pub use client::{
    CLASSIFY_MODEL, ChatAnswer, CloudClient, CloudError, MAX_RESPONSE_BYTES, RawResponse,
    SignatureAnswer, Span, Usage, parse_chat, thinking_off,
};

use std::time::Duration;

use nearbee_core::Secret;
use nearbee_core::evidence::{Attestor, FETCH_BOUND};

/// The production API base (design 6.1).
pub const PRODUCTION_BASE: &str = "https://cloud-api.near.ai/v1";

/// The largest report body read, in bytes.
pub const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

/// The limit of one attempt: the request and the whole body.
pub const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(9);

/// The wait before the second attempt.
pub const RETRY_WAIT: Duration = Duration::from_secs(1);

/// Attempts per fetch.
pub const ATTEMPTS: usize = 2;

/// The reason a fetch fails with when no key is configured.
pub const NO_KEY: &str = "NEARAI_API_KEY is not set";

/// The NEAR AI Cloud attestation client.
pub struct CloudAttestor {
    base: String,
    key: Secret,
    provider: Option<String>,
    client: reqwest::Client,
}

impl std::fmt::Debug for CloudAttestor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloudAttestor")
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

/// What one attempt ended with.
enum Attempt {
    Report(Vec<u8>),
    /// A timeout, a connect error or a 5xx: worth one more attempt.
    Retry(String),
    /// Anything else: the fetch fails now.
    Fail(String),
}

impl CloudAttestor {
    /// A client for `base_url` (production: [`PRODUCTION_BASE`]) with the
    /// configured key. Redirects are never followed and no system proxy
    /// is used.
    pub fn new(base_url: impl Into<String>, key: Secret) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .user_agent("nearbee")
            .build()
            .map_err(|e| format!("the HTTP client cannot be built: {e}"))?;
        Ok(Self {
            base: base_url.into().trim_end_matches('/').to_owned(),
            key,
            provider: None,
            client,
        })
    }

    /// Asks for the reports of one provider only (`provider=near`: the
    /// NEAR model attestations a `provider_tee` signature is matched
    /// against, design 1.3).
    pub fn with_provider(mut self, provider: &str) -> Self {
        self.provider = Some(provider.to_owned());
        self
    }

    async fn attempt(&self, model: &str, nonce: &str) -> Attempt {
        let url = format!("{}/attestation/report", self.base);
        let mut query = vec![
            ("model", model),
            ("signing_algo", "ecdsa"),
            ("nonce", nonce),
        ];
        if let Some(p) = &self.provider {
            query.push(("provider", p));
        }
        let sent = self
            .client
            .get(&url)
            .query(&query)
            .bearer_auth(self.key.expose())
            .send()
            .await;
        let mut resp = match sent {
            Ok(r) => r,
            Err(e) if e.is_timeout() || e.is_connect() => {
                return Attempt::Retry(format!("the request failed: {}", e.without_url()));
            }
            Err(e) => return Attempt::Fail(format!("the request failed: {}", e.without_url())),
        };
        let status = resp.status();
        if status.is_server_error() {
            return Attempt::Retry(format!("status {}", status.as_u16()));
        }
        if !status.is_success() {
            return Attempt::Fail(format!("status {}", status.as_u16()));
        }
        if resp
            .content_length()
            .is_some_and(|n| n > MAX_BODY_BYTES as u64)
        {
            return Attempt::Fail("the report exceeds 2 MiB".to_owned());
        }
        let mut body: Vec<u8> = Vec::new();
        loop {
            match resp.chunk().await {
                Ok(Some(chunk)) => {
                    if body.len() + chunk.len() > MAX_BODY_BYTES {
                        return Attempt::Fail("the report exceeds 2 MiB".to_owned());
                    }
                    body.extend_from_slice(&chunk);
                }
                Ok(None) => break,
                Err(e) if e.is_timeout() => {
                    return Attempt::Retry(format!("the body read failed: {}", e.without_url()));
                }
                Err(e) => {
                    return Attempt::Fail(format!("the body read failed: {}", e.without_url()));
                }
            }
        }
        match serde_json::from_slice::<serde_json::Value>(&body) {
            Ok(v) if v.is_object() => Attempt::Report(body),
            Ok(_) => Attempt::Fail("the report is not a JSON object".to_owned()),
            Err(_) => Attempt::Fail("the report is not JSON".to_owned()),
        }
    }

    async fn fetch(&self, model: &str, nonce: &str) -> Result<Vec<u8>, String> {
        let mut last = String::new();
        for n in 0..ATTEMPTS {
            if n > 0 {
                tokio::time::sleep(RETRY_WAIT).await;
            }
            match tokio::time::timeout(ATTEMPT_TIMEOUT, self.attempt(model, nonce)).await {
                Ok(Attempt::Report(body)) => return Ok(body),
                Ok(Attempt::Fail(reason)) => return Err(reason),
                Ok(Attempt::Retry(reason)) => last = reason,
                Err(_) => last = "the attempt timed out".to_owned(),
            }
        }
        Err(last)
    }
}

impl Attestor for CloudAttestor {
    async fn report(&self, model: &str, nonce: &str) -> Result<Vec<u8>, String> {
        if self.key.expose().is_empty() {
            return Err(NO_KEY.to_owned());
        }
        let outcome = match tokio::time::timeout(FETCH_BOUND, self.fetch(model, nonce)).await {
            Ok(r) => r,
            Err(_) => Err("the fetch ran past its 20-second bound".to_owned()),
        };
        outcome.map_err(|reason| {
            nearbee_core::redact::redact_detail(
                &format!("attestation report for {model}: {reason}"),
                std::slice::from_ref(&self.key),
            )
        })
    }
}
