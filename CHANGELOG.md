# Changelog

All notable changes to this project will be documented in this file.

## [1.3.0] - 2026-01-04

### Added
- Fluent-based localization with system auto-detect, language override, and native language names in the dropdown (issue #10).
- UI translations for Dutch, English, French, German, Spanish, and Swedish plus translated species labels (issues #5, #11).
- Windows in-app updater workflow (download + hash/size validation + installer launch via FeedieUpdater) (issue #6).
- Recursive scan option with cached result re-open (issue #23).
- Linux AppImage releases (x86_64 + aarch64) with AppImage model/resource fallbacks for APPDIR and /usr/share (issue #4).
- Chromebook/Crostini compatibility handling (force X11 + scaling defaults when detected) (issue #4).

### Changed
- Background labels setting replaced free-text input with fixed special label selection (issue #5).
- Batch size no longer exposed in settings; auto-batch selection drives inference (issue #7).
- Language selection list stays stable and uses native labels (Nederlands, English, etc.) (issue #10).
- macOS bundling and release workflows updated for current runners and Feedie.app output (issue #1).

### Fixed
- Context-menu export no longer triggers on hover; it requires a click (issue #2).
- Manifest fetch/update errors localized consistently (issue #11).
- Thumbnail grid scroll behavior now enables immediate interaction on new galleries (issue #3).

### Performance
- Faster preprocessing via fast_image_resize and zune-jpeg decode path (issue #7).
- Pipeline overlap for preprocessing + inference to reduce idle time (issue #7).
