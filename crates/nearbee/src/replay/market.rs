//! The scripted market: one assignment row whose fields the scenario
//! sets, and a log of the writes serve made (S6 design 1.3).

use std::sync::{Arc, Mutex};

use nearbee_core::market::{
    AgentPatch, AgentRecord, DeclineOutcome, DeclineRequest, Market, MarketError, MessageRequest,
    NewPricingPlan, PricingPlan, ProgressOutcome, ProgressRequest, Row, SubmitRequest,
    ThreadMessage, UploadFile, UploadedFile,
};
use time::OffsetDateTime;

#[derive(Debug, Default)]
struct Inner {
    rows: Vec<Row>,
    /// Every write, in order, as `start`, `progress`, `submit` or
    /// `decline <reason>`.
    writes: Vec<String>,
    /// The writes since the last [`ReplayMarket::take_events`].
    events: Vec<String>,
    submitted: Vec<String>,
}

/// The replay's [`Market`].
#[derive(Debug)]
pub struct ReplayMarket {
    inner: Mutex<Inner>,
    now: Arc<Mutex<OffsetDateTime>>,
}

impl ReplayMarket {
    pub fn new(now: Arc<Mutex<OffsetDateTime>>) -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            now,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn now(&self) -> OffsetDateTime {
        *self.now.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The rows serve reads this round.
    pub fn rows(&self) -> Vec<Row> {
        self.lock().rows.clone()
    }

    /// Changes the scenario's rows.
    pub fn edit(&self, f: impl FnOnce(&mut Vec<Row>)) {
        f(&mut self.lock().rows);
    }

    pub fn writes(&self) -> Vec<String> {
        self.lock().writes.clone()
    }

    /// The Markdown of every submit, in order.
    pub fn submitted(&self) -> Vec<String> {
        self.lock().submitted.clone()
    }

    pub fn take_events(&self) -> Vec<String> {
        std::mem::take(&mut self.lock().events)
    }

    fn record(inner: &mut Inner, write: &str, event: String) {
        inner.writes.push(write.to_owned());
        inner.events.push(event);
    }
}

fn unused(what: &str) -> MarketError {
    MarketError::Protocol {
        detail: format!("the replay does not {what}"),
    }
}

impl Market for ReplayMarket {
    async fn assignments(&self) -> Result<Vec<Row>, MarketError> {
        Ok(self.rows())
    }

    async fn start(&self, row_id: &str) -> Result<(), MarketError> {
        let now = self.now();
        let mut inner = self.lock();
        Self::record(&mut inner, "start", "market: start".into());
        if let Some(r) = inner.rows.iter_mut().find(|r| r.id == row_id) {
            r.started_at = Some(now);
        }
        Ok(())
    }

    async fn message(&self, _row_id: &str, _req: &MessageRequest) -> Result<(), MarketError> {
        let mut inner = self.lock();
        Self::record(&mut inner, "message", "market: message".into());
        Ok(())
    }

    async fn progress(
        &self,
        _row_id: &str,
        _req: &ProgressRequest,
    ) -> Result<ProgressOutcome, MarketError> {
        let mut inner = self.lock();
        Self::record(&mut inner, "progress", "market: progress".into());
        Ok(ProgressOutcome::Reported)
    }

    async fn submit(&self, row_id: &str, req: &SubmitRequest) -> Result<(), MarketError> {
        let now = self.now();
        let mut inner = self.lock();
        Self::record(
            &mut inner,
            "submit",
            format!("market: submit ({} bytes of Markdown)", req.markdown.len()),
        );
        inner.submitted.push(req.markdown.clone());
        // The market holds the deliverable for the buyer's review.
        if let Some(r) = inner.rows.iter_mut().find(|r| r.id == row_id) {
            r.status = Some("submitted".into());
            r.submitted_at = Some(now);
            r.deliverable_url = Some("deliverables/managed/replay/deliverable.md".into());
            r.auto_accept_at = Some(now + time::Duration::days(3));
        }
        Ok(())
    }

    async fn decline(
        &self,
        row_id: &str,
        req: &DeclineRequest,
    ) -> Result<DeclineOutcome, MarketError> {
        let mut inner = self.lock();
        let reason = req.reason.as_str();
        Self::record(
            &mut inner,
            &format!("decline {reason}"),
            format!("market: decline {reason}"),
        );
        if let Some(r) = inner.rows.iter_mut().find(|r| r.id == row_id) {
            r.status = Some("cancelled".into());
            r.cancel_reason = Some(reason.to_owned());
        }
        Ok(DeclineOutcome::Done)
    }

    async fn agent(&self, _handle: &str) -> Result<AgentRecord, MarketError> {
        Err(unused("read agents"))
    }

    async fn patch_agent(&self, _a: &str, _p: &AgentPatch) -> Result<AgentRecord, MarketError> {
        Err(unused("patch agents"))
    }

    async fn pricing_plans(&self, _a: &str) -> Result<Vec<PricingPlan>, MarketError> {
        Err(unused("read pricing plans"))
    }

    async fn create_pricing_plan(
        &self,
        _a: &str,
        _p: &NewPricingPlan,
    ) -> Result<PricingPlan, MarketError> {
        Err(unused("create pricing plans"))
    }

    async fn make_default(&self, _a: &str, _p: &str) -> Result<(), MarketError> {
        Err(unused("change pricing plans"))
    }

    async fn upload_file(&self, _f: &UploadFile) -> Result<UploadedFile, MarketError> {
        Err(unused("upload files"))
    }

    async fn messages(
        &self,
        row_id: &str,
        _until: Option<OffsetDateTime>,
    ) -> Result<Vec<ThreadMessage>, MarketError> {
        let inner = self.lock();
        Ok(inner
            .rows
            .iter()
            .filter(|r| r.id == row_id)
            .filter_map(|r| r.latest_message.as_ref())
            .map(|m| ThreadMessage {
                message_id: "replay-message-1".into(),
                body: m.body.clone(),
                sender_agent_id: None,
                attachments: Vec::new(),
                created_at: m.created_at,
            })
            .collect())
    }

    fn origin(&self) -> String {
        "https://market.replay.invalid".into()
    }

    async fn download(
        &self,
        _path: &str,
        _cap: u64,
        _out: &mut (dyn std::io::Write + Send),
    ) -> Result<u64, MarketError> {
        Err(unused("download files"))
    }
}
