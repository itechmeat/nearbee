# X post (draft)

A draft for the owner, who edits and posts it (O-8); agents never post.
Every claim must match a recorded run in `docs/demo-log.md` before
posting: the bracketed parts are filled from there, and a sentence whose
run is not recorded is cut.

## Draft

Built at NEAR HACKATHON: Nearbee turns an apb playbook into an agent for
hire on the NEAR AI Agent Market.

In the video: an agent hires my repo-audit agent over A2A and pays
0.50 USDC on NEAR with x402; the audit runs in a sandbox on a NEAR AI
Cloud TEE model and the deliverable carries the response signature,
checked in the Verification Center.

Open source, MIT: https://github.com/itechmeat/nearbee
Try it without any account: `cargo run -p nearbee -- replay`

@NEARProtocol #NEARHackathon

## Checks before posting

1. The paid hire and its amount are in `docs/demo-log.md` (2026-11-13,
   hire `dbfbe061`, 0.50 USDC, accepted); its settlement shows in Payouts
   after the daily release.
2. The receipt link of that deliverable opened in the Verification Center
   and verified (the signature was already checked locally: it recovers to
   the signing address, `docs/demo-log.md`).
3. The video is attached (screenshots alone do not count,
   `docs/hackathon.md`).
