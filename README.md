# cli

`cli` is a fork of [Herdr](https://github.com/herdrdev/herdr) for running
multiple coding-agent CLIs in one persistent terminal workspace.

It keeps Herdr's server, PTY, pane, workspace, and native-session handling,
then adds a first-class handoff command. The handoff is explicit and bounded:
the new agent receives a packet containing the workspace path, git status,
diff summary, and recent terminal output. The workspace and git state remain
the source of truth.

## Build

Requirements:

- Rust toolchain
- Zig, required to build the vendored `libghostty-vt`

```bash
brew install zig
cargo build --release
```

The resulting binary is `target/release/cli`.

## Run

Start or attach to the persistent workspace:

```bash
cli
```

Switch from an existing agent to another CLI in the same workspace:

```bash
cli switch <agent-target> --to codex
cli switch <agent-target> --to claude --context-tokens 2500
cli switch <agent-target> --to agy --no-handoff
```

When an agent pane is focused, the same switch can be started from inside the
running session:

```text
/switch agy
/switch co-ht
```

The focused agent is used as the source automatically. The command creates and
focuses the target pane in the current session; the original pane remains
available. Other slash input is forwarded to the agent unchanged.

## Profiles and aliases

Profiles port the AgentPort idea into `cli`: one named alias owns its provider,
model, key source, and permission level. The registry is stored at
`~/.config/cli/profiles.toml` (or the platform/config-dir equivalent), while
CLI-specific overlays are written beside the native Claude/Codex config.

Use an environment reference for keys when possible:

```bash
cli profile add cx-luna --cli codex \
  --provider luna --base-url https://gateway.example/v1 \
  --api-key-env LUNA_CODEX_KEY --model luna-max --danger bypass

cli profile add cc-luna --cli claude \
  --provider luna --base-url https://gateway.example \
  --api-key-env LUNA_CLAUDE_KEY --model luna-max --danger accept_edits

cli profile install
cli profile list
```

After opening a new shell, the generated functions can be called directly:

```bash
cx-luna --help
cc-luna --resume
cli switch reviewer --to cx-luna
```

Use `--api-key-stdin` instead of `--api-key` when creating a profile without
putting the secret in the process argument list. Inline keys are supported for
compatibility and are stored in the private profile registry. `--command` can
name an existing shell command or alias for the generated function. When that
profile is used with `cli switch`, the command is sent to the new pane too, so
the alias must be available in that pane's shell; without `--command`, `cli`
uses the native backend executable.

The permission scale is translated per backend: `ask`, `accept_edits`,
`workspace_write`, and `bypass`. Claude maps `workspace_write` to
`acceptEdits`; Codex maps it to `approval_policy=never` plus
`sandbox_mode=workspace-write`. `bypass` enables full Codex access and Claude
`bypassPermissions`, so it should be used only for trusted workspaces.

The original pane remains available. `cli switch` creates a new pane, starts
the selected native CLI directly through the internal API, sends the handoff
packet, and focuses the new pane. It does not invoke an installed `herdr`
binary.

## Context Guarantees

Native Codex, Claude, and Agy conversation histories remain separate. The
handoff reduces context loss by making the transfer deterministic, but it
cannot recreate hidden provider-side context. The receiving agent is told to
inspect the workspace and diff before acting.

Handoff packets are stored under `~/.config/cli/handoffs` and are bounded by
`--context-tokens` (approximately four characters per token).

## Development

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

This fork is licensed under the Apache License 2.0. See [LICENSE](LICENSE).
