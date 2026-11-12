---
applyTo: ".github/workflows/*.yml,.github/workflows/*.yaml"
description: "Guidelines for GitHub Actions workflows in Ouroboros: self-hosted runners, least privilege, and supply-chain hygiene."
---

# GitHub Actions Workflows

## Structure

- Follow the conventions of `ci.yml`: 4-space YAML indentation and a descriptive `name` on workflows, jobs and steps.
- Set `permissions` at the top of the workflow to the minimum (`contents: read`) and widen it per job only when needed.
- Use `concurrency` with `cancel-in-progress` for pull request and push workflows.
- Set `timeout-minutes` on jobs that could hang.

## Runners

- Jobs run on `self-hosted` runners, so never execute untrusted code on them. Avoid `pull_request_target` combined with a checkout of the pull request head, and do not expose secrets to workflows triggered from forks.
- Do not assume a tool is installed. Set up toolchains explicitly (rustup, `actions/setup-python`, `actions/setup-node`).

## Actions and tools

- First-party `actions/*` may be referenced by major-version tag, as `ci.yml` does today. Pin any third-party action to a full commit SHA and note the version in a comment.
- Prefer a plain `run` step with a version-bounded tool (for example `pip install "reuse>=5.1,<6"`) over an unreviewed marketplace action.
- Keep the compliance checks green: `cargo deny`, `cargo audit`, `npm audit`, the license checks, the SPDX header check and `reuse lint`. When a new dependency needs a license added to an allow-list, make that change deliberately.

## Secrets and injection

- Read secrets only through `${{ secrets.NAME }}`, pass them via `env:` to the step that needs them, and never print them.
- Prefer short-lived credentials (OIDC) over stored cloud keys.
- Do not interpolate `${{ github.event.* }}` values straight into shell scripts. Pass them through `env:` and quote the variable in the script.
- Start multi-line shell steps with `set -euo pipefail`.

## Checklist

- [ ] `permissions` is explicit and minimal
- [ ] No untrusted code runs on self-hosted runners
- [ ] Third-party actions are pinned to a commit SHA
- [ ] Secrets are passed through `env:` and never echoed
- [ ] Event data is not interpolated into scripts
- [ ] Long-running jobs have `timeout-minutes`
