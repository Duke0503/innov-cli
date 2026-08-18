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
