# Brainstorm

## Problem

Every step of a market hire runs on the owner's machine with the owner's
rights: a script node or an agent CLI can read the home directory, SSH
keys and `.env`, and reach any host. A buyer's brief is untrusted. The
owner asked for the runs to execute in a container that still offers the
usual tools (apb, nearbee, OCR, OpenSpec, Code Ranker, agent CLIs), with
outbound traffic limited to named hosts.

## Related work

The data statement (PR "Statement: the steps run without a sandbox") says
the steps run without a sandbox; O-11 in `docs/owner-checklist.md` names a
dedicated execution environment as the way to an enforced boundary. The
change `model-thinking-off` made both example playbooks finish live; they
were proven in a hand-run container on 2026-11-12 (`docs/demo-log.md`).

## Doubts

1. Nearbee's own HTTP clients are built with `no_proxy()`, and the tier
   check refuses proxy variables in the instance environment, so an HTTP
   proxy cannot carry Nearbee's direct model calls.
2. The tier check reads the instance environment from
   `systemctl --user show nearbee-apb -p Environment`; the container's
   environment must match what that shows.
3. apb refuses a non-loopback bind without a server key.
4. The apb CLI calls Nearbee makes (`APB_BIN`) need the container's view
   of the config directory.

## Approaches considered

1. Container per hire started by Nearbee (author). Strongest isolation
   between hires, but it needs a new runner backend; too large for the
   remaining window.
2. One long-lived apb instance inside a container, behind an egress
   gateway (author, measured). Nearbee keeps talking REST to
   `127.0.0.1:7322`; only the publish install's temporary files move (review finding 1).
3. HTTP proxy with an allowlist (research: Squid on an internal network).
   Rejected for Nearbee's clients by doubt 1.
4. SNI allowlist gateway (measured 2026-11-12): the container sits on an
   internal Docker network with no route out and no DNS; `/etc/hosts` maps
   each allowed host to an nginx `stream` gateway that reads the TLS server
   name and passes the connection through unchanged (TLS stays end to end,
   so model TEE TLS is untouched). Measured: `cloud-api.near.ai` and
   `api.github.com` answered 200; `example.com` had no DNS and `1.1.1.1`
   no route.

## Decision

Approach 2 with the gateway of approach 4, inside `scripts/apb-instance.sh`
as an opt-in mode, plus an image recipe under `sandbox/`. Hires of one
instance share the container (separate worktrees); per-hire containers stay
a later change.

## Rejected

1. Per-hire containers now: needs a runner backend, out of time.
2. HTTP proxy variables: bypassed by `no_proxy()` and refused by the tier
   check.
3. Mounting host tool directories: they live under the home directory next
   to their credentials; tools are installed in the image instead, and
   self-contained agent CLI binaries are copied in at build time.

## Process

Author: Claude. The research input came from a separate read-only agent
(Reddit, Hugging Face, the Anthropic, OpenAI, Docker and OpenHands docs);
the gateway and the earlier container runs were measured on the owner's
machine.
