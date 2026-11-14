//! The five scenarios and their checks (S6 design 1.5, 1.6).

use std::io::Write;

use nearbee_core::journal::Event;
use nearbee_core::signature::VERIFY_URL;

use super::World;

/// A brief the fast listing takes.
const BRIEF: &str = "Audit https://github.com/example/webhook-demo for security problems.";
/// A brief that names no GitHub repository.
const BRIEF_NO_REPO: &str = "Audit my webhook library for security problems.";
/// The buyer's change request in the redo scenario.
const FEEDBACK: &str = "Please add the fix as a code example.";

/// Runs one scenario and checks its end state.
pub(super) async fn run(name: &str, w: &mut World, out: &mut dyn Write) -> Result<(), String> {
    let id = format!("{name}-hire");
    let id = id.as_str();
    match name {
        "success" => {
            let _ = writeln!(
                out,
                "scenario success: a hire is taken, run, delivered and accepted"
            );
            deliver(w, id, out).await?;
            accept(w, id, out).await?;
            let submitted = w.market.submitted();
            let _ = writeln!(out, "  the deliverable as submitted begins:");
            for line in submitted[0].lines().take(3) {
                let _ = writeln!(out, "    | {line}");
            }
            check(w, id, &["start", "submit"], 1, "accepted")?;
            receipt(&submitted)
        }
        "decline" => {
            let _ = writeln!(
                out,
                "scenario decline: a brief that names no GitHub repository is declined at intake"
            );
            w.hire_arrives(id, BRIEF_NO_REPO)?;
            let owned = id.to_owned();
            w.step(id, "a hire arrives", out, move |w| {
                w.lifecycle(&owned).as_deref() == Some("cancelled")
            })
            .await?;
            let intake = w.journal(id).is_some_and(|j| {
                j.records.iter().any(|r| {
                    matches!(&r.event,
                    Event::Intake(i) if i.check == "github_repo" && i.outcome == "declined")
                })
            });
            if !intake {
                return Err("expected the github_repo intake check to decline, it did not".into());
            }
            check(w, id, &["decline rejected"], 0, "cancelled")
        }
        "redo" => {
            let _ = writeln!(
                out,
                "scenario redo: the buyer asks for changes and the continued run is delivered"
            );
            deliver(w, id, out).await?;
            let first = w.runner.latest_run().unwrap_or_default();
            let now = w.now();
            w.market.edit(|rows| {
                for r in rows.iter_mut() {
                    r.status = Some("in_progress".into());
                    r.submitted_at = Some(now);
                    r.latest_message = Some(nearbee_core::market::Message {
                        body: FEEDBACK.into(),
                        sender_side: Some("buyer".into()),
                        created_at: Some(now),
                        truncated: false,
                        attachments: Vec::new(),
                    });
                }
            });
            w.step(id, "the buyer asks for changes", out, |w| {
                w.runner.starts().len() == 2
            })
            .await?;
            let second = w.runner.starts()[1].clone();
            let continued = second
                .continued_from
                .as_ref()
                .map(|r| r.as_str().to_owned());
            if continued.as_deref() != Some(first.as_str()) {
                return Err(format!(
                    "expected the second run to continue the first, it continues {}",
                    if continued.is_some() {
                        "another run"
                    } else {
                        "none"
                    }
                ));
            }
            if second.params.get("feedback").map(String::as_str) != Some(FEEDBACK) {
                return Err(
                    "expected the feedback param on the second run, it is not there".into(),
                );
            }
            w.runner.finish_latest()?;
            w.step(id, "the continued run finishes", out, |w| {
                w.market.submitted().len() == 2
            })
            .await?;
            accept(w, id, out).await?;
            check(w, id, &["start", "submit", "submit"], 2, "accepted")?;
            receipt(&w.market.submitted())
        }
        "restart" => {
            let _ = writeln!(
                out,
                "scenario restart: the run start's answer is lost and serve restarts"
            );
            w.runner.lose_next_answer();
            w.hire_arrives(id, BRIEF)?;
            w.step(
                id,
                "a hire arrives; apb starts the run but its answer is lost",
                out,
                |w| !w.runner.starts().is_empty(),
            )
            .await?;
            let _ = writeln!(out, "  serve restarts: its in-memory state is dropped");
            w.state = nearbee_core::serve::RoundState::default();
            w.step(id, "the open start intent is resolved", out, |w| {
                w.runner.finds() > 0
                    && w.journal(id)
                        .is_some_and(|j| j.open_intents.iter().all(|i| i.kind != "run_start"))
            })
            .await?;
            w.runner.finish_latest()?;
            w.step(id, "the run finishes", out, |w| {
                w.market.submitted().len() == 1
            })
            .await?;
            accept(w, id, out).await?;
            check(w, id, &["start", "submit"], 1, "accepted")
        }
        "cancellation" => {
            let _ = writeln!(
                out,
                "scenario cancellation: the buyer cancels while the run is working"
            );
            w.hire_arrives(id, BRIEF)?;
            w.step(id, "a hire arrives", out, |w| !w.runner.starts().is_empty())
                .await?;
            w.step(id, "the run is working", out, |_| true).await?;
            w.market.edit(|rows| {
                for r in rows.iter_mut() {
                    r.status = Some("cancelled".into());
                    r.cancel_reason = Some("buyer_cancelled".into());
                }
            });
            w.step(id, "the buyer cancels", out, |w| {
                w.runner.aborts() > 0
                    && w.journal(id).is_some_and(|j| {
                        j.records.iter().any(|r| {
                            matches!(&r.event,
                            Event::RunObserved(o) if o.status == "aborted")
                        })
                    })
            })
            .await?;
            let owned = id.to_owned();
            w.step(id, "the hire is closed", out, move |w| {
                w.journal(&owned).is_some_and(|j| j.open_intents.is_empty())
            })
            .await?;
            check(w, id, &["start"], 1, "cancelled")
        }
        other => Err(format!("unknown scenario {other}")),
    }
}

