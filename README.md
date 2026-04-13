# ppdns

`ppdns` is a guided PowerDNS DNS panel written in Rust.

It wraps `pdnsutil` and focuses on the common workflow that operators use most often:

- create one zone
- add one record
- delete one record
- list zones
- list records inside a zone

The main goal is to lower the learning cost of `pdnsutil` by giving you an interactive panel instead of asking you to remember the full command syntax.

## Features

- Full-screen TUI: run `ppdns` to open a Cloudflare-style DNS panel in the terminal.
- TUI zone creation: press `z` in the panel to create a zone with one primary NS and then replace the default SOA with ppdns-managed values.
- TUI record editing: press `e` on a selected record to update its content or TTL inside the current RRset.
- TUI SOA editing: press `s`, then `e`, to edit SOA primary NS, mailbox, serial, refresh, retry, expire, minimum, and TTL in one form.
- Guided CLI: run `ppdns add record` or `ppdns delete record` and fill the missing fields step by step.
- Safer single-record deletion: if one RRset has multiple values, `ppdns` only removes the selected value and keeps the others.
- Supports both legacy `pdnsutil` 4.x commands and modern PowerDNS 5.x object-style commands.
- SOA health panel: press `s` to inspect SOA state, repair mailbox values such as `hostmaster@example.com`, or open the dedicated SOA editor.
- Checks zone SOA health after create/add/edit/delete and warns when SOA records look malformed.
- `ppdns install` can install, update, or reinstall PowerDNS packages and `ppdns` itself.
- Still scriptable: you can pass flags directly when you do not want the guide.

## Install on Linux

For end users, the recommended path is a prebuilt release binary.

Once the repo is published to GitHub Releases, installation can look like this:

```bash
curl -fsSL https://raw.githubusercontent.com/0x3st/ppdns/main/scripts/install.sh | sh
```

Install a specific version:

```bash
curl -fsSL https://raw.githubusercontent.com/0x3st/ppdns/main/scripts/install.sh | sh -s -- --version 1.0.2
```

Install an alpha or other prerelease:

```bash
curl -fsSL https://raw.githubusercontent.com/0x3st/ppdns/main/scripts/install.sh | sh -s -- --version 1.1.0-alpha.1
```

Install to a custom directory:

```bash
curl -fsSL https://raw.githubusercontent.com/0x3st/ppdns/main/scripts/install.sh | sh -s -- --bin-dir /usr/local/bin
```

The install script will:

- detect Linux architecture
- download the matching release archive
- install `ppdns` into `/usr/local/bin` when writable
- otherwise fall back to `~/.local/bin`

## Requirements For Source Builds

- Rust toolchain
- `pdnsutil` installed and available in `PATH`

You can also point to a custom binary with `--pdnsutil /path/to/pdnsutil`.

## Build

```bash
cargo build --release
```

## Local PowerDNS 4.8 Sandbox

If you want to test real `pdnsutil 4.8.x` behavior locally without touching your host machine, use the bundled Docker sandbox.

Bring it up:

```bash
bash scripts/dev-up.sh
```

Seed one test zone:

```bash
bash scripts/dev-seed.sh
```

Run `ppdns` against the containerized `pdnsutil`:

```bash
cargo run -- --pdnsutil ./scripts/pdnsutil-docker.sh
```

Reset the sandbox back to an empty sqlite database:

```bash
bash scripts/dev-reset.sh
```

The sandbox uses Ubuntu 24.04's `pdns-server 4.8.3`, exposes DNS on `127.0.0.1:5300`, and exposes the PowerDNS web/API listener on `127.0.0.1:8081`.

## Release Linux Binaries

This repo includes:

- [scripts/install.sh](/Users/laywoo/ppdns/scripts/install.sh) for end-user installation
- [scripts/package-release.sh](/Users/laywoo/ppdns/scripts/package-release.sh) for creating release archives
- [scripts/check-changelog.sh](/Users/laywoo/ppdns/scripts/check-changelog.sh) for validating release notes in `CHANGELOG.md`
- [scripts/render-release-notes.sh](/Users/laywoo/ppdns/scripts/render-release-notes.sh) for extracting one version entry into the GitHub Release body
- [.github/workflows/release.yml](/Users/laywoo/ppdns/.github/workflows/release.yml) for GitHub Release automation

Package a local release artifact:

```bash
bash scripts/package-release.sh x86_64-unknown-linux-musl
```

This creates:

- `dist/ppdns-x86_64-unknown-linux-musl.tar.gz`
- `dist/ppdns-x86_64-unknown-linux-musl.tar.gz.sha256`

The GitHub Actions workflow is designed to build and publish Linux binaries on `v*` tags, with version checks against `Cargo.toml` and `CHANGELOG.md`. The published Release body starts with the matching `CHANGELOG.md` section, and GitHub-generated release notes are appended after it.

If the tag includes a prerelease suffix such as `v1.1.0-alpha.1` or `v1.1.0-rc.1`, GitHub Releases will mark it as a prerelease automatically.

## Quick Start

Open the TUI:

```bash
cargo run
```

Use the install manager:

```bash
cargo run -- install
cargo run -- install powerdns
cargo run -- install ppdns --reinstall
```

Guided add:

```bash
cargo run -- add record
```

Guided delete:

```bash
cargo run -- delete record
```

Non-interactive add:

```bash
cargo run -- add record \
  --zone example.com \
  --name www \
  --type A \
  --content 1.2.3.4 \
  --ttl 300
```

Non-interactive delete:

```bash
cargo run -- delete record \
  --zone example.com \
  --name www \
  --type A \
  --content 1.2.3.4
```

List zones:

```bash
cargo run -- list zones
```

List records in a zone:

```bash
cargo run -- list records --zone example.com
```

## How deletion works

`pdnsutil rrset delete` removes the whole RRset, not a single value.

`ppdns` works around this by:

1. reading the current zone records
2. locating the exact value you selected
3. using `rrset replace` when the RRset still has remaining values
4. using `rrset delete` only when the selected value is the last one

This means you can delete one `A` record from a multi-value RRset without wiping the whole set.

## Flags

Global flags:

- `--pdnsutil PATH`
- `--config-dir DIR`
- `--config-name NAME`
- `--dry-run`

Install flags:

- `powerdns`
- `ppdns`
- `--install`
- `--update`
- `--reinstall`

Add flags:

- `--zone ZONE`
- `--name NAME`
- `--type TYPE`
- `--content CONTENT`
- `--ttl TTL`
- `-y`, `--yes`

Delete flags:

- `--zone ZONE`
- `--name NAME`
- `--type TYPE`
- `--content CONTENT`
- `-y`, `--yes`

## Notes

- The default `ppdns` entry opens the full-screen TUI.
- Press `z` inside the TUI to create a zone.
- Press `e` inside the TUI to edit the selected record value or TTL.
- Press `s` inside the TUI to inspect SOA health, and press `e` there to edit SOA fields directly.
- Use `ppdns install` for PowerDNS package management and ppdns self-update flows.
- This tool currently focuses on zone creation plus single-record add/edit/delete workflows.
- Record names are normalized for convenience:
  - `@` becomes the zone apex
  - `www` becomes `www.<zone>.`
  - `www.example.com` becomes `www.example.com.`
- Zone creation asks for one primary nameserver and then rewrites the initial SOA with ppdns-managed defaults.
- `ppdns` warns if a zone has no apex SOA, has multiple SOA records, has malformed SOA content, or still contains `@` inside SOA content.
- For `TXT`, guided mode automatically adds quotes.
