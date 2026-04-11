# Changelog

All notable changes to this project will be documented in this file.

## [1.0.1] - 2026-04-11

### Fixed

- Added compatibility for legacy `pdnsutil` 4.x commands such as `list-all-zones`, `list-zone`, `add-record`, `replace-rrset`, and `delete-rrset`.

## [1.0.0] - 2026-04-11

### Added

- Initial guided Rust CLI for common PowerDNS record operations.
- Interactive flows for adding and deleting a single record.
- Safer single-record deletion for multi-value RRsets by using `rrset replace`.
- Linux release packaging, install script, CI workflow, and automated GitHub Releases.
