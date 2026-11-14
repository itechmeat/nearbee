## ADDED Requirements

### Requirement: Headless Chromium in the image
The sandbox image SHALL contain Playwright's Chromium and its headless shell at one pinned Playwright version, with their system libraries, readable by every user under the path `PLAYWRIGHT_BROWSERS_PATH` names, so a script step can render pages for screenshots and accessibility checks without a download.

#### Scenario: Browser present at the shared path
- **WHEN** a script step in the sandbox launches Chromium through the pinned Playwright version
- **THEN** Playwright finds the browser under `PLAYWRIGHT_BROWSERS_PATH` and nothing is downloaded
