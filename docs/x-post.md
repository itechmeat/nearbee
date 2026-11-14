# X post (draft)

A draft for the owner, who edits and posts it (O-8); agents never post.
Every claim matches a recorded run in `docs/demo-log.md`; a sentence whose
run is not recorded, or not shown in the video, is cut before posting.
Draft of 2026-11-14. Lengths are counted as X counts them (a link is 23
characters).

## Post 1 (272 of 280 characters)

```text
Your automated routines sit idle while someone would pay for that job.

Built at NEAR HACKATHON: Nearbee lists apb playbooks as agents on the NEAR AI Agent Market, paid in USDC on NEAR.

https://github.com/itechmeat/nearbee
@NEARProtocol #NEARHackathon

What would you put up for hire?
```

## Post 2, a reply to post 1 (260 of 280 characters, optional)

```text
Paid x402 hires in the video: a bug fixed with a failing test (2.00 USDC), npm audit from 29 vulnerable packages to 5 low (1.00 USDC), a repo audit with a TEE-signed answer (0.50 USDC).

No account needed to see the hire machine: cargo run -p nearbee -- replay
```

## Checks before posting

1. Every hire in post 2 is in `docs/demo-log.md`: `bug-repro-fix` hire
   `d28caae2`, 2.00 USDC, accepted, with a failing test and the fix
   (2026-11-14); `vuln-deps-fix` hire `37008197`, 1.00 USDC, accepted,
   `npm audit` from 29 vulnerable packages to 5 low (2026-11-14);
   `repo-audit-fast` hire `dbfbe061`, 0.50 USDC, the response signature
   checked locally (2026-11-13).
2. Post 2 says "in the video": each of those hires is in the video
   (`docs/demo-script.md`); cut the ones that are not.
3. The video is attached (screenshots alone do not count,
   `docs/hackathon.md`).
4. The repository is public and the replay command runs on `main`.
