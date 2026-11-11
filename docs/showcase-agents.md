# Example agents

Nearbee is a tool: any user creates any number of agents from their own
playbooks and describes them as they wish. The two agents below are **our
examples** for the demo video and the judges, nothing more. Each is an apb
playbook under `playbooks/<name>/` with `playbook.yaml`, profiles and one or
more `listing.<variant>.yaml` files, and doubles as the template a user copies.
C is dropped from v1 (D-011).

## A. `repo-audit` (dev_tools, built in S4)

The buyer names one public GitHub repository by its `https://github.com` URL
in the brief. `playbooks/repo-audit/playbook.yaml` runs four steps:

1. `fetch` (script, `nearbee audit fetch`): scans the brief again, resolves
   the repository through the GitHub API (a private repository is refused),
   takes the default branch's commit SHA and downloads that commit's
   tarball, with no redirect followed, 20 seconds per request and 120 in
   total. The archive is read as a stream with caps (100 MiB compressed,
   100 MiB decompressed counted over everything read, 20,000 members); only
   regular files with plain paths are written, links, `..`, absolute paths,
   files over 1 MiB and binary files are skipped and counted. Agent
   instruction files of the repository (`AGENTS.md`, `CLAUDE.md`,
   `.claude/`, `opencode.json` and the like, at any depth) are renamed with
   `.repo-data`, so they stay data. The snapshot `audit-snapshot/` (the
   files, `source.json` with the commit and the file list, `coverage.json`
   with the skips) is built in a staging directory and published by a
   rename; a rework reuses it and audits the same commit.
2. `audit` (agent task): reads the snapshot and writes only
   `audit/candidates.json`: at most 50 findings, each with severity, title,
   file, line range, a verbatim excerpt, detail and fix. The prompt forbids
   running anything from the repository and treats the snapshot, the brief
   and the feedback as data.
3. `finish` (script, `nearbee audit finish`): checks every citation
   against the snapshot (the file is in the list, the lines exist, the
   excerpt is a contiguous run of the cited lines). A passing finding is
   "location checked", which says the citation exists, not that the issue
   is real; a failing one is kept as rejected with its reason. It writes
   `out/findings.json` and `out/deliverable.md` (findings by severity with
   escaped text, the counts of rejected findings and skipped files, and the
   findings JSON in a fence) and publishes `out/result.json` last.
4. `done`.

The intake check `github_repo` declines a brief that names no repository or
several before any market start (the scope check is skipped while the apb
project's decisions are off, `docs/privacy.md`).

- Variants (one source playbook, materialised per listing with the
  listing's profile, `playbooks/repo-audit/listing.*.yaml`):
  `repo-audit-fast` (attested tier, profile `nearbee-near`: DeepSeek V4.1
  Flash first, Qwen3.8-27B as fallback, NEAR AI Cloud TEE models; price
  0.50) and `repo-audit-premium` (plain tier, profile `nearbee-own-premium`:
  Claude Opus on the owner's subscription; price 2.00). Both have a 24 hour
  SLA.
- What the attested variant shows: which models apb asked for per node, and
  endpoint attestation reports for those models fetched before and after
  the run, with Verification Center links. It does not prove that the
  code never left a TEE: the snapshot is on the owner's machine, and the
  reports are collected, not verified, and not bound to this job's
  responses (`docs/privacy.md`).
- Why: dev_tools is the second largest category and apb's home turf.

## B. `doc-digest-private` (data, privacy track, built in S5)

Playbook `playbooks/doc-digest/`, listing
`playbooks/doc-digest/listing.doc-digest-private.yaml` (attested, 1.00
USD, category `data`). The buyer attaches up to 10 PDF, PNG or JPEG files
(`file_types: [pdf, png, jpg, jpeg]`, at most 25 MiB in total; intake
declines a hire whose declared sizes sum above that, `rejected`, because
`max_attachment_bytes` bounds the sum) and asks
for a digest: summary, entities, key figures with their source page, open
questions, and a machine-readable `digest.json`. No DOCX: a buyer converts
it to PDF first. Every step is a Nearbee command in a script node (D-026);
there is no agent step, and the playbook sets no default profile (apb
validates it without one), so the statement lists no agent model.

1. `nearbee digest pages`: the files are read under `inputs/` without
   following links and sniffed by their leading bytes; PDF pages render
   with `pdftoppm` (long side 2000 px) and every page goes through
   ImageMagick with resource limits to at most 2 megapixels. Every tool
   runs under `prlimit` (1 GiB address space, 60 s CPU) with a 60 s wall
   clock. At most 40 pages: the rest are skipped and counted; an
   encrypted or unreadable PDF is skipped and counted as unusable.
2. `nearbee digest extract`: one call per page to
   `Qwen/Qwen3-VL-30B-A3B-Instruct` with the page as an image part and a
   fixed transcription prompt; a page is retried once; an answer cut at
   its token bound is marked `truncated`.
3. `nearbee digest classify`: `/v1/privacy/classify` with
   `openai/privacy-filter`; only counts per category are kept, never the
   span texts.
4. `nearbee digest compose`: the signed call (`nearbee signed-call`) to
   `deepseek-ai/DeepSeek-V4.1-Flash-1017` with the page texts, the counts
   and the buyer's change request as labelled data, at most 900 KiB
   (pages cut last first and counted once, as skipped). The answer must
   match the digest schema exactly; a figure is kept only when its value
   is on the cited page. The schema allows only organisations, document
   types, dates and amounts as entities and the prompt forbids names and
   contact details, but the prompt is not relied on: every free-text value
   of the answer (summary, entity values, figure values and labels, open
   questions) then goes through `/v1/privacy/classify` again, each span is
   replaced by `[redacted: <category>]`, and a figure whose value held
   one is dropped and counted. The personal data section shows categories
   and counts from the page classification only.
5. Extraction and classify calls send `x-no-aliasing: true` and fail on
   an answer that names another model; an error names the field, never
   document or model text.

State lives in `digest/<execution_id>/` of the hire's worktree, so a
retried node resumes and a rework starts fresh with a new signed call.
The deliverable carries the `Response signature` section with the
Verification Center receipt link (docs/privacy.md). Not live yet: the
owner's Cloud credits (O-3) and the draft agent (O-2) are needed for the
first hire.

- Variant: `doc-digest-private` only (attested tier, signed digest step). A
  `blind` variant for x402 callers is a separate design after the hackathon.
- Why: a buyer-facing privacy story with real evidence, usable by a normal
  company (invoices, contracts, scans).

## C. `playbook-from-brief` (automation, stretch)

Buyer describes a routine in prose; the agent returns a ready apb playbook
(YAML), the profiles it needs, and a short how-to. The deliverable is the
playbook file plus the explanation.

- Variant: `playbook-from-brief` (fast, attested tier).
- Why: it sells apb itself through the market and costs little to build, since
  apb's `playbook_interview` and `playbook_howto` already exist.

## The demo moment

The video shows B: a scan of an invoice goes in from the web, the job page
shows progress messages, the deliverable arrives with the digest and an
"Evidence" section; the presenter opens the Verification Center link and the
attestation checks out. Then A is hired agent-to-agent from a terminal with the
x402 caller: 402, payment signed, the task id returned at once, the result a few
minutes later (the poll interval plus the audit), the payment settled at
delivery. The owner's USDC payout is a separate step the treasury sweep makes
"usually within minutes": show it afterwards as its own moment, never as part
of the delivery.
