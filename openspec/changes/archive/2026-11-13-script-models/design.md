# Design: script models

## 1. Listing field

`script_models: [claude/claude-opus-5-5, dsh/zai/glm-5.3-flash]`: a list of
strings `<agent>/<model>`, kept in order. `<agent>` is any CLI name of
lowercase letters, digits and `-` (amendment 2026-11-13, owner: agents must
not be tied to one CLI); `<model>` is non-empty, has no whitespace, and has
no further `/` for `claude` and `codex` (other agents may name a provider
route, `dsh/zai/glm-5.3-flash`).
A malformed or duplicated entry is refused naming it. Recorded with the
listing (`ResolvedListing.script_models`) and frozen with each generation's
statement snapshot, the way `direct_models` is.

## 2. Tier rule

`tier: attested` with a non-empty `script_models` is refused: "script_models
need tier plain: a script's own CLI call cannot be checked". Plain accepts
it. Nothing else in the tier check changes.

## 3. Facts and fingerprint

`ExecutionFacts` gains `script_models: Vec<String>` with
`#[serde(default, skip_serializing_if = "Vec::is_empty")]`, set by a
`with_script_models` builder next to `with_direct_models`, so facts without
it serialize byte for byte as before and old fingerprints hold. The drift
pass adds the record's list the same way it adds `direct_models`.

## 4. Statement

After the direct section, when the list is not empty:

```
### Script model calls

The playbook's script steps call these models through the owner's agent CLIs, as declared by the owner in the listing (Nearbee does not observe these calls):

- `claude`, model `claude-opus-5-5`: the owner's Claude subscription (Anthropic).
```

Each line reuses `model_line(agent, model, false)`: `claude` and `codex`
keep their subscription wording; `dsh` reads "`dsh` (DeepSeek Harness),
model `<model>`: through the owner's own provider account (`<provider>` from
the route when given)."; any other agent gets the generic line. The Models section's
existing text is unchanged; with an empty list nothing is added.

## 5. Run param

`script_models` joins `RESERVED_PARAMS`; `sent_params` inserts it as a JSON
list when the frozen list is not empty, exactly like `direct_models`;
everywhere `direct_models` is threaded from the listing record into a start
request (serve, replay), `script_models` follows.

## 6. Sandbox image

`sandbox/Dockerfile` installs `@anthropic-ai/claude-code@2.1.319` in the
existing `npm install -g` line and sets
`ENV DISABLE_AUTOUPDATER=1 CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`.
`api.anthropic.com` is already in `sandbox/egress.allow`. The token is
passed with `NEARBEE_APB_ENV_VARS="NEARAI_API_KEY CLAUDE_CODE_OAUTH_TOKEN"`
(docs only; the script already copies the named variables).

## 6a. DeepSeek Harness in the image (amendment)

`sandbox/Dockerfile` also installs `@deepseek-ai/dsh@0.2.2-rc.3` (the host
version) in the same npm line; `sandbox/egress.allow` adds `api.z.ai`. The
key reaches it as `ZAI_API_KEY` through `NEARBEE_APB_ENV_VARS`; an agent
brings its own dsh profile (no credentials plugin, provider key from the
environment). Codex is not used by the owner's agents (owner 2026-11-13),
but stays in the image for other owners.

## 7. Sign-in failure (docs)

`docs/privacy.md` states the rule: a script step whose model sign-in fails
(an expired or revoked token) exits non-zero; Nearbee then declines the hire
`failed`, which refunds the buyer in full (`docs/market.md`, decline). A
listing never switches to another provider.
