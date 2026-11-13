# Privacy tiers

A buyer's question is "where does my data go, and how do I know". The legal
frame (`docs/legal.md`) makes the owner responsible for every subprocessor and
makes the listing a contract. Nearbee answers the question with tiers it
validates, statements it generates, and evidence it attaches. The claims below
are deliberately no stronger than what the adapter can show.

## Tiers (as built in S4, attested narrowed in S6)

A listing file names its `tier`, `plain` or `attested` (required; no other
value parses). The tier is a checked property of the listing: `nearbee list`
refuses a listing whose execution facts break it, and `serve` stops taking
new work for a listing whose facts drifted.

| Tier | Who can read the buyer's data | Model steps | Other channels | In the deliverable |
| --- | --- | --- | --- | --- |
| **plain** | the owner's machine, the owner's model providers (any) | any profile | any connector, decisions and compaction allowed and named | the data statement |
| **attested** | the owner's machine, NEAR AI Cloud Confidential TEE models | no agent at all (since S6, D-027): script nodes only, every model call a direct call of Nearbee's own commands to a model owned by `nearai` in Nearbee's dated catalog snapshot (the listing's `direct_models`) | no connector, decisions off, compaction off | the endpoint attestation disclosure, then the data statement |
| **attested, signed** (`doc-digest-private` since S5, `repo-audit-fast` since S6) | as attested | as attested, and one bounded model call signed by the model TEE (the listing's `signed_models`; contract below) | as attested | as attested, plus the response signature and the bytes' hashes |
| **blind** | NEAR AI Cloud TEE models only; the owner's machine relays ciphertext | one E2EE model step | no connectors; scripts touch only ciphertext | out of scope for the hackathon: a separate design (`docs/open-questions.md`) |

### The execution facts

The check reads facts from Nearbee's own apb instance (`Runner::inspect`,
`crates/nearbee-apb/src/inspect.rs`), never from a guess: the playbook's
nodes (kind, interactive flag, profile reference with its scope, connectors,
sub-playbook references), every profile a node can use (executor, ordered
fallbacks and tiers, environment, skills, process environment policy), the
model routes of the workspace's `opencode.json`, and the project's decisions
and compaction settings. A fact the configuration does not give (an absent
key, a 404, a file that does not parse, a malformed fallback or tier entry,
which makes its whole profile unknown) is unknown, and an unknown fact is
never read as safe. A failed read of the instance (a transport error, a
server error, a failed `systemctl`) fails the whole inspection instead, so a
transient error never changes the facts or their fingerprint.

One fact is read best effort instead: the sandbox (change statement-sandbox).
The inspection reads `systemctl --user show nearbee-apb -p ExecStart` and
reports a sandbox fact only when the unit's command line is a `docker run` of
a `nearbee-sandbox:<tag>` image with `--read-only`, `--cap-drop ALL`,
`--security-opt no-new-privileges`, `--dns 127.0.0.1` and a `--network`
other than `host`; its hosts are the names of every `--add-host`, sorted and
deduplicated. An option the parser does not know (`--privileged` and
`--cap-add` among them), any other command line or a failed read gives no
sandbox fact, never a failed inspection. The fact comes from what the unit
runs, not from a setting the owner types. Without it the serialized facts
and their fingerprint are what they were before the field existed; with it
the fingerprint moves, so a listing published before the instance was
sandboxed drifts until `nearbee list` runs again.

### What the check refuses

Every tier: a `human_review` node, an interactive node, a node kind the
check does not know, a binding to another playbook, and any unknown fact the
statement needs, the decisions and compaction settings included (a `plain`
statement states them too).

`attested`, in addition:

1. Any agent (since S6, "Attested tier without agents" below): an
   `agent_task` or `prompt` node, whatever its profile (a node with no
   profile and no default profile is refused naming the node), and any
   usable profile (the default profile and every node's profile, by its
   exact name and scope) whose executor, a fallback or a tier runs an
   agent. `opencode` is refused because it runs its sessions in a shared
   background service with the starting user's configuration (apb starts
   it without `--standalone`); any other agent because it is not a
   verified attested route. A profile apb cannot resolve is refused for
   every tier. The S4 model checks still run and name a non-TEE model
   (an incognito gateway model such as Claude, GPT or Gemini, a
   third-party TEE model, a model the snapshot does not list) when one is
   there.
