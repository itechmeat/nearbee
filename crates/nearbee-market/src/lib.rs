//! The market adapter: drives the NEAR AI Agent Market worker REST API
//! (`/v1`) with one agent token.
//!
//! Transport rules: no redirects (a 3xx on a write leaves the outcome
//! unknown), no system proxies, the HTTP library's own retries disabled,
//! the token only as `Authorization: Bearer` to the configured origin.
//! Per-call timeouts: 15 s reads, 30 s writes; connect timeout 5 s.
//!
//! Reads (`assignments`, public `agent`, pricing plans) are retried up to
//! three times on transport errors and 429, 502, 503, 504, honouring
//! `Retry-After`. Writes are never retried by the transport; the client
//! itself resends `start`, `submit` (identical body), a message (same
//! `clientRequestId`) and the file reserve/complete at most once, and only
//! when the first attempt's outcome is unknown. `decline` repeats at most
//! once, with a fresh-read check on `invalid_state_transition`;
//! `progress` repeats only after `409 progress_report_too_soon`, after the
//! given delay.
//!
//! Every error detail is bounded and redacted of tokens, capability links
//! and presigned-URL queries. The no-retry and no-redirect builder
//! settings are proven by review, not by a test: a test would have to
//! rebuild the client and would only prove reqwest.

use std::time::Duration;

pub mod scrub;
pub mod wire;

use nearbee_core::market::{
    AgentPatch, AgentRecord, DeclineOutcome, DeclineReason, DeclineRequest, Market, MarketError,
    Message, MessageRequest, NewPricingPlan, PricingPlan, ProgressOutcome, ProgressRequest,
    ReservedFile, Row, RowStatus, StoredFile, SubmitRequest, ThreadMessage, UploadFile,
};
use nearbee_core::redact::{DETAIL_LIMIT, redact_detail, truncate};
use nearbee_core::secret::{REDACTED, Secret};
use serde_json::Value;

/// How to reach the market.
#[derive(Debug, Clone)]
pub struct MarketOptions {
    /// Origin, default `https://market.near.ai`.
    pub base_url: String,
    /// The agent token (`aat_...`), sent only as a bearer header.
    pub token: Secret,
    /// Every secret value removed from error details (the token is added
    /// automatically).
    pub redact: Vec<Secret>,
    /// Timeouts and pacing.
    pub timing: MarketTiming,
}

/// Timeouts and pacing of the client.
#[derive(Debug, Clone)]
pub struct MarketTiming {
    pub connect: Duration,
    pub read: Duration,
    pub write: Duration,
    /// Delay before each retry of a read without a usable `Retry-After`;
    /// its length is the number of retries.
    pub read_retry_delays: Vec<Duration>,
    /// Delay before the one resend of an unknown write.
    pub write_resend_delay: Duration,
    /// Ceiling on any wait a server names or a retry takes.
    pub retry_after_ceiling: Duration,
    /// Total time budget of one read call, retries included (spec
    /// `market-client`, Read retries: 30 s per call).
    pub read_budget: Duration,
}

impl Default for MarketTiming {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(5),
            read: Duration::from_secs(15),
            write: Duration::from_secs(30),
            read_retry_delays: vec![
                Duration::from_millis(500),
                Duration::from_secs(1),
                Duration::from_secs(2),
            ],
            write_resend_delay: Duration::from_millis(200),
            retry_after_ceiling: Duration::from_secs(120),
            read_budget: Duration::from_secs(30),
        }
    }
}

impl MarketOptions {
    pub fn new(base_url: impl Into<String>, token: Secret) -> Self {
        Self {
            base_url: base_url.into(),
            token,
            redact: Vec::new(),
            timing: MarketTiming::default(),
        }
    }
}

/// The client for one agent token.
#[derive(Debug, Clone)]
pub struct NearAgentMarket {
    http: reqwest::Client,
    base: String,
    token: Secret,
    secrets: Vec<Secret>,
    timing: MarketTiming,
}

/// Page size of the assignments listing (the API's documented maximum).
const PAGE_LIMIT: usize = 100;

/// The market's machine code from a JSON refusal body.
fn machine_code(status: u16, text: &str) -> String {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_else(|| format!("http_{status}"))
}

/// Seconds to wait, from a refusal body's `retry_after` field.
fn retry_after_from_body(text: &str) -> Option<u64> {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| v.get("retry_after").and_then(Value::as_u64))
}

fn describe(e: &reqwest::Error) -> String {
    let what = if e.is_timeout() {
        "timed out"
    } else if e.is_connect() {
        "connection failed"
    } else if e.is_body() || e.is_decode() {
        "reading the answer failed"
    } else {
        "request failed"
    };
    format!("{what}: {e}")
}

/// An id used as one URL path segment.
fn segment(id: &str) -> Result<&str, MarketError> {
    let ok = !id.is_empty()
        && id != "."
        && id != ".."
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    if ok {
        Ok(id)
    } else {
        Err(MarketError::Protocol {
            detail: format!("`{id}` is not a valid market id"),
        })
    }
}

/// Serializes a typed wire request; serialization cannot fail for these
/// types, so a failure is a programming error surfaced as a protocol
/// error.
fn wire_body<T: serde::Serialize>(body: T) -> Result<Value, MarketError> {
    serde_json::to_value(body).map_err(|e| MarketError::Protocol {
        detail: format!("request body: {e}"),
    })
}

