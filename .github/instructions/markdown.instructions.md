---
applyTo: "**/*.md"
description: "Markdown conventions for Ouroboros documentation (CommonMark, English)."
---

# Markdown

Write CommonMark in English.

## Structure

- Use ATX headings (`#`). Each file has one H1, and heading levels do not skip.
- Leave a blank line before and after headings, lists, tables and code blocks.
- Use `-` for bullet lists and `1.` for numbered lists.
- Write tables with leading and trailing pipes and a header separator row. Keep cells short.

## Code

- Fence code blocks and always give a language (`rust`, `bash`, `toml`, `text`, `mermaid`).
- Use inline code for identifiers, file paths, commands and configuration keys.
- Draw diagrams with Mermaid fences.

## Links and images

- Link to files in this repository with relative paths.
- Use descriptive link text, not "here". Write bare URLs as `<https://example.com>`.
- Give every image non-empty alt text.

## Emphasis

- Use `*italic*` and `**bold**`.

## Keeping documents current

- When a change adds, renames or removes a documented file, command or option, update the document that lists it (for example `.github/instructions/README.md`, `docs/WebUI.md`).
