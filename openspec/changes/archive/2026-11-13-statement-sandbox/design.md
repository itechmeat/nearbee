# Design: statement sandbox

1. `nearbee-core::tiers`: `pub struct SandboxFact { pub allowed_hosts:
   Vec<String> }`; `ExecutionFacts` gets `#[serde(default,
   skip_serializing_if = "Option::is_none")] pub sandbox:
   Option<SandboxFact>`, so the serialized facts and the fingerprint are
   unchanged without it. Every constructor sets `None` unless inspection
   finds one.
2. `nearbee-apb::inspect`: a function that runs `systemctl --user show
   <unit> -p ExecStart` the way `instance_env` runs `-p Environment` (same
   binary, unit name and timeout), takes the `argv[]=` part up to ` ;`,
   splits it on spaces (the sandbox argv has no quoted spaces; anything
   unparsable means no fact) and applies the spec rule. A pure parser
   function is unit-tested on the measured argv shape; the systemctl call is
   best effort: an error gives `None`, never a failed inspection.
3. `nearbee-core::statement::run_place_section(tier, sandbox)`: without a
   fact the text is unchanged byte for byte. With one:

   "The run happens on the owner's self-hosted machine, which can read the
   data. The steps run in a container on that machine: a read-only system,
   no added privileges, and only the instance's own directories mounted
   (not the owner's home or keys). Outbound connections are limited to:
   `host1`, `host2`. Hires of every listing on that machine share the
   container and are kept apart by working directories only."

   Hosts in backticks, sorted, comma separated; no em or en dashes.
4. Drift: the drift pass compares fingerprints as today; a sandboxed
   instance changes the fingerprint of an already published listing, which
   is the intended signal to run `nearbee list` again.
5. Docs: `docs/privacy.md` (the statement section and "What is honestly not
   guaranteed") gets the two variants.
6. Live check (integrator): `nearbee list` of the fast listing against the
   sandboxed instance prints the new section; the owner pastes it.
