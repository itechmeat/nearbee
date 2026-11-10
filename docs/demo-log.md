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
