# simplepackageinstaller

A small terminal UI (TUI) for setting up an Arch Linux machine from a single declarative YAML file.

You describe everything you want on a machine — pacman packages, AUR packages, Flatpaks, npm/cargo/uv/dotnet tools, systemd services and custom shell scripts — in `packages.yaml`. `simplepackageinstaller` shows it as a collapsible checklist, detects what is already installed, resolves dependencies, and installs the rest in the correct order with live logs.

## Features

- **One config file** for all package sources: `pacman`, `paru` (AUR), `flatpak`, `npm`, `cargo`, `uv`, `dotnet` and `script`.
- **Interactive selection** in a tree of sections → groups → packages, with required and preselected defaults.
- **Installed-state detection** for every source, so re-running is safe and only installs what is missing.
- **Dependency resolution** between packages and whole sections, with cycle detection at load time. Installer tools are pulled in automatically (e.g. `paru` packages require `paru`, `cargo` packages require `rust`) if they are defined in the config.
- **Post-install actions**: enable system services, user services, or run arbitrary shell commands.
- **Script steps** that run once (tracked with marker files) or every time.
- **Single sudo prompt** inside the TUI; credentials are kept alive for the whole run and child processes never prompt on the terminal.
- **Optional system update** (`pacman -Syu`) before installing.
- **Live progress and logs** per package, plus a full log file on disk.
- **Dry-run mode** to see exactly what would be executed.
- Ships as a single static binary (musl).

## Installation

### Prebuilt binary

Download `simplepackageinstaller` from the latest [GitHub release](../../releases/latest) and make it executable:

```sh
chmod +x simplepackageinstaller
./simplepackageinstaller --help
```

### From source

Requires a Rust toolchain (edition 2024, Rust 1.88+).

```sh
./build.sh          # builds a release binary into bin/simplepackageinstaller
# or
cargo build --release
```

`build.sh` builds a static musl binary if the `x86_64-unknown-linux-musl` target is available, otherwise a regular glibc binary.

## Usage

```sh
simplepackageinstaller [--config packages.yaml] [--dry-run] [--version]
```

| Option | Description |
| --- | --- |
| `--config <path>` | Path to the config. Defaults to `./packages.yaml`, then `$XDG_CONFIG_HOME/simplepackageinstaller/packages.yaml` (default `~/.config/simplepackageinstaller/`). |
| `--dry-run` | Show and "run" all commands without executing anything. |
| `-V`, `--version` | Show version. |
| `-h`, `--help` | Show usage. |

Run it as your normal user, **not** as root. It asks for your sudo password once when the first step that needs it is reached.

### Workflow

