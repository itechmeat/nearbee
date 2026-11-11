//! The scripted attestor: every endpoint report is the recorded NEAR AI
//! Cloud attestation report, embedded in the binary (S6 design 1.4).

use std::sync::Mutex;

use nearbee_core::evidence::Attestor;

/// The replay's [`Attestor`].
#[derive(Debug, Default)]
pub struct ReplayAttestor {
    events: Mutex<Vec<String>>,
}

impl ReplayAttestor {
    pub fn take_events(&self) -> Vec<String> {
        std::mem::take(&mut self.events.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

impl Attestor for ReplayAttestor {
    async fn report(&self, model: &str, _nonce: &str) -> Result<Vec<u8>, String> {
        self.events
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(format!("cloud: attestation report of {model} (recorded)"));
        Ok(super::runner::report_body())
    }
}
