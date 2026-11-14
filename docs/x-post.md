# X post (draft)

A draft for the owner, who edits and posts it (O-8); agents never post.
Every claim matches a recorded run in `docs/demo-log.md`; a sentence whose
run is not recorded, or not shown in the video, is cut before posting.
Draft of 2026-11-14. Lengths are counted as X counts them (a link is 23
characters).

## Post 1 (278 of 280 characters)

```text
Your AI agent works on your machine. Nobody else can hire it.

Built at NEAR HACKATHON: Nearbee puts it up for hire on the NEAR AI Agent Market, paid in USDC on NEAR, with private runs on NEAR AI Cloud.

https://github.com/itechmeat/nearbee
@NEARProtocol #NEARHackathon

What would you sell?
```

The video is attached to post 1.

## Post 2, a reply to post 1 (250 of 280 characters, optional)

```text
In the video, real paid x402 hires: a repo audit with a TEE-signed answer (0.50 USDC), a bug fixed with a failing test (2.00 USDC), npm audit from 29 vulnerable packages to 5 low (1.00 USDC).

Try it without an account: cargo run -p nearbee -- replay
```

## Checks before posting

1. Every hire in post 2 is in `docs/demo-log.md`: `bug-repro-fix` hire
   `d28caae2`, 2.00 USDC, accepted, with a failing test and the fix
   (2026-11-14); `vuln-deps-fix` hire `37008197`, 1.00 USDC, accepted,
   `npm audit` from 29 vulnerable packages to 5 low (2026-11-14);
   `repo-audit-fast` hire `dbfbe061`, 0.50 USDC, the response signature
   checked locally (2026-11-13) and its receipt link shown as verified in
   the NEAR AI Verification Center (2026-11-14).
2. Post 2 says "in the video": each of those hires is in the video
   (`docs/demo-script.md`); cut the ones that are not.
3. The video is attached (screenshots alone do not count,
   `docs/hackathon.md`).
4. The repository is public and the replay command runs on `main`.
