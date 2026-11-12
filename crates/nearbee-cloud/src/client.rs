//! The model calls of Nearbee's own commands (design 1 of the change
//! `doc-digest-and-paid-demo`, spec `signed-call`).
//!
//! The client follows no redirect, uses no system proxy and caps every
//! body it reads. A failure names the step, the status or the field,
//! never the key and never response content: answers are parsed field by
//! field with fixed messages, because a serde error quotes the offending
//! value (review round, Opus 4).
//!
//! Every completion and classify call sends `x-no-aliasing: true`, and
//! [`CloudClient::chat_json`] and [`CloudClient::classify`] refuse an
//! answer that names another model than the one requested, so the model
//! the evidence names is the model that served the call.

use std::time::Duration;

use nearbee_core::Secret;
use serde::{Deserialize, Serialize};

/// The limit of one signature fetch.
pub const SIGNATURE_TIMEOUT: Duration = Duration::from_secs(15);

/// The limit of one classify attempt.
pub const CLASSIFY_TIMEOUT: Duration = Duration::from_secs(60);

/// The limit of one JSON completion.
pub const CHAT_JSON_TIMEOUT: Duration = Duration::from_secs(60);

/// The largest completion or classify body read, in bytes.
pub const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

/// The largest signature body read, in bytes.
pub const MAX_SIGNATURE_BYTES: usize = 64 * 1024;

/// The request key that turns a reasoning model's thinking off:
/// `"chat_template_kwargs": {"thinking": false}`. DeepSeek V4.1 Flash
/// otherwise reasons past the signed call's bound without answering
/// (measured 2026-11-12, change `model-thinking-off`).
pub fn thinking_off() -> serde_json::Value {
    serde_json::json!({"thinking": false})
}

/// The privacy classifier model.
pub const CLASSIFY_MODEL: &str = "openai/privacy-filter";

/// The answer of an exact-bytes completion, whatever its status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawResponse {
    pub status: u16,
    /// The `Content-Encoding` header, when the answer carries one.
    pub content_encoding: Option<String>,
    /// The body bytes as received (no decoding).
    pub bytes: Vec<u8>,
}

/// A response signature (`GET /v1/signature/{id}`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SignatureAnswer {
    pub text: String,
    pub signature: String,
    pub signing_address: String,
    pub signing_algo: String,
    /// `provider_tee` or `gateway`; absent on legacy signatures.
    #[serde(default)]
    pub signature_kind: Option<String>,
}

/// One classified span, without its text: the text is dropped while the
/// answer is parsed and never kept.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Span {
    pub category: String,
    pub start: u64,
    pub end: u64,
}

/// The token usage a completion reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

/// The first choice of a JSON completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatAnswer {
    /// The answer's `model`, when it carries one.
    pub model: Option<String>,
    pub content: String,
    pub finish_reason: String,
    pub usage: Option<Usage>,
}

/// Why a call failed. The text never holds the key or a response body.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CloudError {
    /// The signature is not recorded yet (a 404, or a 2xx carrying
    /// `error_code`): retried by the caller.
    #[error("the signature is not available yet")]
    NotYet,
    #[error("status {0}")]
    Status(u16),
    #[error("{0}")]
    TooLarge(String),
    #[error("{0}")]
    Transport(String),
    #[error("the answer does not parse: {0}")]
    Invalid(String),
}

impl CloudError {
    fn retryable(&self) -> bool {
        match self {
            CloudError::Status(s) => *s >= 500,
            CloudError::Transport(_) => true,
            _ => false,
        }
    }
}

/// The NEAR AI Cloud client of Nearbee's commands.
pub struct CloudClient {
    base: String,
    key: Secret,
    client: reqwest::Client,
}

impl std::fmt::Debug for CloudClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloudClient")
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