2. A `nearai` provider whose base URL is not exactly
   `https://cloud-api.near.ai/v1`, whose `apiKey` is not exactly
   `{env:NEARAI_API_KEY}`, or that declares a model entry with any key
   beyond display and metadata (an `id` alias or `options` override could
   point a catalog id at a non-TEE model on the same base URL). The routes
   are unknown, and the listing is refused, when `opencode.json` is missing
   or unreadable, when the apb instance's environment sets any `OPENCODE_`
   variable (OpenCode reads those over its config files;
   `scripts/apb-instance.sh` passes none and refuses to start when one would
   reach apb), and when OpenCode would merge another configuration over the
   workspace file (next section).
3. Side channels: skills, an environment other than `minimal`, a process
   environment allowlist naming anything but `NEARAI_API_KEY`, a connector,
   decisions enabled or unknown, compaction enabled or unknown.

### OpenCode configuration

OpenCode merges configuration from several places; apb's opencode adapter
declares no sandbox needs and `environment: minimal` does not confine it (apb
1.0.0-beta.6, `crates/apb-core/src/agents/opencode/mod.rs`; apb's own
OpenCode MCP code writes to `$XDG_CONFIG_HOME/opencode/opencode.json`). Read
from the installed OpenCode 2.2.5, its config discovery takes, in order: the
system directory `/etc/opencode`, the global directory
`$XDG_CONFIG_HOME/opencode`, then, walking up from the working directory to
`/`, every `opencode.json`, `opencode.jsonc` and `.opencode/` (plugins,
agents, commands, MCP servers), plus `OPENCODE_CONFIG`,
`OPENCODE_CONFIG_DIR` and `OPENCODE_CONFIG_CONTENT`. Nearbee therefore:

1. starts the apb unit with `XDG_CONFIG_HOME` set to a directory it owns,
   `~/.local/share/nearbee/xdg-config` (created empty and `0700` by
   `scripts/apb-instance.sh start`, which refuses to start when it holds an
   `opencode` entry; `apb.xdg_config_home` in `nearbee.yaml` names the same
   path);
2. makes the routes unknown, refusing an attested listing, unless the unit's
   environment (`systemctl --user show nearbee-apb -p Environment`) carries
   that exact `XDG_CONFIG_HOME` and the directory holds no `opencode` entry;
   unless the workspace has no `.opencode/` directory; unless
   `/etc/opencode` does not exist; and unless no directory above the
   workspace holds `opencode.json`, `opencode.jsonc` or a `.opencode/` with
   more than OpenCode's own install files (the curl installer puts `bin/`,
   `node_modules/` and `package.json` in `~/.opencode`, which is allowed).

The fetched repository's own `opencode.json` and `.opencode/` are renamed
with `.repo-data` in the snapshot, so they are not configuration.

Since S6 an attested listing runs no OpenCode at all, so this section
matters only for a plain listing that uses OpenCode (none ships); its
checks stay as facts.

Not covered, and so not prevented: OpenCode 2.2.5 also loads instructions
and skills from `~/.claude` and `~/.agents` (and from `.claude/` and
`.agents/` directories above the working directory) whatever
`XDG_CONFIG_HOME` says; only a separate `HOME` for the unit would hide them,
and that would also hide the Claude login the plain premium listing uses. On
a machine where those directories hold skills, an attested run can load
them. Other programs in the unit also read `XDG_CONFIG_HOME` (git looks for
`$XDG_CONFIG_HOME/git/config`), so a setting kept only there does not apply
to runs.

Decisions are off for the whole apb project (`scripts/apb-instance.sh
install` writes `decisions: { enabled: false }` into its `.apb/config.yaml`).
The consequence: the intake scope check goes unanswered and is skipped for
every listing while decisions are off (the existing behaviour of
`crates/nearbee-core/src/intake.rs`). For agent A the deterministic intake
check `github_repo` replaces it: a brief must name exactly one public GitHub
repository by its `https://github.com` URL, else the hire is declined
`rejected` before anything else happens.

Compaction is read from the apb version: apb compacts only when a run's
`context_max_bytes` is set, and at `1.0.0-beta.6` and `1.0.0-beta.7` no
REST, CLI or MCP surface sets it (re-read for beta.7 on 2026-11-11,
`docs/apb.md`), so the fact is "off" on those two versions and unknown on
any other, which refuses every tier until that release is read too.

### Attested tier without agents (S6, D-027)

