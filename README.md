<h1 align="center">Motyga CLI</h1>

<p align="center"><strong><code>motyga</code></strong> is a local coding agent that runs on your machine
and talks to <a href="https://motyga.com">Motyga</a> by default. It speaks the <b>Responses API</b>, so it
runs against Motyga with your <code>nb-…</code> key (or any provider you configure) — no third-party account
or subscription needed.</p>

## Quickstart

```shell
npm install -g @motyga/cli
```

> **Supported platforms.** Prebuilt binaries are published for **Windows** (`x64` and `arm64`).
> macOS and Linux builds are produced on request — open an issue and we'll publish the package for your
> OS/arch. The release pipeline already covers all six targets (win/mac/linux × x64/arm64); the
> non-Windows legs are gated off for now, so an unpublished platform is a clean skip, not a broken install.

Motyga is the **built-in default provider**, so all you need is your API key:

```shell
# macOS / Linux
export MOTYGA_API_KEY=nb-YOUR_KEY

# Windows PowerShell
$env:MOTYGA_API_KEY = "nb-YOUR_KEY"
```

```shell
motyga              # interactive
motyga exec "..."   # headless
```

Get an `nb-…` key at [motyga.com](https://motyga.com).

### Configuration (optional)

`motyga` keeps its config and credentials in `~/.motyga/` — override with the `MOTYGA_HOME` environment
variable. You only need a `~/.motyga/config.toml` to change a default, for example to use the
**RU mirror** or point at another provider:

```toml
[model_providers.motyga]
name = "Motyga"
base_url = "https://ru.motyga.com/v1"   # RU mirror; default is https://api.motyga.com/v1
env_key = "MOTYGA_API_KEY"
wire_api = "responses"
```

## Highlights

- **Default provider is Motyga** — `https://api.motyga.com/v1`, Responses API, auth via `MOTYGA_API_KEY`.
- **API-key auth only** — no third-party login; no identity or telemetry calls.
- **Telemetry is off by default.**
- **Config directory is `~/.motyga`.**

## Docs

- Using Motyga with the CLI: [motyga.com](https://motyga.com)

---

## Notice

Motyga CLI is a fork of [OpenAI Codex CLI](https://github.com/openai/codex), licensed under the
[Apache-2.0 License](LICENSE). The upstream `LICENSE` and `NOTICE` are retained as required.
