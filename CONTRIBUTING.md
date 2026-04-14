# Contributing to ppdns

`ppdns` is a small Rust project, but it sits on top of `pdnsutil`, so contribution quality depends less on code volume and more on testing discipline.

## Principles

- Prefer fixing the real PowerDNS behavior, not just the TUI symptom.
- Treat `pdnsutil 4.8.x` compatibility as a first-class path.
- Use feature branches for testing. Do not burn prerelease tags just to debug an unfinished change.
- Keep the UI simple. Low-noise terminal output is a project goal.

## Project Layout

- `src/main.rs`: CLI parsing, PowerDNS integration, install/update flows, inline unit tests
- `src/tui.rs`: full-screen TUI
- `scripts/`: install, packaging, release, and local development helpers
- `docker/` and `docker-compose.yml`: local PowerDNS 4.8 sandbox
- `.github/workflows/`: CI and release workflows

## Recommended Workflow

1. Create a branch such as `feat/...` or `fix/...`.
2. Make the smallest coherent change that solves one problem well.
3. Test locally before pushing.
4. Push the branch and use CI artifacts for remote testing when needed.
5. Merge to `main` only when the change has passed local and branch validation.
6. Tag and publish only after the merged result is known good.

## Local Development

Common commands:

```bash
cargo run
cargo run -- --dry-run
cargo test
cargo fmt --check
```

For release-style packaging:

```bash
bash scripts/package-release.sh x86_64-unknown-linux-musl
```

## Local PowerDNS 4.8 Sandbox

When a change touches create, edit, delete, SOA handling, or legacy command compatibility, test against a real local sandbox instead of guessing.

Bring up the sandbox:

```bash
bash scripts/dev-up.sh
```

Seed a test zone:

```bash
bash scripts/dev-seed.sh
```

Run `ppdns` against containerized `pdnsutil`:

```bash
cargo run -- --pdnsutil ./scripts/pdnsutil-docker.sh
```

Reset the sandbox:

```bash
bash scripts/dev-reset.sh
```

The sandbox uses Ubuntu 24.04 with `pdnsutil 4.8.3`.

## Testing Expectations

At minimum, contributors should run:

```bash
cargo fmt --check
cargo test
```

If you changed TUI editing, delete flows, SOA flows, or zone creation, also verify the change interactively in the local PowerDNS sandbox.

Good regression checks include:

- deleting one value from a multi-value RRset
- deleting legacy-style owner names such as `_domainkey` records
- editing SOA fields
- creating a zone and verifying that ppdns rewrites the default SOA
- checking that status messages and focus behavior recover correctly after mutations

## Coding Notes

- Follow standard Rust style.
- Prefer extending existing helpers over adding parallel logic paths.
- Keep comments sparse and useful.
- Add focused inline unit tests near the behavior you changed.
- Name tests by observed behavior, for example `legacy_add_command_uses_old_pdnsutil_form`.

## Pull Requests

A good pull request should include:

- a short problem/solution summary
- commands you ran
- screenshots or terminal captures for TUI changes
- notes about PowerDNS version coverage if the change touches command compatibility

## Release Discipline

- Do not cut a prerelease just to find out whether a branch is broken.
- Validate on a branch first.
- If a release version changes, update `Cargo.toml`, `Cargo.lock`, and `CHANGELOG.md` together.