The S4 attested route was an agent: `opencode` on NEAR AI Cloud TEE
models, with the configuration checks above. A probe on 2026-11-11
(OpenCode 2.2.5, `docs/apb.md`, "apb 1.0.0-beta.7") showed that OpenCode
2.x, started as apb starts it (without `--standalone`), hands the session
to a shared background service that runs with the environment of whoever
started it: on this machine the owner's home and configuration, with the
owner's MCP server connected. With `--standalone` it still loads
`~/.agents/skills` and `~/.opencode/skills`. Nearbee cannot fix that from
its side, and apb's native `nearai/` route on `dsh` reads the same home
sources. So an attested listing now runs no agent: its playbook has script
nodes only, and its model calls are the direct and signed calls of
Nearbee's own commands (`nearbee digest`, `nearbee audit review`), which
send exact bytes to the models the listing names and nothing else. The
tier check refuses any agent node and any agent profile; the fast
repository audit moved to its own playbook for this
(`playbooks/repo-audit/playbook.fast.yaml`, `docs/showcase-agents.md`).
The owner's shared OpenCode service stays as it is; Nearbee only stops
relying on it.

### When the check runs

1. `nearbee list`: on the materialised source before installing, and again
   on the installed version read back from apb; the two fingerprints must be
   equal. A refusal leaves the previous listing record unchanged and exits
   non-zero.
2. `serve`: once per round for every listing, before intake or any start
   can send buyer data (the drift pass). A drifted listing (facts read, and
   the fingerprint, the statement or the tier check do not match) takes no
   new work: a hire whose current generation has no `run_start` intent is
   declined at once (`rejected` when first-time and not started on the
   market, else `failed`) with attention `listing_drift`. A listing whose
   facts cannot be read this round is held: its hires without a
   `run_start` intent get no intake check, scope check, market start or apb
   start and are not declined, and the waiting-past-deadline rule declines
   them if the hold lasts. A hire whose run already started is still
   observed and delivered with the statement frozen at its start. A record
   written before S4 has no tier and is drifted until the listing is listed
   again; a record that is not serveable never starts a generation. The
   operator is told about a drift once, and again after the listing was
   clean in between.

### What the check cannot prove, said plainly

The check covers declared bindings only. apb's `environment` is not a
sandbox: an agent CLI or a script can call any address with any local
credential, and no apb profile setting confines file access. Egress the
bindings do not declare, and an agent step reading the owner's files, are
not prevented, and every statement says so ("The agent steps run without a
sandbox"). An enforced boundary needs a dedicated execution environment with
egress control.

Since the change apb-sandbox the owner can run Nearbee's apb instance in a
container (`NEARBEE_APB_SANDBOX=1`, `docs/apb.md`, "Sandboxed instance"):
the steps then see only the workspace, the instance config directory, a
dedicated home and named read-only paths, never the owner's home, keys or
`.env`, and reach only the hosts of an allowlist, by TLS server name. What it
does not do: hires share one container (worktrees keep them apart, the
sandbox home and parts of the config directory are shared), the kernel is
the host's, an allowed host that accepts uploads stays a channel out, and a
host behind a shared CDN allows domain fronting. Since the change
statement-sandbox the data statement says so: the sandbox is an execution
fact read from the unit's command line ("The execution facts"), and the
statement's "Where the run happens" names the container, its limits and the
allowed hosts ("The data statement").

### Repository audit: what apb enforces on the agent step

Since S6 only the premium listing (`repo-audit-premium`, plain, `claude`)
runs this agent step. The fast listing runs `playbook.fast.yaml`, whose
`review` step is `nearbee audit review`: a script node that packs the
snapshot as labelled data and makes one signed call, so no agent reads
the repository and nothing below applies to it (next section).

The `audit` node of `playbooks/repo-audit/playbook.yaml` reads
attacker-chosen content (any buyer names the repository). Read at apb
1.0.0-beta.6 (`crates/apb-core/src/schema.rs`, `docs/ISOLATION.md`), the
strongest restrictions apb enforces for it are declared:

1. `writes: ["audit/**"]` and `max_changed_files: 1`: after every attempt
   apb compares the tree and fails the node, without a retry, when the
   attempt changed anything outside `audit/` or more than one file.
2. `protect: ["audit-snapshot/**"]`: apb snapshots the snapshot before every
   attempt, restores it and fails the attempt on a change. Paths git
   ignores are not covered (a `.gitignore` inside the fetched repository
   can hide its own files), so `nearbee audit finish` also checks every
   cited file against the SHA-256 that `fetch` recorded in `source.json`
   and rejects the finding with `file changed after fetch`.

What apb cannot enforce for this node, and so is not prevented:

1. A sandbox. `isolation: full` refuses an `opencode` step at the tag
   (`agent_needs_unknown`: opencode declares no sandbox needs), and
   `best_effort` would run it without a sandbox in a scratch directory that
   lacks the snapshot. The agent therefore runs with the owner's account:
   it can read the owner's files (the market tokens in the credentials
   store, `~/.ssh`) and reach any network address. The process environment
   allowlist keeps the environment to `NEARAI_API_KEY` and the base set
   (it is not a file or network boundary).
