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
ghr install sharkdp/bat -t v0.24.0    # pin to a specific release tag
ghr install junegunn/fzf --to ~/bin   # install into a specific directory
```

`<repo>` accepts `owner/repo` or any `github.com` URL (with or without scheme, trailing paths are ignored). `ghr i` is a shorthand alias for `ghr install`.

Pass `--to <path>` to install into a directory other than the configured `install_dir` (a leading `~` is expanded). The choice is recorded in the tool's install path, so later `ghr update`s reinstall it there too. It's a local override and is not written to the manifest.

Pass `-t/--tag <tag>` to install (and pin) an exact release instead of picking interactively. A pinned tool is **locked**: `ghr update` skips it until you explicitly unpin it with `ghr update <name> --force` (see below). To move a pin to a different tag, re-run `ghr install <repo> -t <newtag>` on the already-managed tool — it reinstalls at that tag and updates the pin in place. Every install records the tool in the [manifest](#manifest).

### `ghr update [name] [--all] [-f/--force]`

Update one or all managed tools to their latest release. Pinned tools are skipped by default. Use `-f/--force` on a named tool to update it to the latest release anyway, which clears its pin (the tool tracks latest again afterwards). `--force` has no effect with `--all` — pinned tools stay locked there; name the tool to force it.

```sh
ghr update ripgrep
ghr update --all
ghr update bat --force   # update a pinned tool to latest and clear its pin
```

### `ghr check [--json]`

Check for available updates without installing anything. Checks run concurrently, and pinned tools are skipped. Exits with code `1` if any updates are available, making it useful in scripts.

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

### `ghr remove [-y] <name>`

Uninstall a binary and remove it from ghr state. `-y` to skip confirmation prompt

```sh
ghr remove ripgrep
```

### `ghr sync`

Install every tool listed in the [manifest](#manifest) that is missing from local state. Pinned entries install their exact tag; the rest install the latest release. Tools already installed are left untouched. This is how you reproduce your toolset on a new machine after copying over `manifest.toml`.

```sh
ghr sync
```

### `ghr clean`

Remove ghr's download cache at `~/.cache/ghr`. Installs already clean up after themselves, but interrupted or failed runs can leave partial downloads and extraction directories behind — this is the manual sweep. The cache is fully regenerable, so it runs without a prompt and reports how much was freed.

```sh
ghr clean
```

### `ghr setup-timer`

Write a systemd user service and timer that runs `ghr check` on a schedule and optionally enable it immediately.

```sh
ghr setup-timer
```

### `ghr disable-timer`

Disable and remove the ghr timer unit created by `ghr setup-timer`.

```sh
ghr disable-timer
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
| `~/.config/ghr/manifest.toml` | Declarative, portable list of managed tools (repo + optional pinned tag) |
| `~/.local/share/ghr/state.toml` | Installed tools, versions, checksums, ETags |
| `~/.cache/ghr/` | Download cache (cleaned after each install; `ghr clean` clears any leftovers) |

---

## Manifest

`~/.config/ghr/manifest.toml` is a declarative, portable list of the tools ghr manages. Unlike `state.toml` — a local runtime cache of install paths, checksums, and ETags — the manifest holds only each tool's portable identity (its repo and an optional pinned tag), so you can commit it to your dotfiles and replay it on another machine.

`ghr install`, `ghr remove`, and `ghr adopt` keep it in sync automatically. You can also hand-edit it:

```toml
[[tools]]
repo = "BurntSushi/ripgrep"

[[tools]]
repo = "sharkdp/bat"
tag = "v0.24.0"      # optional — presence pins/locks the tool to this tag
```

Run `ghr sync` to install everything in the manifest that isn't installed yet. A `tag` both selects the version `sync` installs and locks the tool so `ghr update` skips it.

## Roadmap
- [ ] aliasing with -a / --alias, for ripgrep for example. should be persisted in manifest as well.
- [x] Concurrent `ghr check`
- [x] `ghr i` alias for `ghr install`
- [x] `ghr install --to` command to install to given path
- [x] `ghr clean` to clean cache files
- [ ] logging / tracing. indicatif has both logging and tracing integrations. logs should be available in a log file. replace `println`s with proper log statements.
- [x] Version pinning with `ghr install Ardnys/ghr -t v0.1.1`
- [x] **manifest file support**
  - [x] `manifest.toml` alongside config.toml, shows tools and repositories, optional version tags.
  - [x] `ghr install` and `ghr remove` keeps that file in sync automatically.
  - [x] `ghr sync` installs everything in the manifest file that's missing in current state.
- [ ] perhaps installing binaries to somewhere related to ghr as default, so it's clear what's managed by ghr and what is not

## Contributing
For feature requests and bug reports, please open an issue on GitHub.


## License
ghr is licensed under the MIT License.
