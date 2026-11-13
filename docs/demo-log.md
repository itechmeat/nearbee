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

## 2026-11-12: the fast audit's model reasoned past the bound

The first live runs of the fast listing's playbook (version
`1.0.2808650427`, exported from Nearbee's apb instance and run by apb
`1.0.0-beta.7` in a locked-down container: read-only root, all capabilities
dropped, no host home, only `NEARAI_API_KEY`) failed at `review` with "the
call timed out after 240 s", for `itechmeat/nearbee` and for the 20-file
`sindresorhus/is-plain-obj` alike. The same request from the host timed
out too, so the container was not the cause. A streamed copy showed
DeepSeek V4.1 Flash writing only reasoning text (14,500 characters in 90
seconds, no answer). With `chat_template_kwargs: {"thinking": false}` the
exact request answered in 34 s with 0 reasoning tokens; `reasoning_effort:
low` and `thinking.type: disabled` had no effect.

After the change `model-thinking-off`, the same container run of
`sindresorhus/is-plain-obj` succeeded in 30 s end to end: `fetch`,
`review` and `finish` succeeded, `signature.json` written (completion
`chatcmpl-521c7aa3-ccff-4b63-890b-e6ee2157101f`, ECDSA, signing address
`0xfe0071895500a9755c18044948ab4f4e1a7f337e`), 3,483 prompt and 1,287
completion tokens, 4 findings location checked and 1 rejected.

The document digest playbook (`playbooks/doc-digest`, version 1.0.0) ran
in the same container with the fix on a synthetic one-page invoice PNG
(made-up supplier, person, email and phone): all four steps succeeded in
37 s. One page rendered and transcribed, personal data counted as one
`private_person`, one `private_email` and one `private_phone`, 12 figures
kept and 3 dropped as not found on the cited page, 2 spans in the digest
replaced by their category, and the digest call signed (completion
`chatcmpl-dadc6716-fb94-4d9a-b1c2-0e057a4ade54`, the same signing address).
The container image needs `poppler-utils`, `imagemagick` (the `magick`
command) and `util-linux` (`prlimit`) for the `pages` step.

## 2026-11-12: Nearbee's apb instance in the sandbox

Nearbee's own instance (unit `nearbee-apb`, port 7322, the real workspace)
was switched to the sandboxed mode of change `apb-sandbox`: image
`nearbee-sandbox:local` built from the host `apb` 1.0.0-beta.7 and the
release `nearbee` of this change, gateway `nearbee-apb-gw`, dashboard
container `nearbee-apb-sbx`. `status`: unit active, container and gateway
running, image matching the host binaries, health 200; a server key was
issued once and written to `.env`.

A fast audit run started through the REST API with that key
(`repo-audit-fast` 1.0.2808650427, `sindresorhus/is-plain-obj`, run
`repo-audit-fast-1794521714830`) succeeded: `fetch`, `review` and `finish`
all succeeded inside the container. From inside the container
`https://cloud-api.near.ai/v1/models` answered 200, `https://example.com`
did not resolve, and the owner's `~/.ssh` did not exist.

## 2026-11-13: paid hires over x402 through the sandbox

Buyer `solaitken.near` (a separate wallet holding native USDC, funded
through NEAR Intents), listing `repo-audit-fast` made live by the owner in
the web app (price 0.50 USD, SLA 86400 s), Nearbee's apb instance
sandboxed, `nearbee serve` polling.

1. Hire `1d8ffbf5-287b-404e-b637-c2285b5352d8`
   (`examples/x402-hire`, `itechmeat/nearbee`): the 402 quote of 500000
   atomic USDC, the NEP-366 delegate signed and checked, the task
   `TASK_STATE_COMPLETED`, serve `Delivered`. The deliverable had no
   findings: the pack filled the 768 KiB cap (49 files, 213,396 prompt
   tokens) and the model answered `{"version": 1, "findings": []}` in 13
   tokens. This led to the change `review-quality`.
2. Hire `dbfbe061-27a1-4eb2-a914-eb5c959a571f`, the same brief after
   `review-quality` (256 KiB cap, JSON mode): completed with 14 findings,
   all location checked, none rejected; 609 files listed as not reviewed.
3. Hire `1a4d2709-37f4-4f6f-a73d-aa4bf42a3fe7` with `nearbee serve --reject
   repo-audit-fast`: serve declined it `rejected` before any work, the task
   ended `TASK_STATE_REJECTED`, and the buyer's USDC balance was unchanged
   (8.894556 before and after), so the declined hire cost the buyer nothing.
