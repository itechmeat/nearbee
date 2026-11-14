## 1. Image

- [x] 1.1 Install Playwright 1.64.1 Chromium and headless shell with `--with-deps` under `PLAYWRIGHT_BROWSERS_PATH=/opt/ms-playwright` in `sandbox/Dockerfile`
- [x] 1.2 Static test for the pinned version, the shared path and both builds in `crates/nearbee/tests/sandbox_image.rs`
- [x] 1.3 Rebuild the image and launch Chromium inside it (integrator)
