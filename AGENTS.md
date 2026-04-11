# Repository Guidelines

## Project Structure & Module Organization

`ppdns` is a small Rust CLI/TUI project. Core command parsing, PowerDNS integration, install/update flows, and inline unit tests live in `src/main.rs`. The full-screen terminal panel lives in `src/tui.rs`. Release and maintenance scripts are in `scripts/`:

- `scripts/install.sh`: install a published Linux binary
- `scripts/package-release.sh`: package release archives
- `scripts/check-changelog.sh`: verify release entries
- `scripts/render-release-notes.sh`: build GitHub Release notes from `CHANGELOG.md`

Project metadata is in `Cargo.toml`, `Cargo.lock`, `CHANGELOG.md`, and `README.md`. GitHub Actions workflows are under `.github/workflows/`.

## Build, Test, and Development Commands

- `cargo run` — start the default TUI panel
- `cargo run -- --dry-run` — exercise flows without changing DNS
- `cargo run -- install` — open the guided install/update flow
- `cargo test` — run unit tests
- `cargo fmt --check` — verify formatting
- `bash scripts/check-changelog.sh 1.1.0-alpha.1` — validate a release entry before tagging

Use `cargo build --release` for local release builds. For Linux artifacts, use `bash scripts/package-release.sh x86_64-unknown-linux-musl`.

## Coding Style & Naming Conventions

Follow standard Rust style: 4-space indentation, `snake_case` for functions, `PascalCase` for types and enums, and concise error messages. Keep comments sparse and useful. Prefer extending existing helpers over duplicating `pdnsutil` command assembly. Run `cargo fmt` before committing.

## Testing Guidelines

Tests are currently inline unit tests in `src/main.rs`. Add focused tests near the behavior you change, especially for parsing, version comparison, delete planning, and install/update decisions. Name tests by observable behavior, for example `legacy_add_command_uses_old_pdnsutil_form`.

## PowerDNS Compatibility Notes

When debugging PowerDNS behavior, do not assume the target machine is on the latest PowerDNS release. On apt-based systems, the available package candidate may still be `4.8.x`, and `ppdns install powerdns` currently installs whatever version the current apt repositories provide.

Treat `pdnsutil` 4.x compatibility as a first-class debugging path, especially for delete, replace, and serial-bump flows. Before blaming the TUI or local UI state, verify whether the issue only happens on the legacy command path (`list-all-zones`, `list-zone`, `add-record`, `replace-rrset`, `delete-rrset`, `increase-serial`) and confirm behavior against a real 4.8 environment when possible.

## Commit & Pull Request Guidelines

Recent history uses short, imperative commit subjects such as `Add TUI panel, install command, and MIT license` and `Release 1.1.0-alpha.1`. Keep commits scoped and descriptive. Pull requests should include:

- a short problem/solution summary
- commands you ran (`cargo test`, `cargo fmt --check`)
- screenshots or terminal captures for TUI changes
- release notes updates when changing published behavior

If you change a release version, update `Cargo.toml`, `Cargo.lock`, and `CHANGELOG.md` together.
