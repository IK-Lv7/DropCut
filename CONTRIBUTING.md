# Contributing

Thanks for helping! Bug reports, ideas and pull requests are welcome.

## Ground rules

These come from the product spec ([AGENTS.md](AGENTS.md)) and are not negotiable:

- Everything runs locally: no cloud or paid APIs, no telemetry, no uploads. Only model downloads may use the network.
- Never overwrite the user's source video.
- Start external programs with argument arrays, never shell strings. Validate all external input.
- Show friendly errors, support cancel, clean up temporary files, and never log transcript or subtitle text.
- Keep the UI beginner-simple; advanced options belong in the hidden Advanced section.

## Workflow

```bash
npm install
npm run build
cd src-tauri && cargo test --locked
```

Both must pass before you open a pull request. Add tests for new logic (see `mod tests` in each Rust file). When you add bundled software or models, update `THIRD_PARTY_NOTICES.md`. By contributing you agree that your work is released under the [MIT License](LICENSE).
