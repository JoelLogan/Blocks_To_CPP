<!-- A pull request description sits under its title, so its sections start at level 2. -->
<!-- markdownlint-disable-file first-line-heading -->

## What and why

<!-- What does this change do, and why? Link the issue or spec section. -->

## How it was tested

<!-- Commands run, new tests, screenshots for UI changes. -->

## Checklist

- [ ] Tests added or updated (unit, plus golden/E2E where behaviour is visible)
- [ ] Docs updated (spec, user guide or reference); an ADR for significant decisions
- [ ] `CHANGELOG.md` entry for user-visible changes
- [ ] Accessibility checked for UI changes (keyboard path, labels, contrast)
- [ ] No new warnings; CI green

### Security questions (docs/spec/08-security.md)

- [ ] No new IPC command, or it validates every input and is documented
- [ ] No new `unsafe`, or it is in `crates/b2c-process` with a `// SAFETY:` comment
- [ ] No new dependency, or the justification is in this description
- [ ] No new process spawn or file write, or it follows §8.5–8.7
- [ ] User text reaches generated C++ only through `b2c_ir::text`
