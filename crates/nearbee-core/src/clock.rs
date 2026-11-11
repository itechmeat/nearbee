//! The wall clock, injectable so multi-round tests and the replay can move
//! time; production uses [`Clock::real`].

use std::sync::Arc;

use time::OffsetDateTime;

/// The wall clock of a round and of the journal records it writes (S6
/// design 2). Production uses [`Clock::real`].
#[derive(Clone)]
pub struct Clock(Arc<dyn Fn() -> OffsetDateTime + Send + Sync>);

impl std::fmt::Debug for Clock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Clock")
    }
}

impl Clock {
    /// The real UTC clock.
    pub fn real() -> Self {
        Self::new(OffsetDateTime::now_utc)
    }

    /// A clock reading its instant from `now`.
    pub fn new(now: impl Fn() -> OffsetDateTime + Send + Sync + 'static) -> Self {
        Self(Arc::new(now))
    }

    pub fn now(&self) -> OffsetDateTime {
        (self.0)()
    }
}