2. Prompt injection from repository files. The prompt treats them as data,
   instruction files are renamed, and the write scope and the hash check
   bound what an injected agent can change, but not what it can read or
   send.

### Repository audit, fast variant: one signed review call (S6)

`nearbee audit review` reads only the snapshot's listed files whose bytes
still match the SHA-256 that `fetch` recorded and that are valid UTF-8,
packs them in a fixed order (manifests and build files, source, config,
tests, docs, other files; lock files, minified files, source maps and
files under `dist/`, `build/`, `vendor/` or `node_modules/` last; ties by
path) with numbered lines and JSON-quoted paths in the file headers, and
omits every file that would take the request past 256 KiB while packing
the smaller ones after it. The omitted files are counted per reason in
`audit/review-coverage.json` (with at most 1,000 paths) and listed in the
deliverable's "Not reported" section (at most 50 paths, grouped by
reason). A pack with no manifest, source, config or test file fails the
step with `nothing_packed` before any request. The request carries the model, the two
messages, `stream: false`, `max_tokens: 16384`,
`chat_template_kwargs: {"thinking": false}` and
`response_format: {"type": "json_object"}`, nothing else.
`chat_template_kwargs` turns DeepSeek's reasoning off: with it on, the
model reasoned past the signed call's bound without answering, even for
a 20-file repository (measured 2026-11-12). `response_format` asks for a
JSON object answer, and the 256 KiB cap keeps the pack small enough for
the model to work on: on a pack three times that size (213,396 prompt
tokens) it answered with no findings, and on a smaller pack without
`response_format` it answered invalid JSON (measured 2026-11-13). The
digest request carries both keys. The brief, the buyer's
change request and the files are labelled data blocks under a system
message that forbids following them; the brief's and the change request's
lines are numbered like the files', so a buyer cannot close a block. The model must be in the hire's
frozen `direct_models`, else nothing is sent. The call is the S5 signed
call (state in `audit/signed/<execution_id>/`, a rework makes a new call),
and `finish` copies the four evidence files into `out/evidence/` (only
for a run with direct models, so never for the premium variant), so the
deliverable carries the response signature section and its receipt link.
An answer that is not the candidates object, or has more than 40
findings, fails the step with `review_unparsed` and the hire is declined
`failed`. Every citation is still checked against the snapshot by
`finish`. What it gives up: one packed call reviews less than an agent
that reads files on demand, and a large repository is cut and says so.

Review notes (change `sandbox-toolchains`, 2026-11-13). The fast review
of the owner's agents now makes several direct calls: review lenses on
DeepSeek V4.1 Flash and a verdict on Qwen3.8 27B, and only the verdict
call is signed (D-027 as amended). Facts the findings cannot carry, such
as a lens that failed or findings the verifier dropped, travel as
`notes` in `audit/candidates.json` (at most 10 strings of at most 300
characters, more or longer fails `finish` with no `out/result.json`).
`finish` renders them, in order and escaped like every other untrusted
text, one list item per note with line breaks turned into spaces, under
"Review notes" before "Not reported". Without notes, or with an empty
list, the deliverable is byte for byte as before.

A further limitation (live-audit finding b): the deliverable's secret scan
matches configured secrets and `aat_` tokens as plain text only; an
encoded (base64, hex) or split secret passes it. No scanner stops a
deliberate encoding; the sandbox gap above is the real exposure.

## The data statement

Generated from the same facts, never typed (`crates/nearbee-core/src/statement.rs`,
format 1). Under `## Data handling` it names, in this order:

1. Models: each executor in order. `claude` is "the owner's Claude
   subscription (Anthropic)", `codex` "the owner's OpenAI subscription",
   `opencode` on a checked `nearai/` model "NEAR AI Cloud Confidential TEE
   model `<id>`", anything else by agent and model.
2. Other channels: connectors by name, decisions on or off, compaction on or
   off (an unknown setting is stated as unknown without its reason, so no
   machine path reaches the public bio); when any is on, that these
   channels may send data to providers Nearbee has not validated.
