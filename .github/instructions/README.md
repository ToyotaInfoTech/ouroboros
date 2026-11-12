# `.github/instructions`

This directory contains [GitHub Copilot instruction files](https://docs.github.com/en/copilot/customizing-copilot/adding-repository-custom-instructions-for-github-copilot) that provide context-specific guidance to Copilot.

## Files

| File | Applies To | Purpose |
|------|-----------|---------|
| `code-review.instructions.md` | `**` | Ouroboros-specific code review checklist |
| `code-review-generic.instructions.md` | `**` | Language-agnostic code review guidance |
| `python.instructions.md` | `**/*.py` | Python coding standards |
| `rust.instructions.md` | `**/*.rs` | Ouroboros Rust coding standards |
| `test-generation.instructions.md` | `**` | Test generation guidelines |
| `typescript.instructions.md` | `**/*.ts,**/*.tsx` | TypeScript/React standards |
| `markdown.instructions.md` | `**/*.md` | Markdown conventions for documentation |
| `security-and-owasp.instructions.md` | `**` | Security guidance (trust boundaries, input handling, secrets, dependencies) |
| `github-actions-ci-cd-best-practices.instructions.md` | `.github/workflows/*.yml` | GitHub Actions workflow guidelines |
| `containerization-docker-best-practices.instructions.md` | `**/Dockerfile,**/docker-compose*.yml` | Docker and Compose guidelines |

## Format

Each file uses YAML frontmatter with `applyTo` glob patterns:

```markdown
---
applyTo: "**/*.rs"
description: "Short description"
---

# Instruction content...
```
