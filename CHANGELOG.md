# Changelog

All notable changes to this project will be documented in this file.

## [1.0.22] - 2026-04-11

### Added

- Automatically increase the SOA serial after successful record add and delete operations.

### Fixed

- Improved the delete-record guide to narrow selection by record name, then type, then value.
- Improved PowerDNS package version detection so package revisions like `4.8.3-4build3` are not shown as false updates.
- Adjusted the default SQLite backend package recommendation for apt-based systems.

## [1.0.1] - 2026-04-11

### Fixed

- Added compatibility for legacy `pdnsutil` 4.x commands such as `list-all-zones`, `list-zone`, `add-record`, `replace-rrset`, and `delete-rrset`.

### Added

- Home screen status checks for installed PowerDNS and the latest `ppdns` release.
- Guided menu actions to install, update, or reinstall PowerDNS on apt-based Linux systems.
- Guided menu actions to update or reinstall `ppdns` from GitHub Releases.

## [1.0.0] - 2026-04-11

### Added

- Initial guided Rust CLI for common PowerDNS record operations.
- Interactive flows for adding and deleting a single record.
- Safer single-record deletion for multi-value RRsets by using `rrset replace`.
- Linux release packaging, install script, CI workflow, and automated GitHub Releases.