3. Where the run happens: the owner's machine, which can read the data
   (an attested listing first says its model steps are configured to run on
   NEAR AI Cloud Confidential TEE models). The rest depends on the sandbox
   fact. Without one the text is unchanged since S4:

   > The run happens on the owner's self-hosted machine, which can read the
   > data. The steps run without a sandbox: reading files the owner's
   > account can read and network access the bindings above do not declare
   > are not prevented.

   With one (change statement-sandbox), the hosts in backticks, sorted:

   > The run happens on the owner's self-hosted machine, which can read the
   > data. The steps run in a container on that machine: a read-only
   > system, no added privileges, and only the instance's own directories
   > mounted (not the owner's home or keys). Outbound connections are
   > limited to: `cloud-api.near.ai`, `github.com`. Hires of every listing
   > on that machine share the container and are kept apart by working
   > directories only.

   A sandbox fact with no `--add-host` says "limited to: none".
4. What the market keeps: the brief, thread messages and uploaded files stay
   with the market under its own policy; a file once used by a job cannot
   be deleted there, and its link works for anyone holding it.
5. Local retention: the days after acceptance, and what deletion does not
   cover.

Output files (change `sandbox-toolchains`, design 6): the files a run's
manifest lists beside the Markdown are uploaded to the market with a public
link and listed, linked, in the deliverable's "Files" section, so the
market keeps them as section 4 says. Before any upload they pass the same
secret scan as the Markdown (a hit declines the hire), and at most 20 files,
10 MiB each and 50 MiB in all are delivered. An upload the delivery does not
use (a declined delivery, a refused submit) is deleted at once; one a crash
left unbound is deleted by the market 7 days after its upload. The journal
keeps each file's name, size, SHA-256, market file id and link, never its
bytes.

`nearbee list` prints the statement for the agent's bio in the market web
app (an agent token cannot write it, D-024) and records it with its SHA-256,
its format and the fingerprint of the facts (`sha256:` of the tier and the
facts, fallbacks and tiers in their routing order). Every generation
snapshots the statement, the fingerprint and the evidence model set
(`statement.json`) before its market start and its apb start; delivery uses
that snapshot, never the current record, and appends `---` and the
statement to the deliverable. A generation journaled before S4 delivers
without the footer.

## Evidence (attested tier, as built)

1. Which models: the frozen evidence model set of the generation, the
   distinct `nearai` model ids of every usable profile's executor,
   fallbacks and tiers, and since S5 the listing's direct models (for
   `repo-audit-fast` since S6: DeepSeek V4.1 Flash only; for
   `doc-digest-private`: Qwen3-VL, the privacy filter and DeepSeek).
2. When: one report per model before the generation's market start and apb
   start, and one after its run succeeded, from
   `GET https://cloud-api.near.ai/v1/attestation/report?model=<id>&signing_algo=ecdsa&nonce=<64 hex>`
   with the bearer `NEARAI_API_KEY` (`crates/nearbee-cloud`): no redirects,
   a 2 MiB cap, two attempts within 20 seconds. One fetch per serve round,
   as a long step, never when another hire's deadline is nearer than the
   fetch bound.
3. Stored privately: the raw bytes, nonce, model, fetch time and SHA-256 as
   a snapshot file of the generation outside the worktree, then an
   `evidence_collected` journal record; a failure is journaled as
   `evidence_failed` with a short redacted reason. The key is never stored
   or logged.
4. Fail closed: the third failed fetch of a phase declines the hire
   (`rejected` when first-time and not started on the market, else
   `failed`, attention `evidence_unavailable`). A start report lost after
   the `run_start` intent is never fetched again: the hire is declined
   `failed` with attention `evidence_missing`. An attested deliverable is
   never submitted without both collections.
5. Disclosed: the deliverable carries, before the statement, an
   `Endpoint attestation disclosure` section: per model the id, the start
   and end report SHA-256 and fetch times and the Verification Center link
   `https://cloud.near.ai/verify?model=<URL-encoded id>`; per node attempt
   of the delivered run the model apb requested (or `not recorded`) and the
   reported model, `none reported` or `mixed`, never the profile's model in
   its place.

The reports are collected, not verified: the section says that Nearbee did
not verify them and that they show which endpoints attest as TEEs without
binding this job's responses to a TEE.

Journal hashes: `<state_dir>/hires/<id>/journal.jsonl` holds hashes of every
input, output and deletion with timestamps. Anchoring on NEAR is a stretch.