1. On start you are asked whether to run a full system update first (recommended).
2. Select what to install. Already installed packages are marked `(installed)`; required packages cannot be unchecked; dependencies pulled in automatically are shown as `[+]`. Alternatives in a [choice](#choices) are shown as `( )` / `(•)`.
3. Press `enter` to start. Packages are installed in dependency order. If a package fails, everything that depends on it is skipped.
4. When done, the status of each package is shown along with the path to the log file.

### Keys — selection screen

| Key | Action |
| --- | --- |
| `↑` `↓` / `k` `j` | Move |
| `PgUp` `PgDn`, `Home`/`g`, `End`/`G` | Jump |
| `→` / `l`, `←` / `h` | Expand / collapse section or group |
| `space` | Toggle the item (whole section/group on a header row) |
| `a` / `n` | Select all / select none (except required); one alternative per choice |
| `u` | Toggle system update |
| `R` | Force re-run of an already installed package |
| `enter` | Start installation |
| `q` / `esc` | Quit |

### Keys — install screen

| Key | Action |
| --- | --- |
| `↑` `↓` / `k` `j` | Select package to view its log |
| `f` | Follow the currently running package |
| `enter` | Toggle fullscreen log |
| `PgUp` `PgDn` | Scroll log |
| `q` / `esc` | Quit (when finished) |

### Status icons

| Icon | Meaning |
| --- | --- |
| `○` | Pending |
| `⟳` | Running |
| `✓` | Installed |
| `↷` | Already installed |
| `⚠` | Installed, but a post command failed |
| `✗` | Failed |
| `⊘` | Skipped (a dependency failed or sudo was cancelled) |

## Configuration

The config is a YAML file with a list of **sections**. Each section has **groups**, and each group lists packages per source. A JSON schema is included in [`packages.schema.json`](packages.schema.json); add this line at the top of your config for editor completion and validation (e.g. with the YAML language server):

```yaml
# yaml-language-server: $schema=./packages.schema.json
```

### Example

```yaml
# yaml-language-server: $schema=./packages.schema.json
sections:
  - name: Base
    required: true
    groups:
      - name: Tools
        pacman:
          - base-devel
          - git
          - flatpak
      - name: AUR helper
        choice: paru        # pick one; requiring "paru" is met by either
        script:
          - paru-bin
          - paru:           # runs scripts/paru.sh once
              requires: rust

  - name: Desktop
    selected: true
    requires: base          # every package here requires the "Base" section
    groups:
      - name: Audio
        pacman:
          - pipewire
          - wireplumber:
              user_service: [pipewire.service, wireplumber.service]
      - name: Bluetooth
        selected: false
        pacman:
          - bluez:
              service: bluetooth.service
          - bluez-utils

  - name: Development
    groups:
      - name: Editors
        paru:
          - visual-studio-code-bin
        flatpak:
          - com.jetbrains.Rider
      - name: CLI
        pacman:
          - rust            # same package as paru's requirement
        cargo:
          - ripgrep
          - cargo-watch:
              flags: --features notify
        uv:
          - ruff
        npm:
          - typescript
      - name: .NET
        pacman:
          - dotnet-sdk:
              post: dotnet --info
        dotnet:
          - dotnet-ef:
              requires: dotnet-sdk
      - name: Dotfiles
        script:
          - dotfiles:
              file: ~/dotfiles/install.sh
              always: true
              flags: [--force]
```

### Sections

| Field | Type | Description |
| --- | --- | --- |
| `name` | string | Heading. Its id is the slug of the name (`Desktop Hardware` → `desktop-hardware`) and can be used in `requires`. |
| `groups` | list | Groups in the section. |
| `required` | bool | Always selected; cannot be unchecked. Inherited downwards. |
| `selected` | bool | Preselected, but can be unchecked. Inherited downwards. |
| `requires` | string or list | Packages or section ids that every package in the section requires. |
| `choice` | bool or string | Pick one of the section's packages. See [Choices](#choices). |

### Groups

| Field | Type | Description |
| --- | --- | --- |
| `name` | string | Group name. |
| `required` / `selected` | bool | As for sections. |
| `choice` | bool or string | Pick one of the group's packages. See [Choices](#choices). |
| `pacman`, `paru`, `flatpak`, `npm`, `cargo`, `uv`, `dotnet`, `script` | list | Packages per source. |

Within a group, packages are installed in the source order listed above (pacman first, script last), subject to dependencies.

### Choices

With `choice` on a section or group, at most one of its packages can be selected; selecting one unchecks the others. With `required`, exactly one is always selected: an installed alternative, else one marked `selected`, else the first.

```yaml
- name: Editor
  required: true
  choice: neovim
  pacman: [neovim]
  paru: [neovim-git]
```

A string value is a base name. Requiring it — with `requires` or as a source's implicit requirement (e.g. `paru` for AUR packages) — is met by whichever alternative is chosen or installed; if none is, the first is pulled in. `choice: true` makes a choice without a base name. A section id in `requires` counts each choice in it as one requirement.

`choice` cannot be set on both a section and one of its groups, and a package can only be part of one choice.

### Packages

A package is either a plain name or a single-key map of name → options:

```yaml
pacman:
  - git
  - docker:
      service: docker.service
      post: sudo usermod -aG docker "$USER"
```

The same package may be listed in several places (e.g. `rust` both as a dependency in Base and under Development). It is one package: selecting it anywhere selects it everywhere. It must use the same source everywhere, and options may only be given on one of the occurrences. `required` / `selected` from any occurrence apply.

| Option | Type | Description |
| --- | --- | --- |
| `flags` | string or list | Extra flags for the install command (arguments for scripts). |
| `service` | string or list | `sudo systemctl enable --now <service>` after install. |
| `user_service` | string or list | `systemctl --user enable <service>` after install. |
| `post` | string or list | Shell commands run after install. |
| `requires` | string or list | Packages or section ids that must be installed first. |
| `required` / `selected` | bool | As for sections. The closest level wins: package → group → section. |
| `file` | string | Script only. Path to the script: relative to the config, absolute, or `~/…`. Default `scripts/<name>.sh`. |
| `always` | bool | Script only. Run every time instead of once. |

### Sources and commands

| Source | Command | Implicitly requires | Installed check |
| --- | --- | --- | --- |
| `pacman` | `sudo pacman -S --needed --noconfirm <flags> <name>` | — | `pacman -Qq` / `pacman -Qg` |
| `paru` | `paru -S --needed --noconfirm --sudoloop <flags> <name>` | `paru` | `pacman -Qq` / `pacman -Qg` |
| `flatpak` | `flatpak install -y --noninteractive <flags> flathub <id>` | `flatpak` | `flatpak list` |
| `npm` | `sudo npm install -g <flags> <name>` | `npm` | `npm ls -g` |
| `cargo` | `cargo install --locked <flags> <crate>` | `rust` | `cargo install --list` |
| `uv` | `uv tool install <flags> <name>` | `uv` | `uv tool list` |
| `dotnet` | `dotnet tool update -g <flags> <id>` | `dotnet` | `dotnet tool list -g` |
| `script` | `bash -euo pipefail <file> <flags>` | — | marker file, or a pacman package with the same name |

"Implicitly requires" only applies if a package or choice base name with that name exists in the config.

### Scripts

Script steps run with `bash -euo pipefail`, with the config's directory as working directory and these environment variables:

| Variable | Value |
| --- | --- |
| `CONFIG_PATH` | Directory containing the config. |
| `FILES` | `$CONFIG_PATH/files` — a convenient place for dotfiles and assets. |

After a successful run a marker file is written to `$XDG_STATE_HOME/simplepackageinstaller/<name>.done` (default `~/.local/state/simplepackageinstaller/`), so the script is not run again. Delete the marker or press `R` to run it again; set `always: true` to run it on every install.

A typical layout:

```
my-setup/
├── packages.yaml
├── packages.schema.json
├── scripts/
│   └── paru.sh
└── files/
    └── ...
```

### Non-interactive commands

All commands run without a terminal (stdin is closed), so they must not prompt:

- `sudo` is wrapped with `sudo -n` via a shim in `PATH`; the password entered in the TUI is cached and kept alive for the whole run.
- Git never prompts for credentials (`GIT_TERMINAL_PROMPT=0`) and new SSH host keys are accepted automatically.

## Security

> [!WARNING]
> **Only use configs you trust.** Once the sudo password has been entered, every `script`, `post` and `service` step can run `sudo` without asking. A config is effectively code that runs as root.

How the sudo password is handled:

- It is only written to `sudo -S -v` over stdin. It never appears in command-line arguments, environment variables, logs or on disk, and is masked in the UI.
- `sudo -v` only validates the password and caches sudo's timestamp; child processes use that cache through the `sudo -n` shim and never see the password.
- The input buffer has a fixed capacity (so it never reallocates and leaves copies behind) and is zeroed after use.
- While running, the sudo timestamp is kept alive for the whole terminal session, not just for the installer's own jobs.
- On exit — including `q`, Ctrl-C, errors and panics — `sudo -k` is run to invalidate the cached credentials. This also clears any sudo cache you had in that terminal before starting.

## Files

| Path | Contents |
| --- | --- |
| `$XDG_CONFIG_HOME/simplepackageinstaller/packages.yaml` | Config, if not in the current directory or given with `--config`. |
| `$XDG_STATE_HOME/simplepackageinstaller/logs/<timestamp>.log` | Full log of each run. |
| `$XDG_STATE_HOME/simplepackageinstaller/*.done` | Markers for scripts that have run. |
| `$XDG_RUNTIME_DIR/simplepackageinstaller/shim/sudo` | The non-interactive sudo wrapper. Falls back to `$XDG_CACHE_HOME/simplepackageinstaller/shim/` without a runtime dir. |

Unset XDG variables fall back to the spec defaults: `~/.config`, `~/.local/state` and `~/.cache`.

## Development

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo build --release
```

### CI and releases

- **CI** (`.github/workflows/ci.yml`) runs on pull requests and pushes to `main`: `cargo fmt --check`, `cargo clippy -D warnings`, and a release build of the static binary.
- **Release** (`.github/workflows/release.yml`) runs when a GitHub release is published. The release tag is the version: `v1.2.3` (or `1.2.3`) must be semver and on `main`. The workflow sets that version in `Cargo.toml` before building, so `simplepackageinstaller --version` matches the release, then builds a static musl binary and attaches the binary, a tarball and checksums to the release.

To release, create a new release on GitHub (*Releases → Draft a new release*), create a tag such as `v0.2.0` on `main`, and publish. Or with the GitHub CLI:

```sh
gh release create v0.2.0 --target main --generate-notes
```
