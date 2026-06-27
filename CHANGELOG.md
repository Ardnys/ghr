## [0.2.0] - 2026-06-22

### 🚀 Features

- Implement manifest file and version tagging
- Add 'i' alias for install
- Add "clean" command to clean cache folder
- Add --to flag for specific install location
- Concurrent `cmd_check`
- Add -a/--alias for aliasing binaries
- Add `sync --prune` and preserve manifest structure and comments
- Add tracing

### 🐛 Bug Fixes

- Add handler for dialoguer Ctrl-C exits making cursor invisable

### 🚜 Refactor

- Rewrite installation pipeline
- [**breaking**] Rename project ghr -> binto

### ⚡ Performance

- Concurrent downloads on `cmd_sync`

### ⚙️ Miscellaneous Tasks

- Bump version to v0.2.0