## Direct models and the signed call (as built in S5)

A listing may declare `direct_models`: the NEAR AI Cloud models Nearbee's
own commands call from the playbook's script nodes, without an agent
(D-026). Agent B (`doc-digest-private`) declares
`Qwen/Qwen3-VL-30B-A3B-Instruct` (page transcription),
`openai/privacy-filter` (personal data counts) and
`deepseek-ai/DeepSeek-V4.1-Flash-1017` (the digest), and signs the digest
call (`signed_models`).

1. Tier check: an attested listing refuses a direct model the catalog
   snapshot does not list with owner `nearai`; a plain listing refuses one
   absent from the snapshot. The model routes count as unknown, so an
   attested listing is refused and the drift pass reports it, when the apb
   instance environment sets `NEARAI_CLOUD_URL`, any `NEARBEE_CLOUD_`
   variable or a proxy variable (`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`,
   any case); `scripts/apb-instance.sh start` refuses the same names. The
   commands call only `https://cloud-api.near.ai/v1`, with no system proxy
   and no redirect.
2. Statement and evidence: the statement lists every direct model in a
   "Direct model calls" section and names the signed step; the
   fingerprint covers the lists in order (a listing without them keeps
   its S4 fingerprint); the evidence model set adds the direct models, so
   attestation reports are collected for them too. The lists are frozen
   with the generation's statement snapshot and reach the run as the
   `direct_models` param; a command refuses any model outside the frozen
   list before sending.
3. The signed call: `nearbee signed-call` sends the request file's exact
   bytes (`stream: false`, `max_tokens` at most 16,384, at most 1 MiB)
   with `x-no-aliasing: true` and `Accept-Encoding: identity`; the
   review and digest requests also carry
   `chat_template_kwargs: {"thinking": false}`, because the model's
   reasoning otherwise outruns the bound (measured 2026-11-12), and
   `response_format: {"type": "json_object"}`, because without it the
   model answered invalid JSON (measured 2026-11-13). One
   invocation is bounded by 360 s; the completion waits for the time left
   in that bound minus a 30 s signature reserve (no shorter fixed
   timeout), and fails before sending when nothing is left. It sends only
   after it
   stored a fresh model attestation report (`provider=near`) with a
   non-empty `model_attestations` in which at least one attestation
   carries a signing address. It keeps the response bytes before
   parsing them, sends the completion at most once per state directory
   (no idempotency key exists; a lost answer fails the hire rather than
   paying twice), refuses a compressed or cut answer, and fetches
   `GET /v1/signature/{id}?model=..&signing_algo=ecdsa` for up to 60 s. It
   accepts only `signature_kind: provider_tee`, a signed text equal to
   `model:sha256(request):sha256(response)` and a signing address that
   matches at least one model attestation of the preflight report (case
   insensitive; instances of one model share one key). It writes
   `signature.json` (public) and `request.bin`, `response.bin`,
   `report.json` (private) as manifest evidence entries.
4. Delivery: serve rechecks the entry from the validated bytes (hashes,
   signed text, `provider_tee`, a frozen direct model, every field's
   format) and declines `failed` with attention `signature_mismatch`, or
   `signature_missing` when a signed listing's run has no entry. A
   statement snapshot that cannot be read or parsed declines `failed`
   with `statement_missing` (at the run start too), so it never reads as
   a generation without signed models. A valid
   entry adds a `Response signature` section before the evidence section:
   every field in a code span and a receipt link
   `https://cloud.near.ai/verify?model=<id>#receipt=<base64url JSON>` the
   buyer opens in the Verification Center. The two private files are
   snapshotted with the generation when the submit intent opens and are
   never delivered.

What the buyer can verify: that a NEAR AI Cloud model TEE with the shown
signing address signed a response with the shown hash for the shown
request hash. What the buyer cannot verify: the hashes themselves, because
the request and response bytes stay with the owner (the response may
carry the model's reasoning, which quotes the documents beyond the
checked digest), nor that the deliverable was derived from that response;
the section says so. Nearbee does not verify the signature itself.

Personal data in the digest: the digest's own text (summary, entity
values, figure values and labels, open questions) goes through
`openai/privacy-filter` once more before it is written; every span is
replaced by `[redacted: <category>]` and a figure whose value held one is
dropped and counted. This holds whatever the prompt or an injected page
instruction made the model write; it is as good as the classifier.

