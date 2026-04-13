# Changelog

All notable changes to this project will be documented in this file.

## [1.1.0-alpha.7] - 2026-04-13

### Added

- Added TUI zone creation on `z`, with post-create zone verification and automatic focus on the new zone's records view.
- Added TUI record editing on `e` for updating the selected record content and TTL through verified rrset replacement.
- Added a TUI SOA health panel on `s`, including warnings for missing or malformed SOA state and a safe mailbox rewrite repair when possible.

### Changed

- Kept zone creation as a TUI-first workflow instead of exposing a separate `create zone` CLI command.

### Fixed

- Reused the verified rrset replacement path for edit and SOA repair flows so legacy PowerDNS `4.8.x` remains on the compatibility-tested command path.

## [1.1.0-alpha.6] - 2026-04-12

### Fixed

- Corrected legacy PowerDNS `pdnsutil` owner-name arguments for `add-record`, `delete-rrset`, and `replace-rrset`, so PowerDNS `4.8.x` receives relative owner names instead of absolute names.
- Fixed legacy delete operations that could return success while leaving the target rrset untouched, especially for records such as DKIM `CNAME` entries under `_domainkey`.

## [1.1.0-alpha.5] - 2026-04-12

### Fixed

- Verified add and delete mutations by reloading the zone after `pdnsutil` returns, and now show an explicit error when PowerDNS reports success but the record state does not actually change.
- Updated the TUI mutation path to refresh from the verified zone state instead of optimistic local edits, so stale records do not silently reappear as a false success.
- Defaulted the initial TUI focus to the records pane so record browsing works immediately after launch.

## [1.1.0-alpha.4] - 2026-04-12

### Fixed

- Moved zone-record loading and add/delete mutations off the TUI thread so navigation no longer blocks on `pdnsutil`.
- Accepted terminal key repeat events in addition to key press events so arrow-key navigation works reliably in terminals such as Termius.
- Kept mutation results flowing back into the current view through a background event channel so successful changes update the panel without freezing input.

## [1.1.0-alpha.3] - 2026-04-12

### Fixed

- Updated add and delete flows to apply successful changes to the in-memory record list immediately, so the TUI no longer leaves stale rows on screen after a mutation.
- Delayed the follow-up zone refresh after mutations so the panel can stay responsive while PowerDNS catches up.

## [1.1.0-alpha.2] - 2026-04-11

### Changed

- Simplified the default TUI to a leaner two-pane layout with fewer decorative elements and less persistent status text.
- Removed startup PowerDNS and latest-release status checks from the TUI path so the panel opens with less overhead on smaller machines.
- Reduced record-table work by caching filtered rows and debouncing zone reloads while moving through the zone list.

### Fixed

- Improved TUI responsiveness when navigating zones and large record sets.
- Prevented delete actions from using stale selections while the next zone is still loading.

## [1.1.0-alpha.1] - 2026-04-11

### Added

- Full-screen terminal DNS panel as the default `ppdns` experience, with zone navigation, record table, detail sidebar, inline filtering, add-record dialog, and delete confirmation.
- New `ppdns install` command for guided PowerDNS package install/update/reinstall and `ppdns` self-update flows.
- MIT license metadata and repository license file.

### Changed

- Default interactive entry now launches the TUI panel instead of the old menu-driven home screen.
- Release workflow now marks tags with prerelease suffixes such as `-alpha.1` as GitHub prereleases automatically.

## [1.0.2] - 2026-04-11

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