impl CloudClient {
    /// A client for `base_url` (the commands pass
    /// [`crate::PRODUCTION_BASE`]; tests a local server).
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
            client,
        })
    }

    fn transport(&self, what: &str, e: reqwest::Error) -> CloudError {
        CloudError::Transport(nearbee_core::redact::redact_detail(
            &format!("{what} failed: {}", e.without_url()),
            std::slice::from_ref(&self.key),
        ))
    }

    async fn read_capped(
        &self,
        mut resp: reqwest::Response,
        cap: usize,
        cap_text: &str,
    ) -> Result<Vec<u8>, CloudError> {
        let too_large = || CloudError::TooLarge(format!("the answer exceeds {cap_text}"));
        if resp.content_length().is_some_and(|n| n > cap as u64) {
            return Err(too_large());
        }
        let mut body = Vec::new();
        loop {
            match resp.chunk().await {
                Ok(Some(chunk)) => {
                    if body.len() + chunk.len() > cap {
                        return Err(too_large());
                    }
                    body.extend_from_slice(&chunk);
                }
                Ok(None) => return Ok(body),
                Err(e) => return Err(self.transport("the body read", e)),
            }
        }
    }

    async fn bounded<T>(
        limit: Duration,
        fut: impl std::future::Future<Output = Result<T, CloudError>>,
    ) -> Result<T, CloudError> {
        match tokio::time::timeout(limit, fut).await {
            Ok(r) => r,
            Err(_) => Err(CloudError::Transport(format!(
                "the call timed out after {} s",
                limit.as_secs()
            ))),
        }
    }

    /// `POST {base}/chat/completions` with `body` sent unchanged and the
    /// headers of a `provider_tee` signature (`x-no-aliasing: true`,
    /// `accept-encoding: identity`). Every status is returned; no retry.
    /// The request and the whole body are bounded by `limit` alone: the
    /// caller passes the time left in its invocation (spec `signed-call`).
    pub async fn chat_exact(
        &self,
        body: &[u8],
        limit: Duration,
    ) -> Result<RawResponse, CloudError> {
        Self::bounded(limit, async {
            let resp = self
                .client
                .post(format!("{}/chat/completions", self.base))
                .header("content-type", "application/json")
                .header("x-no-aliasing", "true")
                .header("accept-encoding", "identity")
                .bearer_auth(self.key.expose())
                .body(body.to_vec())
                .send()
                .await
                .map_err(|e| self.transport("the completion", e))?;
            let status = resp.status().as_u16();
            let content_encoding = resp
                .headers()
                .get(reqwest::header::CONTENT_ENCODING)
                .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned());
            let bytes = self.read_capped(resp, MAX_RESPONSE_BYTES, "4 MiB").await?;
            Ok(RawResponse {
                status,
                content_encoding,
                bytes,
            })
        })
        .await
    }

    /// `GET {base}/signature/{id}?model=..&signing_algo=ecdsa`. A 404 or a
    /// 2xx carrying `error_code` is [`CloudError::NotYet`].
    pub async fn signature(&self, id: &str, model: &str) -> Result<SignatureAnswer, CloudError> {
        Self::bounded(SIGNATURE_TIMEOUT, async {
            let resp = self
                .client
                .get(format!("{}/signature/{id}", self.base))
                .query(&[("model", model), ("signing_algo", "ecdsa")])
                .header("accept", "application/json")
                .bearer_auth(self.key.expose())
                .send()
                .await
                .map_err(|e| self.transport("the signature fetch", e))?;
            let status = resp.status().as_u16();
            if status == 404 {
                return Err(CloudError::NotYet);
            }
            if !(200..300).contains(&status) {
                return Err(CloudError::Status(status));
            }
            let body = self
                .read_capped(resp, MAX_SIGNATURE_BYTES, "64 KiB")
                .await?;
            let v: serde_json::Value = serde_json::from_slice(&body)
                .map_err(|_| CloudError::Invalid("the signature is not JSON".into()))?;
            if v.get("error_code").is_some() {
                return Err(CloudError::NotYet);
            }
            parse_signature(&v)
        })
        .await
    }

    /// `POST {base}/privacy/classify` for one text: the spans without
    /// their text. One retry on a 5xx or a transport failure.
    pub async fn classify(&self, text: &str) -> Result<Vec<Span>, CloudError> {
        let body = serde_json::json!({"model": CLASSIFY_MODEL, "input": text});
        // The model is fixed, so its check is part of every attempt.
        let mut last = None;
        for _ in 0..2 {
            match Self::bounded(CLASSIFY_TIMEOUT, self.classify_once(&body)).await {
                Ok(spans) => return Ok(spans),
                Err(e) if e.retryable() => last = Some(e),
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| CloudError::Transport("classify failed".into())))
    }

    async fn classify_once(&self, body: &serde_json::Value) -> Result<Vec<Span>, CloudError> {
        let resp = self
            .client
            .post(format!("{}/privacy/classify", self.base))
            .header("x-no-aliasing", "true")
            .bearer_auth(self.key.expose())
            .json(body)
            .send()
            .await
            .map_err(|e| self.transport("classify", e))?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(CloudError::Status(status));
        }
        let bytes = self.read_capped(resp, MAX_RESPONSE_BYTES, "4 MiB").await?;
        parse_classify(&bytes)
    }

    /// A normal non-streamed completion: the first choice's content and
    /// `finish_reason`, sent with `x-no-aliasing: true`. The answer must
    /// name the requested model. No retry (the caller retries a page
    /// once).
    pub async fn chat_json(&self, body: &serde_json::Value) -> Result<ChatAnswer, CloudError> {
        let requested = body.get("model").and_then(serde_json::Value::as_str);
        Self::bounded(CHAT_JSON_TIMEOUT, async {
            let resp = self
                .client
                .post(format!("{}/chat/completions", self.base))
                .header("x-no-aliasing", "true")
                .bearer_auth(self.key.expose())
                .json(body)
                .send()
                .await
                .map_err(|e| self.transport("the completion", e))?;
            let status = resp.status().as_u16();
            if !(200..300).contains(&status) {
                return Err(CloudError::Status(status));
            }
            let bytes = self.read_capped(resp, MAX_RESPONSE_BYTES, "4 MiB").await?;
            let answer = parse_chat(&bytes)?;
            if requested.is_none() || answer.model.as_deref() != requested {
                return Err(CloudError::Invalid(
                    "the completion answer names another model than the one requested".into(),
                ));
            }
            Ok(answer)
        })
        .await
    }
}