Cost: the direct calls are billed to the owner's NEAR AI Cloud key outside
apb, so the spent-cost figure and the daily loss limit do not include them
yet; each execution records their token usage in
`digest/<execution_id>/usage.json` (worst case under 0.20 USD per hire,
design 8).

## Script models (change `script-models`)

A playbook's script steps may call a model through the owner's agent CLI
(ux-flow runs `claude -p` on the owner's Claude subscription). Nearbee does
not see such a call, so the listing declares it:
`script_models: [claude/claude-opus-5-5, dsh/zai/glm-5.3-flash]`, a list
of `<agent>/<model>` entries kept in order. `<agent>` is any agent CLI
name of lowercase letters, digits and `-` (amendment 2026-11-13: agents
are not tied to one CLI); `<model>` is non-empty without whitespace, and
without a further `/` for `claude` and `codex` (other agents may name a
provider route). A malformed or duplicated entry is refused naming it.

1. Tier: plain only. An attested listing with any entry is refused with
   "script_models need tier plain: a script's own CLI call cannot be
   checked".
2. Statement: after the direct section the statement adds, word for word:

   ```
   ### Script model calls

   The playbook's script steps call these models through the owner's agent CLIs, as declared by the owner in the listing (Nearbee does not observe these calls):

   - `claude`, model `claude-opus-5-5`: the owner's Claude subscription (Anthropic).
   ```

   Each line uses the Models section's per-agent wording. A `dsh` entry
   reads, for `dsh/zai/glm-5.3-flash`:

   ```
   - `dsh` (DeepSeek Harness), model `zai/glm-5.3-flash`: through the owner's own provider account `zai`.
   ```

   (without a route, the line ends at "provider account."). An `opencode`
   entry, and any agent without its own wording, gets the generic "agent
   `<agent>`, model ..." line, never the checked TEE wording. The fingerprint covers the list in order; a
   listing without it keeps its statement and fingerprint byte for byte.
   Script models are not endpoint evidence: Nearbee never calls them.
3. Run param: the list is frozen with the generation's statement snapshot
   and reaches the run as the reserved `script_models` param (a JSON list,
   like `direct_models`), so a script can refuse a model the listing did
   not declare.
4. Account (change `sandbox-toolchains`, owner 2026-11-13): a listing may
   say which account an agent CLI of `script_models` uses,
   `script_accounts: {claude: anthropic-api}`. The kinds are closed: for
   `claude`, `subscription` (the default when absent) and `anthropic-api`;
   no other agent has kinds yet. An agent key not used by `script_models`,
   an agent without kinds or an unknown kind is refused naming it. The
   accounts are recorded and frozen with the listing like `script_models`.
   With `anthropic-api` the statement's `claude` lines read:

   ```
   - `claude`, model `claude-opus-5-5`: the owner's Anthropic API account (API key).
   ```

   The subscription wording is unchanged. Only a non-default kind enters
   the fingerprint, so a listing without the field, or with `claude:
   subscription`, keeps its statement and fingerprint byte for byte. The
   owner's reading (2026-11-13): API-key calls are within the Anthropic
   API terms; after the hackathon these calls move to Anthropic models
   through NEAR AI Cloud. The script step gets the key through
   `NEARBEE_APB_ENV_VARS` (`docs/apb.md`).
5. Sign-in failure: a script step whose model sign-in fails (an expired or
   revoked token) exits non-zero; Nearbee then declines the hire `failed`,
   which refunds the buyer in full (`docs/market.md`, decline). A listing
   never switches to another provider.

## Deletion inventory (as built in S3)

`nearbee hires` lists every journaled hire (read only, no lock): hire id,
agent handle, state, latest run, spent cost, deadline and an attention code;
never the brief text, a token or a capability link (`--json` for the same as
JSON).

`nearbee cleanup` takes the serve lock (`<state_dir>/serve.lock`, exit 4 while
a serve or another cleanup holds it) and deletes, for each eligible hire, only
the paths Nearbee owns:

1. the hire's worktree under `<workspace>/work/` (the buyer's `inputs/`, the
   run's `out/`, anything else the run wrote there): every worktree the hire's
   run intents journaled (an S2 hire's per-execution path included) and the
   per-hire default;
2. the hire's snapshot directories `<state_dir>/hires/<hire>/gen-*` (frozen
   submissions, run request bodies, the previous generations' `out/`).

A journaled worktree outside `<workspace>/work/` after canonicalisation is
never touched and is reported as refused. The hire journal stays (hashes,
timestamps, states) and gets a `deleted` record listing every path removed,
already absent, failed or refused, and what is not covered. A path counts
as removed or absent only once the directory that held it is synced, and
snapshot directories that cannot be listed are a failure, never an empty
list. A hire with a failed path stays eligible and the next cleanup retries
it.

Eligibility (`retention.after_acceptance_days`, default 7):

1. closed `accepted`: that many days after `finalizedAt` (else after the
   acceptance was first observed); acceptance is never inferred from
   `autoAcceptAt`;
2. closed `cancelled` with no delivery, a decline at intake included: at once;
3. closed `cancelled` after a delivery, or `dispute_resolved`: that many days
   after the closing was first observed;
4. never: a frozen (disputed) hire, a hire with an open intent, a hire still
   open or delivered and not yet closed.

`nearbee cleanup --dry-run` prints the same plan, takes no lock and changes
nothing.

Not covered, listed by name in every `deleted` record and in the dry run:

1. apb's run directories of the hire's run ids (apb `1.0.0-beta.6` and
   `1.0.0-beta.7` have no run deletion, `docs/apb.md`);
2. apb's `.apb/decisions.jsonl` entries of the scope check (digests and
   metadata of the question, never deleted by Nearbee);
3. caches the executors wrote (agent CLI memory and session files).

Market playbooks run on Nearbee's own apb instance (separate config and data
directory, `docs/apb.md`), so nothing they produce reaches the operator's
everyday apb. apb itself learns from every run today with no switch to turn it
off (S1 finding); apb's `near-ai-cloud-models` change adds an `external_data`
run flag that excludes a run from every learning and reuse path (corrections,
known problems, stats and estimates, judge examples, scenario capture, run
memory) and keeps it visible on the run (D-023). It shipped in apb
`1.0.0-beta.7`; Nearbee uses it since S6 (next section). The flag does not
cover an agent CLI's own memory and session files: those are the executor
caches above.

