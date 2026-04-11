# ppdns

`ppdns` is a guided PowerDNS CLI written in Rust.

It wraps `pdnsutil` and focuses on the common workflow that operators use most often:

- add one record
- delete one record
- list zones
- list records inside a zone

The main goal is to lower the learning cost of `pdnsutil` by giving you an interactive guide instead of asking you to remember the full command syntax.

## Features

- Guided mode: run `ppdns` or `ppdns add record` and fill the missing fields step by step.
- Safer single-record deletion: if one RRset has multiple values, `ppdns` only removes the selected value and keeps the others.
- No external Rust dependencies, so it can compile offline.
- Still scriptable: you can pass flags directly when you do not want the guide.

## Install on Linux

For end users, the recommended path is a prebuilt release binary.

Once the repo is published to GitHub Releases, installation can look like this:

```bash
curl -fsSL https://raw.githubusercontent.com/0x3st/ppdns/main/scripts/install.sh | sh
```

Install a specific version:

```bash
curl -fsSL https://raw.githubusercontent.com/0x3st/ppdns/main/scripts/install.sh | sh -s -- --version 1.0.0
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

## Quick Start

Interactive entry:

```bash
cargo run
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

- This tool currently focuses on single-record add/delete workflows.
- Record names are normalized for convenience:
  - `@` becomes the zone apex
  - `www` becomes `www.<zone>.`
  - `www.example.com` becomes `www.example.com.`
- For `TXT`, guided mode automatically adds quotes.
