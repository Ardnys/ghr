# ghr

A user-land binary package manager for GitHub releases. Install, track, and update CLI tools without `sudo`, a system package manager, or compiling from source.

```
ghr install BurntSushi/ripgrep
ghr install https://github.com/sharkdp/bat
ghr update --all
```

---

## Install

Download the latest release for your platform from the [releases page](../../releases/latest), extract, and place the binary on your `PATH`:

```sh
tar -xzf ghr-*-x86_64-unknown-linux-gnu.tar.gz
mv ghr ~/.local/bin/
```

Then bootstrap ghr to manage itself:

```sh
ghr adopt ~/.local/bin/ghr Ardnys/ghr
ghr update ghr
```

---

## Usage

### `ghr install <repo>`

Fetch releases for a GitHub repository, pick a release and asset interactively, download, verify checksum, and install to `~/.local/bin`.

```sh
ghr install BurntSushi/ripgrep
ghr install https://github.com/cli/cli
ghr install sharkdp/fd --prerelease   # include pre-releases
```

`<repo>` accepts `owner/repo` or any `github.com` URL (with or without scheme, trailing paths are ignored).

### `ghr update [name] [--all]`

Update one or all managed tools to their latest release.

```sh
ghr update ripgrep
ghr update --all
```

### `ghr check [--json]`

Check for available updates without installing anything. Exits with code `1` if any updates are available, making it useful in scripts.

```sh
ghr check
ghr check --json | jq '.[] | select(.update_available)'
```

### `ghr list [--json]`

List all managed tools with their installed version and last-checked timestamp.

```sh
ghr list
ghr list --json
```

### `ghr adopt <path> <repo>`

Register a binary that is already on disk (installed by a package manager, curl script, etc.) under ghr management without moving or reinstalling it. Subsequent `ghr update` calls will update it in place.

```sh
ghr adopt ~/.local/bin/fzf junegunn/fzf
ghr adopt /usr/local/bin/lazygit jesseduffield/lazygit
```

### `ghr remove <name>`

Uninstall a binary and remove it from ghr state.

```sh
ghr remove ripgrep
```

### `ghr setup-timer`

Write a systemd user service and timer that runs `ghr check` on a schedule and optionally enable it immediately.

```sh
ghr setup-timer
```

---

## Configuration

Config is stored at `~/.config/ghr/config.toml` and created with defaults on first run.

```toml
install_dir = "~/.local/bin"
github_token = ""           # or set GITHUB_TOKEN env var
include_prereleases = false
check_interval_hours = 24
notify = "terminal"         # "terminal" | "desktop" | "none"
```

**`GITHUB_TOKEN`** — unauthenticated requests are limited to 60/hour. A token raises this to 5000/hour. Create one at <https://github.com/settings/tokens> (no scopes needed for public repos).

---

## How asset selection works

ghr filters out checksums, source archives, Windows/macOS assets, `.deb`/`.rpm` packages, then scores remaining assets by:

- Architecture match (`x86_64`, `aarch64`, `armv7`, `i686` and common synonyms like `amd64`, `arm64`)
- `linux` keyword presence
- libc preference (`gnu` > `musl`)
- Format preference (raw binary > tar > zip > AppImage)

If the top candidate's score is sufficiently ahead of the second, it is selected automatically. Otherwise an interactive picker is shown. The selected asset's name pattern is saved so future updates skip the picker entirely.

---

## State files

| Path | Purpose |
|------|---------|
| `~/.config/ghr/config.toml` | User configuration |
| `~/.local/share/ghr/state.toml` | Installed tools, versions, checksums, ETags |
| `~/.cache/ghr/` | Download cache (cleaned after each install) |
