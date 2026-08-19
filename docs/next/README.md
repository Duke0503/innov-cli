# innov


<p align="center">
  <img src="assets/logo.png" alt="innov" width="100" />
</p>

<p align="center">
  <a href="https://innov.dev">innov.dev</a> · <a href="#install">install</a> · <a href="https://innov.dev/docs/quick-start/">quick start</a> · <a href="https://innov.dev/docs/">docs</a>
</p>

<p align="center">
  English · <a href="README.zh-CN.md">简体中文</a>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-666666?labelColor=333333" alt="Apache 2.0 license" /></a>
  <a href="https://github.com/Duke0503/cli/releases"><img src="https://img.shields.io/github/downloads/Duke0503/cli/total?labelColor=333333&color=666666" alt="total GitHub release downloads" /></a>
  <a href="https://github.com/Duke0503/cli/stargazers"><img src="https://img.shields.io/github/stars/Duke0503/cli?labelColor=333333&color=666666&logo=github" alt="GitHub stars" /></a>
  <a href="https://github.com/Duke0503/cli/releases/latest"><img src="https://img.shields.io/github/v/release/Duke0503/cli?label=release&labelColor=333333&color=666666" alt="latest stable release" /></a>
  <a href="https://formulae.brew.sh/formula/innov"><img src="https://img.shields.io/homebrew/v/innov?label=homebrew&labelColor=333333&color=666666" alt="Homebrew version" /></a>
  <a href="https://x.com/innovdev"><img src="https://img.shields.io/badge/follow-%40innovdev-000000?logo=x&logoColor=white" alt="follow @innovdev on X" /></a>
</p>

---

https://github.com/user-attachments/assets/043ec09f-4bdd-41d5-aee0-8fda6b83e267

**the runtime your coding agents live on.**

- **always running** — innov is a background server; the terminals live inside it. close the lid, drop the network, or restart the machine; agents keep working and sessions come back. reattach from any terminal, or over ssh.
- **never hunt for the stuck one** — every pane is marked working, blocked, or idle. when an agent stops and needs an answer, innov says so.
- **agent-native** — agents drive innov through the innov and socket api: they can spawn panes, prompt each other, and wait until another agent is genuinely blocked. [agent skill →](https://innov.dev/docs/agent-skill/)
- **runs what you already run** — claude code, codex, cursor, opencode, grok and the rest. innov doesn't wrap or replace them; it owns their terminals.
- **keyboard and mouse, both first-class** — tmux-style prefix keys *and* click, drag, split. pick per moment, not per tool.
- **plugins** — extend panes and workflows. [browse the marketplace →](https://innov.dev/plugins/)
- **one rust binary, no electron** — runs in whatever terminal you already use.

---

## install

```bash
curl -fsSL https://innov.dev/install.sh | sh
```

or `brew install innov` · `mise use -g innov` · windows beta: `powershell -ExecutionPolicy Bypass -c "irm https://innov.dev/install.ps1 | iex"` · [binaries](https://github.com/Duke0503/cli/releases)

then start it where the work lives:

```bash
innov
```

run your agents, split panes, walk away. `ctrl+b q` detaches, `innov` reattaches. [quick start →](https://innov.dev/docs/quick-start/)

## docs

everything lives at [innov.dev/docs](https://innov.dev/docs/): [quick start](https://innov.dev/docs/quick-start/) · [concepts](https://innov.dev/docs/concepts/) · [supported agents](https://innov.dev/docs/agents/) · [keyboard](https://innov.dev/docs/keyboard/) · [configuration](https://innov.dev/docs/configuration/) · [session state](https://innov.dev/docs/session-state/) · [remote](https://innov.dev/docs/persistence-remote/) · [integrations](https://innov.dev/docs/integrations/) · [plugins](https://innov.dev/docs/plugins/) · [socket api](https://innov.dev/docs/socket-api/)

## thanks

every past sponsor and backer is listed in [SPONSORS.md](./SPONSORS.md) — thank you 🐑

enterprise / partnership: hey@innov.dev

## agent instructions

if you are an ai agent helping with this repository, read [`AGENTS.md`](./AGENTS.md) before making changes and read [`CONTRIBUTING.md`](./CONTRIBUTING.md) before opening issues or PRs.

## development

```bash
git clone https://github.com/Duke0503/cli
cd cli
cargo build --release

just test        # unit tests
just check       # formatting, tests, and maintenance checks
```

## license

Innov is licensed under the [Apache License 2.0](LICENSE).
