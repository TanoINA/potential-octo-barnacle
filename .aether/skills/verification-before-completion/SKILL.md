---
name: verification-before-completion
description: Verify code changes, builds, tests, and generated files before reporting success. Use when finishing implementation or claiming that a fix works.
---
# Verification Before Completion

Use focused checks for the changed behavior. Do not run every test or delegate a review for every edit.

1. Identify the claim and the smallest check that supports it. Inspect existing project scripts and tests instead of inventing commands.
2. For a bug fix, add a regression test when practical and establish the failure before changing implementation.
3. Run checks after the final relevant edit. Read exit status, actual results, and skipped tests. An earlier run does not verify later edits.
4. Run broader checks once when changes cross modules or affect persistence, security, or shared contracts.
5. Inspect the final diff for accidental files, secrets, and unrelated changes.
6. Report what passed and what remains unchecked. A failed, timed-out, or running command is not a success.

## Match evidence to claims
- A build proves compilation and packaging, not correct UI behavior.
- A mocked test does not prove the external service works.
- File existence does not prove correct contents. Inspect relevant content and metadata; hash files when identity matters.
- Starting a background task is not finishing it. Monitor its existing task handle and confirm the outcome.
- A local commit is not a successful push. Confirm the remote result before claiming publication.
- Exercise UI changes in a browser when tooling is available. Otherwise disclose missing browser verification.

Do not install tools, alter production data, commit, push, or deploy merely to obtain evidence without required authorization. Use the project's dependency environment.

Keep the final summary short: changes, checks and results, limitations. Never fabricate test counts, logs, screenshots, or external confirmations.
