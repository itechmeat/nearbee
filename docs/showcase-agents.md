# Example agents

Nearbee is a tool: any user creates any number of agents from their own
playbooks and describes them as they wish. The two agents below are **our
examples** for the demo video and the judges, nothing more. Each is an apb
playbook under `playbooks/<name>/` with `playbook.yaml`, profiles and one or
more `listing.<variant>.yaml` files, and doubles as the template a user copies.
C is dropped from v1 (D-011).

## A. `repo-audit` (dev_tools)

Buyer gives a public Git repository URL (and optionally a focus: security,
dependencies, tests, docs). The playbook clones shallowly, runs a structured
audit with a coding agent (findings with file and line, severity, fix
suggestions), checks its own claims against the tree, and writes a Markdown
report plus a `findings.json` file.

- Variants: `repo-audit-fast` (NEAR AI Cloud DeepSeek V4.1 Flash in a TEE,
  attested tier), `repo-audit-premium` (owner's Claude subscription, plain tier).
- Limits: repository up to 100 MB shallow, public only.
- Why: dev_tools is the second largest category and apb's home turf; the
  attested variant shows that code never left a TEE model, which matters for
  private repositories later.

## B. `doc-digest-private` (data, privacy track)

Buyer attaches up to 10 documents or images (PDF, PNG, JPEG, DOCX, up to 25 MB
total, up to 40 pages) and asks for a digest: summary, extracted entities, key
figures with source page references, a machine-readable `digest.json`, and a
list of questions the documents leave open. Pipeline, in this order:

1. Local conversion in the worktree: PDF and DOCX pages to PNG at a fixed
   resolution (no model involved; pixel and page limits enforced here).
2. Vision extraction: each page image as an image content part to
   `Qwen/Qwen3-VL-30B-A3B-Instruct` (no tools; one call per page, TEE).
3. PII classification of the extracted text through `/v1/privacy/classify`
   (`openai/privacy-filter` is not a chat model); categories are reported,
   values are never quoted in the digest.
4. Digest by `deepseek-ai/DeepSeek-V4.1-Flash-1017` or `Qwen/Qwen3.8-27B` (TEE).

Step 4 is the **signed step** (D-008): a trusted `script` node makes the one
Cloud API call itself, keeps the exact bytes and fetches the `provider_tee`
response signature, which goes into the deliverable's evidence block.

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
