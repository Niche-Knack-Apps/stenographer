# Stenographer

[![ci](https://github.com/Niche-Knack-Apps/stenographer/actions/workflows/ci.yml/badge.svg)](https://github.com/Niche-Knack-Apps/stenographer/actions/workflows/ci.yml)
[![release](https://img.shields.io/github/v/release/Niche-Knack-Apps/stenographer)](https://github.com/Niche-Knack-Apps/stenographer/releases/latest)
[![license](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

Shell history search. A background daemon records every command you run across all terminals; a CLI lets you search and replay them instantly.

Made by [Niche-Knack](https://nk.tools) — Smart Tools. Sharp Solutions.

## Install

Linux, amd64 or arm64. Each release has two static binaries — `steno` (the CLI)
and `stenographerd` (the daemon) — that only need to be put on your PATH. Then
`steno install` sets up the systemd user service and the shell hooks and prints
the one `source` command to run:

```bash
arch=amd64   # or arm64
base=https://github.com/Niche-Knack-Apps/stenographer/releases/latest/download
curl -LO $base/steno_linux_$arch -LO $base/stenographerd_linux_$arch -LO $base/SHA256SUMS
sha256sum --ignore-missing -c SHA256SUMS
install -m 0755 steno_linux_$arch ~/.local/bin/steno
install -m 0755 stenographerd_linux_$arch ~/.local/bin/stenographerd
steno install
```

From source:

```bash
make install        # build release + install + print source command
source ~/.zshrc     # activate in current shell (or restart terminal)
```

`make install` compiles both binaries, copies them to `~/.local/bin/`, installs the systemd user service, and adds shell integration to your RC files. It prints the exact `source` command to run at the end. Make runs in a subprocess so it cannot source your shell itself — you need to run that one command.

To upgrade: `make install && source ~/.zshrc` — the hook files are regenerated on every install.

## Usage

Open history search from anywhere:

```
steno              # search all recorded commands (default)
steno history      # same as above
steno here         # search commands run in the current directory
```

Or press **Ctrl-R** from any prompt.

Select a command and press:
- **Enter** — place it at your prompt, ready to edit or run
- **Alt-Enter** — execute it immediately in your current shell

## Picker keys

| Key | Action |
|---|---|
| Type | Filter by substring (all tokens must match) |
| `↑` / `↓` | Navigate |
| `Enter` | Place selected command at prompt |
| `Alt-Enter` | Execute selected command immediately |
| `Esc` | Cancel |

The right pane shows the full command, working directory, exit code, duration, git branch, and surrounding context.

## How many commands the picker loads

By default the picker loads the **500** most recent commands. Raise it for a single search with `--limit`, or change the default permanently in config:

```bash
steno --limit 5000     # load up to 5000 this time
steno --limit 0        # load everything (0 or -1 = unlimited)
steno here --limit -1  # same, scoped to the current directory
```

To make it permanent, set `history_limit` in `~/.config/stenographer/config.toml` (`0` or `-1` = unlimited):

```toml
history_limit = 0
```

The pool is still bounded by `retention_days` — unlimited means "everything still retained," not "forever."

## Commands

| Command | Description |
|---|---|
| `steno` / `steno history` | Search all recorded commands |
| `steno here` | Search commands run in the current directory |
| `steno prime` | Import existing shell history into the database |
| `steno install` | First-time setup: binaries, service, shell hooks |
| `steno hook install` | Re-install / refresh shell hooks |
| `steno hook uninstall` | Remove shell hooks |
| `steno hook status` | Show hook installation status |
| `steno daemon start\|stop\|restart\|logs` | Control the background daemon |
| `steno version` / `steno -v` | Print version |
| `steno completions <shell>` | Print shell completions |

## Priming from existing history

Import your existing shell history on first setup:

```bash
steno prime                   # auto-detect shell (zsh/bash/fish)
steno prime --shell zsh       # specify shell explicitly
steno prime --limit 50000     # raise the import limit (default: 10 000)
steno prime --dry-run         # preview without importing
```

Noise commands (`ls`, `clear`, `cd ..`, etc.) are filtered out. Duplicates are deduplicated before import. After priming, `steno` shows results immediately without waiting for new history to accumulate.

## Shell completions

```bash
# zsh
steno completions zsh > ~/.zfunc/_steno
# add to ~/.zshrc: fpath=(~/.zfunc $fpath) && autoload -U compinit && compinit

# bash
steno completions bash >> ~/.bash_completion

# fish
steno completions fish > ~/.config/fish/completions/steno.fish
```

## Architecture

Two binaries: `stenographerd` (background daemon, SQLite, Unix socket) and `steno` (CLI client). Shell hooks write pre/post-exec events to a spool file; the daemon picks them up via inotify and stores them in SQLite. The CLI queries the daemon over a Unix socket.

**Shell integration** uses a temp-file pattern (same as atuin): the binary writes the selected command to `STENO_RESULT_FILE`; the shell wrapper reads it and either calls `print -z` (place at prompt) or `eval` (execute immediately). This means `cd`, `export`, and other builtins work correctly.

**Absolute paths.** When the daemon records a command it rewrites relative path arguments to absolute paths rooted at the command's working directory (e.g. `cat zarf/x/setup.sh` → `cat /home/me/proj/zarf/x/setup.sh`), so replaying from a different directory still works. A one-time backfill rewrites existing history on the next upgrade. This is a whitespace-token heuristic gated on the file actually existing — **not** a full shell parser: quoted paths with spaces, punctuation-adjacent paths, globs, and command substitution are left untouched, and bare words without a slash or extension (`cargo test`, `make build`) are never rewritten. The backfill resolves against the *current* filesystem, so history whose files have since moved stays as-is.

If the daemon isn't running:

```
✗ Daemon is not running.
  First-time setup:  steno install
  Start the daemon:  steno daemon start
  Check the logs:    steno daemon logs
```

## Without systemd

Add to `~/.profile`:

```bash
if ! pgrep -u "$USER" stenographerd >/dev/null 2>&1; then
    nohup stenographerd >> ~/.local/share/com.niche-knack.stenographer/logs/daemon.log 2>&1 &
fi
```

## Build

```bash
make dev        # debug build + start daemon
make build      # release build → dist/
make install    # build + install + print source command
make verify     # clippy + tests + fmt check
```

## Releases

Every merge to `main` runs formatting, clippy and the tests. When the merge carries
a new version in `VERSION` (kept equal to `Cargo.toml`), the release workflow builds
static amd64 and arm64 Linux binaries of `steno` and `stenographerd`, tags `vX.Y.Z`,
and publishes them with `SHA256SUMS` as a GitHub release. A pull request that does
not bump `VERSION` fails its check, so every merge ships.

## Support

Stenographer is free and open. Niche-Knack also makes small, sharp, offline-first
apps — see them at [nk.tools](https://nk.tools).

Really believe in this project's potential and want to contribute to its
development? [Buy Me a Coffee](https://buymeacoffee.com/nicheknack).

## License

[Apache License 2.0](LICENSE). See [NOTICE](NOTICE).