fn invalid(what: &str, field: &str) -> CloudError {
    CloudError::Invalid(format!("the {what} answer has no valid {field}"))
}

fn str_field(v: &serde_json::Value, what: &str, field: &str) -> Result<String, CloudError> {
    v.get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| invalid(what, field))
}

fn u64_field(v: &serde_json::Value, what: &str, field: &str) -> Result<u64, CloudError> {
    v.get(field)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| invalid(what, field))
}

fn parse_signature(v: &serde_json::Value) -> Result<SignatureAnswer, CloudError> {
    const W: &str = "signature";
    let signature_kind = match v.get("signature_kind") {
        None | Some(serde_json::Value::Null) => None,
        Some(k) => Some(
            k.as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid(W, "signature_kind"))?,
        ),
    };
    Ok(SignatureAnswer {
        text: str_field(v, W, "text")?,
        signature: str_field(v, W, "signature")?,
        signing_address: str_field(v, W, "signing_address")?,
        signing_algo: str_field(v, W, "signing_algo")?,
        signature_kind,
    })
}

/// Parses a classify answer: the spans without their text, refusing an
/// answer that names another model than [`CLASSIFY_MODEL`].
fn parse_classify(bytes: &[u8]) -> Result<Vec<Span>, CloudError> {
    const W: &str = "classify";
    let v: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| CloudError::Invalid("the classify answer is not JSON".into()))?;
    if v.get("model").and_then(serde_json::Value::as_str) != Some(CLASSIFY_MODEL) {
        return Err(CloudError::Invalid(
            "the classify answer names another model than the one requested".into(),
        ));
    }
    let data = v
        .get("data")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| invalid(W, "data"))?;
    let mut out = Vec::new();
    for item in data {
        let spans = item
            .get("spans")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| invalid(W, "spans"))?;
        for span in spans {
            out.push(Span {
                category: str_field(span, W, "category")?,
                start: u64_field(span, W, "start")?,
                end: u64_field(span, W, "end")?,
            });
        }
    }
    Ok(out)
}

/// Parses a non-streamed completion body into its first choice. An error
/// names the field, never the content.
pub fn parse_chat(bytes: &[u8]) -> Result<ChatAnswer, CloudError> {
    const W: &str = "completion";
    let v: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| CloudError::Invalid("the completion answer is not JSON".into()))?;
    let first = v
        .get("choices")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| invalid(W, "choices"))?
        .first()
        .ok_or_else(|| CloudError::Invalid("the answer has no choices".into()))?;
    let content = first
        .get("message")
        .ok_or_else(|| invalid(W, "message"))
        .and_then(|m| str_field(m, W, "content"))?;
    let finish_reason = str_field(first, W, "finish_reason")?;
    let usage = match v.get("usage") {
        None | Some(serde_json::Value::Null) => None,
        Some(u) => Some(Usage {
            prompt_tokens: u64_field(u, W, "prompt_tokens")?,
            completion_tokens: u64_field(u, W, "completion_tokens")?,
        }),
    };
    let model = match v.get("model") {
        None | Some(serde_json::Value::Null) => None,
        Some(m) => Some(
            m.as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid(W, "model"))?,
        ),
    };
    Ok(ChatAnswer {
        model,
        content,
        finish_reason,
        usage,
    })
}
