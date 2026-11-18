---
name: webapp-testing
description: Test local websites and web applications with existing browser tooling, preferably Playwright. Use for UI interactions, responsive checks, browser errors, and frontend regressions.
---
# Web Application Testing

## Discover the setup
1. Inspect package scripts, browser tests, configuration, and server instructions.
2. Reuse the project's Playwright or equivalent runner. Do not assume a browser tool, Python environment, or browser binary exists.
3. Prefer focused regression tests over a new framework. Ask before adding substantial dependencies or downloading browser runtimes.

## Run safely
- Use isolated local test data. Do not perform destructive actions against production or real accounts.
- Start long-running servers through the available background-task tool. Confirm the ready URL in logs; do not assume a port or use fixed sleeps as readiness checks.
- Track the task handle. Stop only processes you started; never kill unrelated servers to free a port.
- Keep credentials, cookies, and personal data out of screenshots, fixtures, and reports.

## Exercise behavior
- Inspect the rendered page before selecting elements.
- Prefer role and accessible-name locators, or stable test IDs when semantics cannot identify the target. Avoid positional selectors.
- Assert visible outcomes, navigation, validation errors, persisted changes, or completed requests. A click alone is not a passing test.
- Wait for expected UI states rather than arbitrary delays. Do not increase timeouts simply to hide flaky assertions.
- Cover the changed happy path and relevant failure path. For race bugs, reproduce the sequence that broke.
- Check keyboard access, narrow viewport behavior, and relevant browser errors. Distinguish actual errors from expected mocked failures.

Run the focused test after final edits. Save evidence only when useful and link generated screenshots or reports using absolute paths. State which browser and behaviors were exercised. If browser execution is unavailable, report that limitation; unit tests are not an end-to-end pass.