/// A hire arrives, its run starts and finishes, and it is submitted.
async fn deliver(w: &mut World, id: &str, out: &mut dyn Write) -> Result<(), String> {
    w.hire_arrives(id, BRIEF)?;
    let starts = w.runner.starts().len();
    w.step(id, "a hire arrives", out, move |w| {
        w.runner.starts().len() > starts
    })
    .await?;
    w.runner.finish_latest()?;
    let submitted = w.market.submitted().len();
    w.step(id, "the run finishes", out, move |w| {
        w.market.submitted().len() > submitted
    })
    .await
}

/// The buyer accepts the submitted hire.
async fn accept(w: &mut World, id: &str, out: &mut dyn Write) -> Result<(), String> {
    let now = w.now();
    w.market.edit(|rows| {
        for r in rows.iter_mut() {
            r.status = Some("accepted".into());
            r.finalized_at = Some(now);
        }
    });
    let owned = id.to_owned();
    w.step(id, "the buyer accepts", out, move |w| {
        w.lifecycle(&owned).as_deref() == Some("accepted")
    })
    .await
}

/// The market writes in order, the run starts and the end state.
fn check(w: &World, id: &str, writes: &[&str], starts: usize, end: &str) -> Result<(), String> {
    let got = w.market.writes();
    if got != writes {
        return Err(format!(
            "expected the market writes [{}], got [{}]",
            writes.join(", "),
            got.join(", ")
        ));
    }
    let n = w.runner.starts().len();
    if n != starts {
        return Err(format!("expected {starts} run starts, got {n}"));
    }
    let state = w.lifecycle(id).unwrap_or_else(|| "none".into());
    if state != end {
        return Err(format!("expected the hire to end {end}, it ended {state}"));
    }
    Ok(())
}

/// Every submitted deliverable carries the Verification Center receipt
/// link as a bare URL alone on its line.
fn receipt(submitted: &[String]) -> Result<(), String> {
    for (i, md) in submitted.iter().enumerate() {
        let on_its_line = md.lines().any(|l| {
            l.starts_with(&format!("{VERIFY_URL}?model="))
                && l.contains("#receipt=")
                && !l.contains(')')
        });
        if !on_its_line {
            return Err(format!(
                "expected a receipt link in submission {}, there is none",
                i + 1
            ));
        }
    }
    Ok(())
}
