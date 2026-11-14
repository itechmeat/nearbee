# Proposal: headless Chromium in the sandbox image

## Why

A script step that builds a web page must render it to check it:
screenshots for a visual review and an accessibility pass (axe) need a
browser. The owner's new website agent does exactly that, and the sandbox
image has no browser, so its build step cannot pass inside the sandbox.

No brainstorm artifact: one pinned package in the image, scope fixed by the
owner's agent and the hackathon deadline.

## What Changes

- The sandbox image installs Playwright's Chromium and headless shell at one
  pinned Playwright version, with their system libraries, under a shared
  read-only `PLAYWRIGHT_BROWSERS_PATH` (`/opt/ms-playwright`).
- A static test checks the Dockerfile for the pinned version, the shared
  path and both browser builds.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `apb-sandbox`: a requirement for headless Chromium in the image.

## Impact

`sandbox/Dockerfile`, `crates/nearbee/tests/sandbox_image.rs`. The image
grows by the browser builds and their libraries. An agent that uses the
browser pins the same Playwright version in its own lock.
