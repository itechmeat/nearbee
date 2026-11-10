//! Cost of hires (design 8): the per-hire ceiling, what a hire has spent,
//! the next run's budget and the daily loss across all hires.
//!
//! A run's charge is its reported cost when that cost is complete; a
//! missing or incomplete cost counts as the run's whole budget (safe by
//! design: a run without a reported cost leaves less budget, never more),
//! and a run started without a budget (protection off) counts its reported
//! cost as is.

use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;

use rust_decimal::Decimal;
use time::{Date, OffsetDateTime};

use crate::journal::{Event, Record, RunObserved};
use crate::market::Row;
use crate::runner::RunStatus;

/// The hire's ceiling: `amount * 0.95 - 0.25` USD, where `amount` is the
/// row's escrow when above zero and the listing's price for a zero escrow
/// (an owner test run). Spec `hire-intake`, Cost admission.
pub fn ceiling(row: &Row, price_usd: Decimal) -> Decimal {
    let escrow = row
        .escrow_amount
        .as_deref()
        .and_then(|s| Decimal::from_str(s.trim()).ok())
        .filter(|d| *d > Decimal::ZERO);
    let amount = escrow.unwrap_or(price_usd);
    (amount * Decimal::new(95, 2) - Decimal::new(25, 2)).normalize()
}

fn dec(s: &Option<String>) -> Option<Decimal> {
    s.as_deref().and_then(|v| Decimal::from_str(v.trim()).ok())
}

/// What one terminal run observation charges (design 8).
pub fn charge(o: &RunObserved) -> Decimal {
    let cost = dec(&o.cost_usd);
    match dec(&o.budget_usd) {
        Some(budget) => match (cost, o.cost_complete) {
            (Some(c), Some(true)) => c,
            _ => budget,
        },
        // No budget (protection off, a stray): the reported cost as is.
        None => cost.unwrap_or(Decimal::ZERO),
    }
}

/// The terminal observation of every run of these records, one per run
/// id (the last one wins), with its record.
fn terminal_runs(records: &[Record]) -> BTreeMap<String, &Record> {
    let mut out = BTreeMap::new();
    for r in records {
        if let Event::RunObserved(o) = &r.event
            && RunStatus::parse(&o.status).is_terminal()
        {
            out.insert(o.run_id.clone(), r);
        }
    }
    out
}

fn observed(r: &Record) -> &RunObserved {
    match &r.event {
        Event::RunObserved(o) => o,
        _ => unreachable!("terminal_runs keeps run observations only"),
    }
}

/// What a hire has spent: the charges of its runs' terminal observations,
/// every generation and stray included.
pub fn spent(records: &[Record]) -> Decimal {
    terminal_runs(records)
        .values()
        .map(|r| charge(observed(r)))
        .sum()
}

/// The next run's budget: the ceiling minus what the hire has spent.
pub fn next_budget(ceiling: Decimal, records: &[Record]) -> Decimal {
    (ceiling - spent(records)).normalize()
}

/// Whether the generation of a run ended without a delivery: a decline,
/// an abort, or a hire the market closed by cancellation; a stray
/// (execution 0) always does.
fn ended_without_delivery(records: &[Record], generation: u32, execution: u32) -> bool {
    if execution == 0 {
        return true;
    }
    let gen_records = records.iter().filter(|r| r.generation == generation);
    let mut delivered = false;
    let mut failed = false;
    for r in gen_records {
        match &r.event {
            Event::Decision(d) if d.decision == "delivered" => delivered = true,
            Event::Decision(d) if d.decision == "decline" => failed = true,
            Event::Intent(i) if i.kind == "run_abort" || i.kind == "market_decline" => {
                failed = true
            }
            _ => {}
        }
    }
    let cancelled = records.iter().any(|r| {
        matches!(&r.event, Event::Lifecycle(l)
            if l.status == "cancelled" || l.status == "dispute_resolved")
    });
    !delivered && (failed || cancelled)
}

/// The loss of one hire's journal on a UTC day: the charges of the runs
/// that ended without a delivery, dated by their terminal observation.
pub fn loss_on(records: &[Record], day: Date) -> Decimal {
    terminal_runs(records)
        .values()
        .filter(|r| r.timestamp.to_offset(time::UtcOffset::UTC).date() == day)
        .filter(|r| {
            let o = observed(r);
            ended_without_delivery(records, r.generation, o.execution)
        })
        .map(|r| charge(observed(r)))
        .sum()
}

