# Privacy tiers

A buyer's question is "where does my data go, and how do I know". The legal
frame (`docs/legal.md`) makes the owner responsible for every subprocessor and
makes the listing a contract. Nearbee answers the question with tiers it
validates, statements it generates, and evidence it attaches. The claims below
are deliberately no stronger than what the adapter can show.

## Tiers (as built in S4)

A listing file names its `tier`, `plain` or `attested` (required; no other
value parses). The tier is a checked property of the listing: `nearbee list`
refuses a listing whose execution facts break it, and `serve` stops taking
new work for a listing whose facts drifted.

| Tier | Who can read the buyer's data | Model steps | Other channels | In the deliverable |
| --- | --- | --- | --- | --- |
| **plain** | the owner's machine, the owner's model providers (any) | any profile | any connector, decisions and compaction allowed and named | the data statement |
| **attested** | the owner's machine, NEAR AI Cloud Confidential TEE models | every executor, fallback and tier of every usable profile runs `opencode` on a `nearai/<id>` model owned by `nearai` in Nearbee's dated catalog snapshot | no connector, decisions off, compaction off | the endpoint attestation disclosure, then the data statement |
| **attested, signed** (approved for `doc-digest-private`, D-008) | as attested | one bounded model step done by a trusted `script` node that calls the Cloud API itself (contract below, built in S5) | as attested | as attested, plus the response signature and the bytes' hashes |
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

### What the check refuses

Every tier: a `human_review` node, an interactive node, a node kind the
check does not know, a binding to another playbook, and any unknown fact the
statement needs, the decisions and compaction settings included (a `plain`
statement states them too).

`attested`, in addition:

1. A usable profile (the default profile and every node's profile, by its
   exact name and scope) that apb cannot resolve, or whose executor, a
   fallback or a tier is not the agent `opencode` (another agent on a TEE
   model id, such as `claude`, is refused too), or not on a `nearai/<id>`
   catalog model owned by `nearai`. Incognito gateway models (Claude, GPT,
   Gemini) and third-party TEE models are refused.
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

Compaction is read from the apb version: apb `1.0.0-beta.6` compacts only
when a run's `context_max_bytes` is set, and no REST, CLI or MCP surface
sets it, so the fact is "off" on that version and unknown on any other.

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
egress control; that is post-hackathon work.

### Repository audit: what apb enforces on the agent step

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
3. Where the run happens: the owner's machine, which can read the data; the
   agent steps run without a sandbox, so reading the owner's files and
   undeclared network access are not prevented.
4. What the market keeps: the brief, thread messages and uploaded files stay
   with the market under its own policy.
5. Local retention: the days after acceptance, and what deletion does not
   cover.

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
   fallbacks and tiers (for `repo-audit-fast`: DeepSeek V4.1 Flash and
   Qwen3.8-27B).
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

## Attested, signed (contract for S5)

Not built in S4; this is the contract the S5 step implements (D-008):
`nearbee signed-call --request <file> --out <dir>` sends the request file's
exact bytes with `stream: false` to `POST /v1/chat/completions`, stores the
response bytes before parsing them, then fetches
`GET /v1/signature/{completion id}?model={id}&signing_algo=ecdsa`. It
requires `signature_kind: "provider_tee"` and a signed text equal to
`model + ":" + sha256(request) + ":" + sha256(response)`. Only the signature
fetch is retried, never the completion; the step journals its own
operation state so a node retry resumes the signature fetch; it bounds
tokens and wall clock; and it writes manifest evidence entries (`kind:
tee_signature`): the bytes private, their hashes and the signature public.
Verifying the signature is not part of the step.

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

1. apb's run directories of the hire's run ids (apb `1.0.0-beta.6` has no run
   deletion, `docs/apb.md`);
2. apb's `.apb/decisions.jsonl` entries of the scope check (digests and
   metadata of the question, never deleted by Nearbee);
3. caches the executors wrote (agent CLI memory and session files).

Market playbooks run on Nearbee's own apb instance (separate config and data
directory, `docs/apb.md`), so nothing they produce reaches the operator's
everyday apb. apb itself learns from every run today with no switch to turn it
off (S1 finding); apb's `near-ai-cloud-models` change adds an `external_data`
run flag that excludes a run from every learning and reuse path (corrections,
known problems, stats and estimates, judge examples, scenario capture, run
memory) and keeps it visible on the run (D-023). Nearbee sets that flag on
every market run once apb ships it and checks it on the run detail; until then
only the owner's own test hires run. The flag does not cover an agent CLI's
own memory and session files: those are the executor caches above.

## What is honestly not guaranteed

1. In `plain` and `attested`, the owner's machine sees the data.
2. `attested` shows which model endpoints attest as TEEs and which models apb
   requested and the executors reported; it does not prove per response
   unless the signed path (S5) is used, and Nearbee does not verify the
   reports it collects.
3. Platform-held copies (brief, thread, files) are outside the owner's control.
4. Evidence depends on what the executor exposes; the signed path exists
   because general CLIs hide the bytes.
