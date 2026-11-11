# Repository audit: example/webhook-demo

Repository https://github.com/example/webhook-demo at commit 1111111111111111111111111111111111111111.

1 findings are location checked, 0 candidate findings were rejected, 0 files were skipped.

Location checked means the cited file, lines and excerpt exist in the snapshot; it does not prove that an issue is real.

## Medium

### 1. Webhook signature compared with a plain string comparison

File src/webhook.rs, lines 12 to 12.

> if header == expected {

The signature check returns as soon as one byte differs, so the time it takes reveals how much of a forged signature is right.

Fix: Compare the two values in constant time, for example with the subtle crate.

## Not reported

0 candidate findings were rejected by the location check. 0 archive members were skipped.

This deliverable is a replay fixture: no repository was fetched and no model was called.
