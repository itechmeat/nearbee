# Demo log

Recorded end-to-end runs on the live NEAR AI Agent Market. Times are UTC.

## 2026-11-10: first end-to-end hire (S2 task 7.1)

The owner hired the draft agent `repo-audit-fast` from the market web app
("Test run": nothing held, 10-minute SLA) with a short synthetic brief, while
`nearbee serve` ran on the owner's machine against Nearbee's own apb
instance.

| Field | Value |
| --- | --- |
| Assignment | `ae6a5c83-e4e6-4563-b7b4-3c65e2755f23` |
| Hire placed | 09:29:40 |
| Started by serve | 09:30:06 |
| apb run | `repo-audit-fast-1794303006668`, playbook version `1.0.1689991491` |
| Submitted | 09:30:30 (24 s after the start) |
| SLA deadline | 09:39:40 |
| Deliverable | `deliverables/managed/ae6a5c83-e4e6-4563-b7b4-3c65e2755f23/4a9411b1d9de223a.md` |

Two earlier test runs the same morning were declined `failed` by serve and
fixed before this one:

1. `2facf432-840f-40ae-aaee-4fa12dba1bb8`: apb refused the run start with
   `400 invalid_budget` (a budget needs a token or dollar limit; serve sent
   none). Serve now sets a per-run dollar ceiling.
2. `0bad290b-14ec-4074-afdd-e2b9092e17da`: the 10-minute SLA of a test run
   minus serve's fixed 10-minute margin left no time; the margin is now at
   most a fifth of the work window.

## 2026-11-10: a second hire and the test decline (S2 task 7.2)

The owner placed two more "Test run" hires of `repo-audit-fast` with the same
87-byte brief.

1. `43636aba-e64e-45f7-b7fe-8f38d73108d4`, placed 10:48:48 while `serve`
   still ran in normal mode: started at once, apb run
   `repo-audit-fast-1794307729439` on `1.0.1689991491`, submitted at
   10:49:02 (14 s). The first run detail right after the start carried no
   `version` yet (`playbook_version: null`); the next poll named it.
2. `d17ae716-ef46-47a5-8e4b-aae3e83dd678`, placed 10:49:58 with `serve
   --reject repo-audit-fast`: declined at 10:50:15 (17 s) without a start or
   a run. The market row then reads `status: cancelled`, `cancelReason:
   rejected`, `cancelDetail: "test decline, nothing is charged"`,
   `startedAt` and `submittedAt` null.

## 2026-11-11: agent A on the owner's subscription (S4 task 11.4)

The `repo-audit` playbook, materialised by hand as
`repo-audit-premium-live` with `defaults.profile: nearbee-own-premium`
(the premium listing has no agent token yet, O-2), ran through Nearbee's
apb (REST start, `itechmeat/nearbee` as the brief's repository, run
`repo-audit-premium-live-1794390013910`, about five minutes):

1. `fetch`: the GitHub snapshot of commit
   `9f76e441f9ce18b3ab5cbab65f67b3d9bcefce27`, 595 archive members, 487
   files written, one link skipped.
2. `audit` on Claude Opus: 20 candidate findings.
3. `finish`: all 20 location checked, none rejected; `out/deliverable.md`
   34,914 bytes with the findings JSON embedded, `out/findings.json`, and a
   manifest naming only the Markdown.

A first attempt failed at `fetch` with "script not found" because the
hand-built bundle named its scripts `scripts/fetch.sh`; `nearbee list`
names them by file name, as apb expects. `nearbee list` of the fast
listing on the live instance passed the attested check (version
`1.0.86431461`, evidence models DeepSeek V4.1 Flash and Qwen3.8-27B).

After the review round (the `audit` node now declares `writes:
["audit/**"]`, `max_changed_files: 1` and `protect:
["audit-snapshot/**"]`, and the instance runs with Nearbee's own
`XDG_CONFIG_HOME`), the same run as `repo-audit-premium-live` 1.0.3
(`repo-audit-premium-live-1794392573316`) succeeded again: 9 findings,
all location checked, a 19,581-byte deliverable. `nearbee list` of the
fast listing passed the stricter attested check on the live instance
(version `1.0.1828462250`).