/// The daily loss across every hire journal under `state_dir` on the UTC
/// day of `now` (spec `hire-intake`, Daily loss limit). A journal that
/// cannot be read is skipped here; it already holds the slot (design 5).
/// A `hires/` directory that cannot be listed leaves the loss unknown: the
/// error says why (review round 3, R3-1).
pub fn daily_loss(state_dir: &Path, now: OffsetDateTime) -> Result<Decimal, String> {
    let day = now.to_offset(time::UtcOffset::UTC).date();
    let scan = crate::retention::scan(state_dir);
    if let Some(problem) = scan.problem {
        return Err(problem);
    }
    Ok(scan
        .entries
        .iter()
        .filter_map(|e| e.read.as_ref().ok())
        .map(|j| loss_on(&j.records, day))
        .sum())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::{self, Decision, Intent, Lifecycle};
    use time::macros::datetime;

    fn row(escrow: Option<&str>) -> Row {
        let mut v = serde_json::json!({"id": "a1", "status": "in_progress"});
        if let Some(e) = escrow {
            v["escrowAmount"] = serde_json::json!(e);
        }
        serde_json::from_value(v).unwrap()
    }

    fn rec(seq: u64, generation: u32, at: OffsetDateTime, event: Event) -> Record {
        Record {
            schema_version: journal::SCHEMA_VERSION,
            seq,
            timestamp: at,
            hire_id: "a1".into(),
            generation,
            event,
        }
    }

    fn terminal(
        run: &str,
        execution: u32,
        cost: Option<&str>,
        complete: Option<bool>,
        budget: Option<&str>,
    ) -> Event {
        Event::RunObserved(RunObserved {
            run_id: run.into(),
            execution,
            status: "failed".into(),
            playbook_version: Some("1.0.1".into()),
            cost_usd: cost.map(str::to_owned),
            cost_complete: complete,
            budget_usd: budget.map(str::to_owned),
        })
    }

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn the_ceiling_comes_from_the_escrow_or_the_price_for_a_zero_escrow() {
        assert_eq!(ceiling(&row(Some("3.00")), d("0.50")), d("2.60"));
        assert_eq!(ceiling(&row(Some("0")), d("0.50")), d("0.225"));
        assert_eq!(ceiling(&row(None), d("0.50")), d("0.225"));
    }

    #[test]
    fn a_missing_or_incomplete_cost_counts_as_the_whole_budget() {
        let at = datetime!(2026-11-10 12:00 UTC);
        let complete = rec(
            1,
            1,
            at,
            terminal("r1", 1, Some("0.10"), Some(true), Some("2.00")),
        );
        let partial = rec(
            2,
            1,
            at,
            terminal("r2", 1, Some("0.10"), Some(false), Some("2.00")),
        );
        let missing = rec(3, 1, at, terminal("r3", 1, None, None, Some("1.50")));
        let unprotected = rec(4, 1, at, terminal("r4", 1, Some("0.30"), None, None));
        assert_eq!(spent(std::slice::from_ref(&complete)), d("0.10"));
        assert_eq!(spent(std::slice::from_ref(&partial)), d("2.00"));
        assert_eq!(spent(std::slice::from_ref(&missing)), d("1.50"));
        assert_eq!(spent(std::slice::from_ref(&unprotected)), d("0.30"));
        assert_eq!(
            next_budget(d("2.60"), &[complete, missing]),
            d("2.60") - d("0.10") - d("1.50")
        );
    }

    #[test]
    fn the_daily_loss_counts_undelivered_runs_by_their_utc_day() {
        let today = datetime!(2026-11-10 23:30 UTC);
        let yesterday = datetime!(2026-11-09 23:30 UTC);
        let declined = vec![
            rec(
                1,
                1,
                today,
                terminal("r1", 1, Some("0.40"), Some(true), Some("2.00")),
            ),
            rec(
                2,
                1,
                today,
                Event::Decision(Decision {
                    decision: "decline".into(),
                    reason: "failed: x".into(),
                }),
            ),
        ];
        assert_eq!(loss_on(&declined, today.date()), d("0.40"));
        // The next UTC day starts from zero.
        assert_eq!(
            loss_on(&declined, today.date().next_day().unwrap()),
            Decimal::ZERO
        );
        let delivered = vec![
            rec(
                1,
                1,
                today,
                terminal("r1", 1, Some("0.40"), Some(true), Some("2.00")),
            ),
            rec(
                2,
                1,
                today,
                Event::Decision(Decision {
                    decision: "delivered".into(),
                    reason: String::new(),
                }),
            ),
        ];
        assert_eq!(loss_on(&delivered, today.date()), Decimal::ZERO);
        // A stray's cost counts.
        let stray = vec![rec(
            1,
            1,
            today,
            terminal("rs", 0, Some("0.25"), None, None),
        )];
        assert_eq!(loss_on(&stray, today.date()), d("0.25"));
        // An aborted run of a cancelled hire counts on its own day.
        let cancelled = vec![
            rec(
                1,
                1,
                yesterday,
                Event::Intent(Intent {
                    operation_id: uuid::Uuid::now_v7(),
                    kind: "run_abort".into(),
                    execution: 1,
                    request_digest: "x".into(),
                    refs: Default::default(),
                }),
            ),
            rec(2, 1, yesterday, terminal("r1", 1, None, None, Some("1.00"))),
            rec(
                3,
                1,
                yesterday,
                Event::Lifecycle(Lifecycle {
                    status: "cancelled".into(),
                    cancel_reason: Some("buyer".into()),
                    finalized_at: None,
                }),
            ),
        ];
        assert_eq!(loss_on(&cancelled, yesterday.date()), d("1.00"));
        assert_eq!(loss_on(&cancelled, today.date()), Decimal::ZERO);
    }
}