/// An `int32` wire field from a Nearbee `u64`.
fn int32(v: u64) -> Result<i32, MarketError> {
    i32::try_from(v).map_err(|_| MarketError::Protocol {
        detail: format!("{v} does not fit the market's int32 field"),
    })
}

/// The agent fields the three agent view components share.
struct AgentFields {
    handle: String,
    name: String,
    listing_status: wire::ListingStatus,
}

/// Removes market tokens, `/files/<token>/` capability links and presigned
/// URL queries, then applies the generic redaction and bound.
pub fn redact_market_detail(text: &str, secrets: &[Secret]) -> String {
    let links = redact_capability_links(text);
    let queries = redact_presigned_queries(&links);
    redact_detail(&queries, secrets)
}

/// Replaces every non-empty segment after `/files/` with `<redacted>`.
fn redact_capability_links(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find("/files/") {
        let start = pos + "/files/".len();
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let end = tail
            .find(|c: char| c == '/' || c == '?' || c == '#' || c.is_whitespace() || c == '"')
            .unwrap_or(tail.len());
        if end > 0 {
            out.push_str(REDACTED);
            rest = &tail[end..];
        } else {
            rest = tail;
        }
    }
    out.push_str(rest);
    out
}

/// Strips the query part (and everything after it) of any `https://` URL in
/// free text, so a presigned URL never survives in a detail.
fn redact_presigned_queries(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find("https://") {
        let after = &rest[pos..];
        let end = after
            .find(|c: char| c.is_whitespace() || c == '"' || c == ')')
            .unwrap_or(after.len());
        let url = &after[..end];
        match url.split_once('?') {
            Some((base, _)) => {
                out.push_str(&rest[..pos]);
                out.push_str(base);
                out.push_str("?<redacted>");
            }
            None => out.push_str(&rest[..pos + end]),
        }
        rest = &rest[pos + end..];
    }
    out.push_str(rest);
    out
}

/// Whether the URL's host is a loopback address or `localhost`. Parsed
/// with a real URL parser: bracketed IPv6 hosts and userinfo (which moves
/// the authority's host) are handled by the parser, not by string splits
/// (ocr2 11).
fn is_loopback_host(url: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(url) else {
        return false;
    };
    if !u.username().is_empty() || u.password().is_some() {
        // `http://localhost:secret@evil.example` is a call to evil.example.
        return false;
    }
    match u.host_str() {
        Some("localhost") => true,
        Some(h) => h
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback()),
        None => false,
    }
}

impl NearAgentMarket {
    /// The absolute URL of an attachment download path: a relative path is
    /// resolved against the configured origin; an absolute URL must be the
    /// same origin (scheme, host and port), without userinfo or fragment.
    fn attachment_url(&self, path: &str) -> Result<String, MarketError> {
        let base = reqwest::Url::parse(&self.base).map_err(|_| MarketError::Protocol {
            detail: "the configured market origin does not parse".into(),
        })?;
        let url = if path.starts_with("http://") || path.starts_with("https://") {
            reqwest::Url::parse(path).map_err(|_| MarketError::Protocol {
                detail: "the download url does not parse".into(),
            })?
        } else {
            if !path.starts_with('/') {
                return Err(MarketError::Refused {
                    status: 0,
                    code: "foreign_origin".into(),
                    detail: "the download path is neither the origin's url nor a path".into(),
                });
            }
            base.join(path).map_err(|_| MarketError::Protocol {
                detail: "the download path does not join the origin".into(),
            })?
        };
        let same_origin = url.scheme() == base.scheme()
            && url.host() == base.host()
            && url.port_or_known_default() == base.port_or_known_default();
        let clean =
            url.username().is_empty() && url.password().is_none() && !url.fragment().is_some();
        if !same_origin || !clean {
            return Err(MarketError::Refused {
                status: 0,
                code: "foreign_origin".into(),
                detail: "the download url is not on the market origin, or carries userinfo or a fragment".into(),
            });
        }
        Ok(url.to_string())
    }
}

/// Maps a wire `ThreadMessageView` onto Nearbee's message model.
fn thread_message(m: wire::ThreadMessageView) -> ThreadMessage {
    ThreadMessage {
        message_id: m.message_id,
        body: m.body,
        sender_agent_id: m.sender_agent_id,
        attachments: m.attachments.into_iter().map(attachment_from).collect(),
        created_at: wire::wire_time(&m.created_at),
    }
}

/// Maps a wire `AttachmentBrief` onto Nearbee's attachment model.
fn attachment_from(a: wire::AttachmentBrief) -> nearbee_core::market::Attachment {
    nearbee_core::market::Attachment {
        id: a.id,
        filename: a.filename,
        content_type: a.content_type,
        byte_size: a.byte_size,
        sha256: a.sha256,
        download_url: a.download_url,
    }
}

