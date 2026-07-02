# Motyga CLI — rebrand status & checklist

This repo is a **fork of [OpenAI Codex CLI](https://github.com/openai/codex)** (Apache-2.0), rebranded to run
against **[Motyga](https://motyga.com)** by default. Rebrand is staged so the risky, build-required changes are
separated from the mechanical identity layer.

## Seed provenance / caveats
- `main` is **vendored from `openai/codex@129ea2a`** as a single squashed commit (LICENSE + NOTICE retained).
- **`.github/workflows/` was omitted** from the seed (the fork push lacked the GitHub `workflow` OAuth scope).
  CI is to be rebuilt for Motyga — see Part B. To restore full upstream history/CI later, re-import from upstream.

## ✅ Part A — mechanical identity + attribution (done in this PR, no Rust build)
- `codex-cli/package.json` — `name` → `@motyga/cli`; `bin` → `motyga`; `repository.url` → this repo; description.
- `codex-cli/bin/codex.js` — platform-package map → `@motyga/cli-<plat>`; install hints → `@motyga/cli`; banner/error text.
  (Kept: the vendored executable basename `codex`/`codex.exe` and the `CODEX_MANAGED_*` env vars — the **unchanged
  Rust binary** still expects those. They flip in Part B when the Rust bin is renamed.)
- `NOTICE` — Motyga modification notice prepended, upstream NOTICE retained (Apache-2.0 §4).
- `README.md` — Motyga banner + Quickstart prepended; upstream README kept below for reference.

## ⏳ Part B — code changes that REQUIRE `cargo build` (owner, Rust toolchain)
> Verified against upstream `main` (2026-07-02). **Do NOT `sed codex→motyga` globally** — it breaks internal
> crate names (`codex-core`, `codex_home`, `codex-cli`, thousands of deps). Rebrand only the user-visible surface.

1. **Installed binary name** — `codex-rs/cli/Cargo.toml`: `[[bin]] name = "codex"` → `"motyga"`.
   Then flip `codex-cli/bin/codex.js` line ~91 (`codex.exe`/`codex`) to `motyga.exe`/`motyga`, and update any
   scripts/`justfile`/tests that invoke the `codex` binary. (`[package] name`/`[lib] name` stay — internal.)
2. **Config dir** — crate `codex-rs/codex-home`: env `CODEX_HOME` → `MOTYGA_HOME`, default `~/.codex` → `~/.motyga`
   (change the canonical const/string; ~250 references read it by identifier). Also project-local `.codex/` → `.motyga/`.
3. **Default provider = Motyga** — `codex-rs/core/src/config/mod.rs` (`built_in_model_providers(...)` merge, ~line 3400)
   + crate `codex-model-provider-info`: add/point default `model_provider` → `"motyga"`
   (`base_url = https://api.motyga.com/v1`, `wire_api = responses`, `env_key = MOTYGA_API_KEY`,
   `disable_response_storage = true`). Rename `CODEX_API_KEY` env → `MOTYGA_API_KEY`.
4. **Disable ChatGPT login** — crate `codex-rs/login` (`device_code_auth.rs`, `server.rs`, `pkce.rs`, `auth/`) +
   `codex-rs/chatgpt`: default auth = API key; hide/disable the OAuth "Sign in with ChatGPT" path.
5. **Disable telemetry** — crate `codex-rs/analytics` (`analytics_capture.rs`, `client.rs`, `events.rs`).
6. **`apply_patch` note** — the freeform `apply_patch` tool needs a **Responses-native** model; on a chat-only
   provider (e.g. `glm-5.2@zai`) Motyga returns 400. Default coding workflows to a Responses-native model;
   keep `glm-5.2@zai` for plain chat. Smoke-test a real `motyga exec` patch task before release.
7. **Publish pipeline** — build per-platform Rust binaries; publish `@motyga/cli` + `@motyga/cli-<plat>` to npm;
   optional Homebrew tap / GitHub Releases. Rebuild CI (`.github/workflows/`) for Motyga.

## Definition of done
`npm i -g @motyga/cli` → `motyga` on PATH; `motyga exec "<prompt>"` runs against `api.motyga.com/v1`
(`wire_api=responses`), auth via `MOTYGA_API_KEY`, zero OpenAI-identity calls.