## External-data runs (as built in S6)

1. Before any market start of a new first-time hire, serve asks the apb
   instance whether it takes external-data runs (its version is at least
   `1.0.0-beta.7`). An older apb declines the hire `rejected` within the
   first hour with "the agent cannot take hires right now" and the
   attention `apb_too_old`; an unreadable answer writes nothing and is
   asked again next round.
2. Before every run start (first run, rework, redo) serve asks again; no
   or an error starts nothing, the hire keeps its state with `apb_too_old`
   and its deadline declines it later; an unreadable answer is told to
   the operator once per hire. Every run serve starts carries
   `external_data: true`, and its run start intent records it.
3. The incident path: on every observed run detail of a market run whose
   start intent recorded the flag (the first observation, reworks and
   redos included) serve checks that the run reports the flag. A run
   started before the intent recorded it (a hire in flight across the
   upgrade) is not checked, so the upgrade does not abort it. When it does not, buyer data has reached apb's
   learning state: serve aborts the run, journals `privacy_flag_missing`
   with the run id, declines the hire `failed` with "the run could not be
   isolated" and raises the operator attention `privacy_incident`. The
   owner then clears what apb learned from that run by hand (its
   corrections, memory and examples of the playbook); Nearbee cannot.
4. Earlier unflagged runs: the owner's own test hires and smoke runs
   before S6 ran without the flag, so apb may have learned from them.
   They carried synthetic briefs only; the flag does not undo what apb
   learned then.
5. What the flag does not reach: what an agent CLI keeps itself. The
   premium repository audit runs Claude Code, which keeps its session
   files under the owner's home; the data statement says so for every
   listing with a `claude` executor, and says that no agent session files
   exist for a listing without agents.
6. `nearbee config check` prints the apb version with `ok` from
   `1.0.0-beta.7` on and `failed`, naming the minimum, below it.

## What is honestly not guaranteed

1. In `plain` and `attested`, the owner's machine sees the data.
2. `attested` shows which model endpoints attest as TEEs and which models apb
   requested and the executors reported; it does not prove per response
   unless the signed path (S5) is used, and Nearbee does not verify the
   reports it collects.
3. Platform-held copies (brief, thread, files) are outside the owner's control.
4. Evidence depends on what the executor exposes; the signed path exists
   because general CLIs hide the bytes.
5. The optional sandbox (`docs/apb.md`, "Sandboxed instance") limits file
   access and egress per instance, not per hire, and filters egress by TLS
   server name only. The statement names the sandbox only when the unit's
   command line shows it; without it the statement says the steps run
   without a sandbox, and with it that hires share the container and are
   kept apart by working directories only.