impl NearAgentMarket {
    pub fn new(opts: MarketOptions) -> Result<Self, MarketError> {
        let mut secrets = opts.redact;
        secrets.push(opts.token.clone());
        let base = opts.base_url.trim_end_matches('/').to_owned();
        if !(base.starts_with("http://") || base.starts_with("https://")) {
            return Err(MarketError::Protocol {
                detail: format!("market URL must start with http:// or https://: {base}"),
            });
        }
        if base.starts_with("http://") && !is_loopback_host(&base) {
            // The bearer token must not travel in cleartext.
            return Err(MarketError::Protocol {
                detail: format!(
                    "market URL must be https:// (http:// is allowed only for a loopback host): {base}"
                ),
            });
        }
        let http = reqwest::Client::builder()
            .retry(reqwest::retry::never())
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(opts.timing.connect)
            .build()
            .map_err(|e| MarketError::Protocol {
                detail: redact_detail(&e.to_string(), &secrets),
            })?;
        Ok(Self {
            http,
            base,
            token: opts.token,
            secrets,
            timing: opts.timing,
        })
    }

    fn redact(&self, text: &str) -> String {
        redact_market_detail(text, &self.secrets)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, self.url(path))
            .bearer_auth(self.token.expose())
    }

    /// One request attempt: the answer's status and body, or the transport
    /// failure text.
    async fn attempt(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&Value>,
        timeout: Duration,
    ) -> Result<Result<(u16, String, Option<u64>), String>, MarketError> {
        let mut rb = self
            .request(method.clone(), path)
            .query(query)
            .timeout(timeout);
        if let Some(b) = body {
            rb = rb.json(b);
        }
        match rb.send().await {
            Err(e) => Ok(Err(format!("{} {path}: {}", method.as_str(), describe(&e)))),
            Ok(resp) => {
                let status = resp.status().as_u16();
                let retry_after = resp
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse::<u64>().ok());
                match resp.text().await {
                    Ok(text) => Ok(Ok((status, text, retry_after))),
                    Err(e) => Ok(Err(format!(
                        "{} {path}: HTTP {status}: {}",
                        method.as_str(),
                        describe(&e)
                    ))),
                }
            }
        }
    }

    /// A `GET` with up to three retries on transport errors and 429, 502,
    /// 503, 504, within the read time budget: a `Retry-After` that would
    /// pass it ends the call at once as a retryable read (spec
    /// `market-client`, Read retries).
    async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value, MarketError> {
        let mut delays = self.timing.read_retry_delays.iter().copied();
        // One absolute deadline for the whole call: every attempt gets at
        // most the time left, and no wait may pass it (C15).
        let deadline = std::time::Instant::now() + self.timing.read_budget;
        let mut last: Option<(String, Option<u64>)> = None;
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                let (detail, named) = last.unwrap_or_else(|| {
                    (format!("GET {path}: the read time budget is spent"), None)
                });
                return Err(MarketError::RetryableRead {
                    detail: truncate(&self.redact(&detail), DETAIL_LIMIT),
                    retry_after_secs: named,
                });
            }
            let outcome = self
                .attempt(
                    reqwest::Method::GET,
                    path,
                    query,
                    None,
                    self.timing.read.min(left),
                )
                .await?;
            let retryable: Option<(String, Option<u64>)> = match outcome {
                Err(transport) => Some((format!("GET {path}: {transport}"), None)),
                Ok((status, text, retry_after)) if matches!(status, 429 | 502 | 503 | 504) => {
                    let from_body = retry_after.or_else(|| retry_after_from_body(&text));
                    Some((
                        format!("GET {path}: HTTP {status}: {}", text.trim()),
                        from_body,
                    ))
                }
                Ok((status, text, _)) => return self.read_answer(path, status, &text),
            };
            let Some((detail, named)) = retryable else {
                unreachable!()
            };
            last = Some((detail.clone(), named));
            let mut named_secs = named;
            // The budget: a named wait that would pass the total ends the
            // call now, with the delay for the caller to schedule.
            if let Some(secs) = named_secs {
                let wait = Duration::from_secs(secs).min(self.timing.retry_after_ceiling);
                if std::time::Instant::now() + wait > deadline {
                    return Err(MarketError::RetryableRead {
                        detail: truncate(&self.redact(&detail), DETAIL_LIMIT),
                        retry_after_secs: Some(secs),
                    });
                }
                named_secs = Some(secs);
            }
            match delays.next() {
                Some(delay) => {
                    let wait = named_secs
                        .map(Duration::from_secs)
                        .unwrap_or(delay)
                        .min(self.timing.retry_after_ceiling);
                    // No retry starts when its wait would pass the budget.
                    if std::time::Instant::now() + wait > deadline {
                        return Err(MarketError::RetryableRead {
                            detail: truncate(&self.redact(&detail), DETAIL_LIMIT),
                            retry_after_secs: named_secs,
                        });
                    }
                    tracing::debug!(detail = %self.redact(&detail), "retrying market read");
                    tokio::time::sleep(wait).await;
                }
                None => {
                    return Err(MarketError::RetryableRead {
                        detail: truncate(&self.redact(&detail), DETAIL_LIMIT),
                        retry_after_secs: named_secs,
                    });
                }
            }
        }
    }

    /// Maps a read answer whose body was read.
    fn read_answer(&self, path: &str, status: u16, text: &str) -> Result<Value, MarketError> {
        if status == 401 {
            return Err(MarketError::Unauthorized {
                detail: self.redact(&format!("GET {path}: {}", text.trim())),
            });
        }
        if !(200..300).contains(&status) {
            return Err(MarketError::Refused {
                status,
                code: machine_code(status, text),
                detail: self.redact(&format!("GET {path}: {}", text.trim())),
            });
        }
        serde_json::from_str(text).map_err(|e| MarketError::Protocol {
            detail: self.redact(&format!("GET {path}: not JSON: {e}")),
        })
    }

    /// One attempt of a write. 401 is unauthorized; 4xx refused; a 3xx, a
    /// 5xx, a transport failure and a success whose body is not JSON leave
    /// the outcome unknown.
    async fn write_once(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Option<Value>, MarketError> {
        let outcome = self
            .attempt(method.clone(), path, &[], body, self.timing.write)
            .await?;
        let (status, text, _retry_after) = match outcome {
            Err(transport) => {
                return Err(MarketError::WriteOutcomeUnknown {
                    detail: self.redact(&format!("{} {path}: {transport}", method.as_str())),
                });
            }
            Ok(pair) => pair,
        };
        let prefix = format!("{} {path}", method.as_str());
        if status == 401 {
            return Err(MarketError::Unauthorized {
                detail: self.redact(&format!("{prefix}: {}", text.trim())),
            });
        }
        if (200..300).contains(&status) {
            if text.trim().is_empty() {
                return Ok(None);
            }
            return match serde_json::from_str::<Value>(&text) {
                Ok(v) => Ok(Some(v)),
                Err(e) => Err(MarketError::WriteOutcomeUnknown {
                    detail: self.redact(&format!(
                        "{prefix}: HTTP {status} with a body that is not JSON: {e}"
                    )),
                }),
            };
        }
        if status >= 500 || (300..400).contains(&status) {
            return Err(MarketError::WriteOutcomeUnknown {
                detail: self.redact(&format!(
                    "{prefix}: unexpected HTTP {status}: {}",
                    text.trim()
                )),
            });
        }
        Err(MarketError::Refused {
            status,
            code: machine_code(status, &text),
            detail: self.redact(&format!("{prefix}: {}", text.trim())),
        })
    }

    /// Sends a write and resends it at most once, and only when the first
    /// attempt's outcome is unknown; a still-unknown outcome surfaces after
    /// exactly two requests.
    async fn write_with_resend(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Option<Value>, MarketError> {
        match self.write_once(method.clone(), path, body.as_ref()).await {
            Err(MarketError::WriteOutcomeUnknown { .. }) => {
                tokio::time::sleep(self.timing.write_resend_delay).await;
                self.write_once(method, path, body.as_ref()).await
            }
            other => other,
        }
    }

    /// Maps one `MyAssignmentItem` onto Nearbee's own row model. The
    /// core `Row` is the model; the wire item is the API's envelope
    /// (`assignment`, `job`, `latestMessage`, `autoAcceptAt`).
    fn to_row(item: wire::MyAssignmentItem, path: &str) -> Result<Row, MarketError> {
        let protocol = |detail: String| MarketError::Protocol {
            detail: format!("GET {path}: {detail}"),
        };
        let dt = |s: &str| wire::wire_time(s).ok_or_else(|| protocol(format!("bad timestamp {s}")));
        let a = item.assignment;
        let secs = |v: i32| u64::try_from(v).ok();
        Ok(Row {
            id: a.assignment_id,
            status: Some(wire::wire_str(&a.status)),
            started_at: a.started_at.as_deref().map(dt).transpose()?,
            submitted_at: a.submitted_at.as_deref().map(dt).transpose()?,
            deliverable_url: a.deliverable_url,
            sla_deadline_at: Some(dt(&a.sla_deadline_at)?),
            auto_accept_at: item.auto_accept_at.as_deref().map(dt).transpose()?,
            cancel_reason: a.cancel_reason,
            created_at: dt(&a.created_at).ok(),
            updated_at: dt(&a.updated_at).ok(),
            finalized_at: a.finalized_at.as_deref().and_then(|t| dt(t).ok()),
            sla_seconds: secs(a.sla_seconds),
            escrow_amount: Some(a.escrow_amount),
            escrow_token: Some(a.escrow_token),
            job_id: Some(item.job.job_id),
            title: Some(item.job.title),
            // The buyer's brief: data, never instructions to Nearbee.
            description: Some(item.job.description),
            job_attachments: item
                .job
                .attachments
                .into_iter()
                .map(attachment_from)
                .collect(),
            latest_message: item.latest_message.map(|m| Message {
                body: m.body,
                sender_side: Some(m.sender_side),
                created_at: wire::wire_time(&m.created_at),
                truncated: m.truncated,
                attachments: m.attachments.into_iter().map(attachment_from).collect(),
            }),
        })
    }

    fn parse_rows(&self, v: Value, path: &str) -> Result<Vec<Row>, MarketError> {
        let resp: wire::MyAssignmentsResponse =
            serde_json::from_value(v).map_err(|e| MarketError::Protocol {
                detail: self.redact(&format!("GET {path}: {e}")),
            })?;
        resp.assignments
            .into_iter()
            .map(|i| Self::to_row(i, path))
            .collect()
    }

    /// Maps the wire `PricingPlan` component onto Nearbee's model type.
    fn to_plan(v: wire::PricingPlan) -> PricingPlan {
        PricingPlan {
            pricing_plan_id: v.pricing_plan_id,
            model: Some(wire::wire_str(&v.model)),
            price_amount: v.price_amount,
            sla_seconds: v.sla_seconds.and_then(|s| u64::try_from(s).ok()),
            is_default: v.is_default,
            listing_status: Some(wire::wire_str(&v.listing_status)),
        }
    }

    /// The Nearbee agent record from any of the agent view components
    /// (they share the snake_case names `agent_id` and `listing_status`).
    fn to_agent(agent_id: String, v: AgentFields) -> AgentRecord {
        AgentRecord {
            agent_id,
            handle: Some(v.handle),
            name: Some(v.name),
            listing_status: Some(wire::wire_str(&v.listing_status)),
        }
    }

    fn parse_plan(v: Value, origin: &str) -> Result<PricingPlan, MarketError> {
        let w: wire::PricingPlan =
            serde_json::from_value(v).map_err(|e| MarketError::Protocol {
                detail: format!("{origin}: {e}"),
            })?;
        Ok(Self::to_plan(w))
    }

    /// The answer to a PATCH: always the owner view (`OwnerAgentView`).
    fn parse_owner_agent(v: Value, origin: &str) -> Result<AgentRecord, MarketError> {
        let o: wire::OwnerAgentView =
            serde_json::from_value(v).map_err(|e| MarketError::Protocol {
                detail: format!("{origin}: {e}"),
            })?;
        Ok(Self::to_agent(
            o.agent_id.clone(),
            AgentFields {
                handle: o.handle.clone(),
                name: o.name.clone(),
                listing_status: o.listing_status,
            },
        ))
    }

    fn parse_agent(v: Value, origin: &str) -> Result<AgentRecord, MarketError> {
        let view: wire::AgentView =
            serde_json::from_value(v).map_err(|e| MarketError::Protocol {
                detail: format!("{origin}: {e}"),
            })?;
        Ok(match view {
            wire::AgentView::Owner(o) => Self::to_agent(
                o.agent_id.clone(),
                AgentFields {
                    handle: o.handle.clone(),
                    name: o.name.clone(),
                    listing_status: o.listing_status,
                },
            ),
            wire::AgentView::Admin(a) => Self::to_agent(
                a.public.agent_id.clone(),
                AgentFields {
                    handle: a.public.handle.clone(),
                    name: a.public.name.clone(),
                    listing_status: a.public.listing_status,
                },
            ),
            wire::AgentView::Public(p) => Self::to_agent(
                p.agent_id.clone(),
                AgentFields {
                    handle: p.handle.clone(),
                    name: p.name.clone(),
                    listing_status: p.listing_status,
                },
            ),
        })
    }

    /// A fresh read of one assignment row, for the decline repeat check.
    /// The worker API has no `GET /v1/assignments/{id}`; the row is found
    /// in the paged `status=all` list.
    async fn assignment(&self, row_id: &str) -> Result<Row, MarketError> {
        let rows = Market::assignments(self).await?;
        rows.into_iter()
            .find(|r| r.id == row_id)
            .ok_or_else(|| MarketError::Protocol {
                detail: format!("assignment {row_id} is not in the status=all list"),
            })
    }
}

impl Market for NearAgentMarket {
    async fn assignments(&self) -> Result<Vec<Row>, MarketError> {
        let mut out = Vec::new();
        let mut offset = 0usize;
        // A ceiling so a server that ignores `offset` cannot loop forever.
        for _page in 0..100 {
            let path = "/v1/agents/me/assignments";
            let v = self
                .get(
                    path,
                    &[
                        ("status", "all"),
                        ("limit", &PAGE_LIMIT.to_string()),
                        ("offset", &offset.to_string()),
                    ],
                )
                .await?;
            let rows = self.parse_rows(v, path)?;
            let got = rows.len();
            out.extend(rows);
            if got < PAGE_LIMIT {
                return Ok(out);
            }
            offset += got;
        }
        Err(MarketError::Protocol {
            detail: "GET /v1/agents/me/assignments: more than 100 pages".into(),
        })
    }

    async fn start(&self, row_id: &str) -> Result<(), MarketError> {
        let path = format!("/v1/assignments/{}/start", segment(row_id)?);
        // The operation defines no request body; none is sent.
        self.write_with_resend(reqwest::Method::POST, &path, None)
            .await?;
        Ok(())
    }

    async fn message(&self, row_id: &str, req: &MessageRequest) -> Result<(), MarketError> {
        let path = format!("/v1/assignments/{}/messages", segment(row_id)?);
        let body = serde_json::to_value(wire::SendMessageRequest {
            body: req.body.clone(),
            client_request_id: Some(req.client_request_id.clone()),
        })
        .map_err(|e| MarketError::Protocol {
            detail: format!("message body: {e}"),
        })?;
        self.write_with_resend(reqwest::Method::POST, &path, Some(body))
            .await?;
        Ok(())
    }

    async fn progress(
        &self,
        row_id: &str,
        req: &ProgressRequest,
    ) -> Result<ProgressOutcome, MarketError> {
        let path = format!("/v1/assignments/{}/progress", segment(row_id)?);
        let body = wire_body(wire::ReportProgressRequest {
            message: req.message.clone(),
            // `int32`, 0 to 100.
            percent: req.percent.map(|p| i32::from(p.min(100))),
        })?;
        // The client never repeats a progress write: a too-soon refusal is
        // returned at once with its delay, for the caller to schedule
        // (spec `market-client`, Decline and progress repeats).
        let outcome = self
            .attempt(
                reqwest::Method::POST,
                &path,
                &[],
                Some(&body),
                self.timing.write,
            )
            .await?;
        let (status, text, retry_after) = match outcome {
            Err(transport) => {
                return Err(MarketError::WriteOutcomeUnknown {
                    detail: self.redact(&format!("POST {path}: {transport}")),
                });
            }
            Ok(pair) => pair,
        };
        if status == 401 {
            return Err(MarketError::Unauthorized {
                detail: self.redact(&format!("POST {path}: {}", text.trim())),
            });
        }
        if (200..300).contains(&status) {
            return Ok(ProgressOutcome::Reported);
        }
        if status == 409 && machine_code(status, &text) == "progress_report_too_soon" {
            let seconds = retry_after
                .or_else(|| retry_after_from_body(&text))
                .unwrap_or(30);
            return Ok(ProgressOutcome::TooSoon {
                retry_after_secs: seconds,
            });
        }
        if status >= 500 || (300..400).contains(&status) {
            return Err(MarketError::WriteOutcomeUnknown {
                detail: self.redact(&format!("POST {path}: unexpected HTTP {status}")),
            });
        }
        Err(MarketError::Refused {
            status,
            code: machine_code(status, &text),
            detail: self.redact(&format!("POST {path}: {}", text.trim())),
        })
    }

    async fn submit(&self, row_id: &str, req: &SubmitRequest) -> Result<(), MarketError> {
        let path = format!("/v1/assignments/{}/submit", segment(row_id)?);
        let body = wire_body(wire::SubmitRequest {
            deliverable_markdown: req.markdown.clone(),
            files: req.files.clone(),
        })?;
        self.write_with_resend(reqwest::Method::POST, &path, Some(body))
            .await?;
        Ok(())
    }

    async fn decline(
        &self,
        row_id: &str,
        req: &DeclineRequest,
    ) -> Result<DeclineOutcome, MarketError> {
        let path = format!("/v1/assignments/{}/decline", segment(row_id)?);
        let body = wire_body(wire::DeclineRequest {
            reason: Some(match req.reason {
                DeclineReason::Rejected => wire::WorkerCancelReason::Rejected,
                DeclineReason::Failed => wire::WorkerCancelReason::Failed,
            }),
            detail: req.detail.clone(),
        })?;
        let first = self
            .write_once(reqwest::Method::POST, &path, Some(&body))
            .await;
        match first {
            Ok(_) => Ok(DeclineOutcome::Done),
            Err(MarketError::WriteOutcomeUnknown { .. }) => {
                // One repeat. If that answers `invalid_state_transition`,
                // the decline landed earlier: count it as done only when a
                // fresh read shows the row cancelled.
                tokio::time::sleep(self.timing.write_resend_delay).await;
                match self
                    .write_once(reqwest::Method::POST, &path, Some(&body))
                    .await
                {
                    Ok(_) => Ok(DeclineOutcome::Done),
                    Err(MarketError::Refused { status, code, .. })
                        if status == 400 && code == "invalid_state_transition" =>
                    {
                        match self.assignment(row_id).await {
                            Ok(row) if row.status() == RowStatus::Cancelled => {
                                Ok(DeclineOutcome::Done)
                            }
                            Ok(_) => Ok(DeclineOutcome::Refused),
                            Err(e) => Err(e),
                        }
                    }
                    other => other.map(|_| DeclineOutcome::Done),
                }
            }
            Err(e) => Err(e),
        }
    }

    async fn agent(&self, handle: &str) -> Result<AgentRecord, MarketError> {
        let path = format!("/v1/agents/{}", segment(handle)?);
        let v = self.get(&path, &[]).await?;
        Self::parse_agent(v, &format!("GET {path}"))
    }

    async fn patch_agent(
        &self,
        agent_id: &str,
        patch: &AgentPatch,
    ) -> Result<AgentRecord, MarketError> {
        let path = format!("/v1/agents/{}", segment(agent_id)?);
        let category = patch
            .category
            .as_deref()
            .map(|c| {
                serde_json::from_value::<wire::Category>(Value::String(c.to_owned())).map_err(
                    |_| MarketError::Protocol {
                        detail: format!("`{c}` is not one of the market's categories"),
                    },
                )
            })
            .transpose()?;
        let body = wire_body(wire::PatchAgentRequest {
            name: patch.name.clone(),
            description: patch.description.clone(),
            bio_markdown: patch.bio_markdown.clone(),
            category,
            tags: patch.tags.clone(),
            sla_seconds: patch.sla_seconds.map(int32).transpose()?,
        })?;
        // `listing_status` is deliberately absent: publishing never
        // publishes.
        let path_owned = path.clone();
        match self
            .write_once(reqwest::Method::PATCH, &path_owned, Some(&body))
            .await?
        {
            Some(v) => Self::parse_owner_agent(v, &format!("PATCH {path_owned}")),
            None => self.agent(agent_id).await,
        }
    }

    async fn pricing_plans(&self, agent_id: &str) -> Result<Vec<PricingPlan>, MarketError> {
        let path = format!("/v1/agents/{}/pricing-plans", segment(agent_id)?);
        let v = self.get(&path, &[]).await?;
        let arr: Vec<wire::PricingPlan> =
            serde_json::from_value(v).map_err(|e| MarketError::Protocol {
                detail: format!("GET {path}: {e}"),
            })?;
        Ok(arr.into_iter().map(Self::to_plan).collect())
    }

    async fn create_pricing_plan(
        &self,
        agent_id: &str,
        plan: &NewPricingPlan,
    ) -> Result<PricingPlan, MarketError> {
        let path = format!("/v1/agents/{}/pricing-plans", segment(agent_id)?);
        let body = wire_body(wire::CreatePricingPlanRequest {
            model: wire::PricingModel::PerCall,
            price_amount: plan.price_usd.clone(),
            sla_seconds: Some(int32(plan.sla_seconds)?),
            display_name: plan.display_name.clone(),
        })?;
        let answer = self
            .write_once(reqwest::Method::POST, &path, Some(&body))
            .await?;
        let Some(v) = answer else {
            return Err(MarketError::Protocol {
                detail: format!("POST {path}: empty answer"),
            });
        };
        Self::parse_plan(v, &format!("POST {path}"))
    }

    async fn make_default(&self, agent_id: &str, plan_id: &str) -> Result<(), MarketError> {
        let _ = agent_id;
        let path = format!("/v1/pricing-plans/{}/make-default", segment(plan_id)?);
        // The operation defines no request body; none is sent.
        self.write_with_resend(reqwest::Method::POST, &path, None)
            .await?;
        Ok(())
    }

    async fn messages(
        &self,
        row_id: &str,
        until: Option<time::OffsetDateTime>,
    ) -> Result<Vec<ThreadMessage>, MarketError> {
        let base_path = format!("/v1/assignments/{}/messages", segment(row_id)?);
        let mut out = Vec::new();
        let mut offset = 0usize;
        // A ceiling so a server that ignores `offset` cannot loop forever.
        for _page in 0..100 {
            let path = base_path.clone();
            let v = self
                .get(
                    &path,
                    &[
                        ("limit", &PAGE_LIMIT.to_string()),
                        ("offset", &offset.to_string()),
                    ],
                )
                .await?;
            let resp: wire::ListMessagesResponse =
                serde_json::from_value(v).map_err(|e| MarketError::Protocol {
                    detail: self.redact(&format!("GET {path}: {e}")),
                })?;
            let got = resp.messages.len();
            for m in resp.messages {
                let created = wire::wire_time(&m.created_at);
                if let Some(until) = until
                    && let Some(t) = created
                    && t <= until
                {
                    // The stop page counts: this message is the last read.
                    out.push(thread_message(m));
                    return Ok(out);
                }
                out.push(thread_message(m));
            }
            if got < PAGE_LIMIT {
                return Ok(out);
            }
            offset += got;
        }
        Err(MarketError::Protocol {
            detail: "GET /v1/assignments/{id}/messages: more than 100 pages".into(),
        })
    }

    fn origin(&self) -> String {
        self.base.clone()
    }

    async fn download(
        &self,
        path: &str,
        cap: u64,
        out: &mut (dyn std::io::Write + Send),
    ) -> Result<u64, MarketError> {
        // The origin check again in the client (design 9.4): the bearer
        // never travels anywhere but the configured origin.
        let url = self.attachment_url(path)?;
        let resp = self
            .http
            .get(url)
            .bearer_auth(self.token.expose())
            .timeout(Duration::from_secs(120))
            .send()
            .await
            .map_err(|e| MarketError::RetryableRead {
                detail: self.redact(&format!("GET <attachment>: {}", describe(&e))),
                retry_after_secs: None,
            })?;
        let status = resp.status().as_u16();
        match status {
            401 => Err(MarketError::Unauthorized {
                detail: self.redact(&format!("GET <attachment>: HTTP {status}")),
            }),
            403 | 404 => Err(MarketError::Refused {
                status,
                code: machine_code(status, ""),
                detail: self.redact(&format!("GET <attachment>: HTTP {status}")),
            }),
            s if (300..400).contains(&s) => Err(MarketError::Protocol {
                detail: format!("GET <attachment>: HTTP {s} to a download is not followed"),
            }),
            503 => Err(MarketError::RetryableRead {
                detail: self.redact(&format!("GET <attachment>: HTTP {status}")),
                retry_after_secs: resp
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse::<u64>().ok()),
            }),
            s if !(200..300).contains(&s) => Err(MarketError::Refused {
                status: s,
                code: format!("http_{s}"),
                detail: self.redact(&format!("GET <attachment>: HTTP {s}")),
            }),
            _ => {
                let mut total: u64 = 0;
                let mut resp = resp;
                while let Some(chunk) =
                    resp.chunk().await.map_err(|e| MarketError::RetryableRead {
                        detail: self.redact(&format!("GET <attachment>: {}", describe(&e))),
                        retry_after_secs: None,
                    })?
                {
                    total += chunk.len() as u64;
                    if total > cap {
                        return Err(MarketError::Protocol {
                            detail: format!(
                                "GET <attachment>: the stream passed its {} byte cap",
                                cap
                            ),
                        });
                    }
                    out.write_all(&chunk).map_err(|e| MarketError::Protocol {
                        detail: format!("writing the attachment: {e}"),
                    })?;
                }
                Ok(total)
            }
        }
    }

    async fn reserve_file(&self, file: &UploadFile) -> Result<ReservedFile, MarketError> {
        // `filename` and `byteSize` are the component's required
        // properties; the signed URL carries the exact byte count.
        let reserve = wire_body(wire::ReserveFileRequest {
            filename: file.name.clone(),
            byte_size: file.bytes.len() as i64,
            link: true,
        })?;
        let answer = self
            .write_with_resend(reqwest::Method::POST, "/v1/files", Some(reserve))
            .await?;
        let v = answer.ok_or_else(|| MarketError::Protocol {
            detail: "POST /v1/files: empty answer".into(),
        })?;
        let r: wire::ReserveFileResponse =
            serde_json::from_value(v).map_err(|e| MarketError::Protocol {
                detail: format!("POST /v1/files: {e}"),
            })?;
        // `fileUrl` is present only for `link: true`, which this reserve
        // always sends; a reserve without it cannot be delivered, and the
        // slot it named is deleted so it does not outlive the refusal.
        let Some(file_url) = r.file_url else {
            if let Err(cleanup) = self.delete_file(&r.file_id).await {
                tracing::warn!(error = %cleanup, "deleting a reserve without fileUrl");
            }
            return Err(MarketError::Protocol {
                detail: "POST /v1/files: a linked reserve answered without fileUrl".into(),
            });
        };
        // The complete step needs the id; refuse before any bytes move.
        segment(&r.file_id)?;
        Ok(ReservedFile {
            file_id: r.file_id,
            upload_url: r.upload_url,
            file_url,
        })
    }

    async fn complete_file(
        &self,
        reserved: &ReservedFile,
        file: &UploadFile,
    ) -> Result<Option<StoredFile>, MarketError> {
        // A failure leaves a reserved, unbound file: it is deleted (best
        // effort) before the failure is reported (design 6.3), so no slot
        // outlives a failed upload.
        match self
            .put_and_complete(&reserved.upload_url, &reserved.file_id, &file.bytes)
            .await
        {
            Ok(stored) => Ok(stored),
            Err(e) => {
                if let Err(cleanup) = self.delete_file(&reserved.file_id).await {
                    tracing::warn!(error = %cleanup, "deleting a failed upload's reserved file");
                }
                Err(e)
            }
        }
    }

    async fn delete_file(&self, file_id: &str) -> Result<(), MarketError> {
        let path = format!("/v1/files/{}", segment(file_id)?);
        // Idempotent: a second delete, or a file the market no longer
        // knows, is gone all the same.
        match self
            .write_with_resend(reqwest::Method::DELETE, &path, None)
            .await
        {
            Ok(_) => Ok(()),
            Err(MarketError::Refused { status: 404, .. }) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

impl NearAgentMarket {
    /// The PUT of the exact bytes and the complete of one reserved file.
    /// `Ok(None)` when the complete answered `409 upload_not_pending`: the
    /// same-origin sink stored the bytes on the PUT (live capture
    /// 2026-11-10), so the market reported no digest.
    async fn put_and_complete(
        &self,
        upload_url: &str,
        file_id: &str,
        bytes: &[u8],
    ) -> Result<Option<StoredFile>, MarketError> {
        // PUT the exact bytes to the upload URL, with no extra headers (no
        // auth, no JSON content type).
        let put = self
            .http
            .put(upload_url)
            .timeout(self.timing.write)
            .body(bytes.to_vec())
            .send()
            .await;
        let put_status = match put {
            Err(e) => {
                return Err(MarketError::WriteOutcomeUnknown {
                    detail: self.redact(&format!("PUT <upload url>: {}", describe(&e))),
                });
            }
            Ok(resp) => resp.status().as_u16(),
        };
        if !(200..300).contains(&put_status) {
            return Err(MarketError::WriteOutcomeUnknown {
                detail: self.redact(&format!("PUT <upload url>: HTTP {put_status}")),
            });
        }
        let path = format!("/v1/files/{}/complete", segment(file_id)?);
        // The operation defines no request body; none is sent.
        match self
            .write_with_resend(reqwest::Method::POST, &path, None)
            .await
        {
            Ok(Some(v)) => {
                let c: wire::CompleteFileResponse =
                    serde_json::from_value(v).map_err(|e| MarketError::Protocol {
                        detail: format!("POST /v1/files/<id>/complete: {e}"),
                    })?;
                let byte_size = u64::try_from(c.byte_size).map_err(|_| MarketError::Protocol {
                    detail: "POST /v1/files/<id>/complete: a negative byteSize".into(),
                })?;
                Ok(Some(StoredFile {
                    byte_size,
                    sha256: c.sha256.to_ascii_lowercase(),
                }))
            }
            Ok(None) => Err(MarketError::Protocol {
                detail: "POST /v1/files/<id>/complete: empty answer".into(),
            }),
            Err(MarketError::Refused {
                status: 409, code, ..
            }) if code == "upload_not_pending" => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod loopback_tests {
    use super::is_loopback_host;

    /// Only a real loopback host is allowed on http; userinfo and IPv6
    /// forms are parsed, not string-split (ocr2 11).
    #[test]
    fn http_is_allowed_only_for_loopback_hosts() {
        for good in [
            "http://localhost:8080",
            "http://127.0.0.1:9000",
            "http://[::1]:8080",
            "http://[::1]",
        ] {
            assert!(is_loopback_host(good), "{good} refused");
        }
        for bad in [
            "http://localhost:x@evil.example",
            "http://u:p@127.0.0.1",
            "http://example.com",
            "http://127.0.0.1.evil.example",
            "http://0.0.0.0",
            "not a url",
        ] {
            assert!(!is_loopback_host(bad), "{bad} passed");
        }
    }
}
