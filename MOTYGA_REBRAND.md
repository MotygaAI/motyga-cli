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

## ✅ Part B — functional rebrand (items 1–5 DONE 2026-07-02, built + live-verified on branch `motyga-partb`)
> Built with Rust 1.95.0 (pinned) + MSVC on Windows; `cargo build --bin motyga` → `target/debug/motyga.exe`
> (373 MB debug). **Do NOT `sed codex→motyga` globally** — internal crate names (`codex-core`, `codex-cli`, …)
> stay; only the user-visible surface was rebranded.

1. ✅ **Installed binary name** — `codex-rs/cli/Cargo.toml` `[[bin]] name = "codex"` → `"motyga"`;
   `codex-cli/bin/codex.js:91` (`codex.exe`/`codex`) → `motyga.exe`/`motyga`; `justfile` + `scripts/*.sh` +
   `.codex/environments/environment.toml` `--bin codex` → `--bin motyga` (kept `codex-file-search`/`codex-tui`/
   `codex-code-mode-host`/`codex-write-config-schema`). 32 `cargo_bin("codex")` test refs → `"motyga"`.
   `[package]`/`[lib]` names unchanged. **Verified:** `motyga.exe --version` → `codex-cli 0.0.0`.
2. ✅ **Config dir** — `codex-rs/utils/home-dir/src/lib.rs::find_codex_home()`: read `MOTYGA_HOME`
   (**back-compat**: falls back to `CODEX_HOME` — keeps ~20 integration tests green), default `~/.codex` → `~/.motyga`.
   Project-local `.codex/` → `.motyga/` intentionally DEFERRED (would churn `external_agent_config` tests). **Verified:**
   `MOTYGA_HOME=… motyga exec` honored the override.
3. ✅ **Default provider = Motyga** — `model-provider-info/src/lib.rs`: `MOTYGA_PROVIDER_ID`/name/base-url/env-key
   consts + `create_motyga_provider()` (base_url `https://api.motyga.com/v1`, `wire_api=Responses`,
   `env_key=MOTYGA_API_KEY`, `requires_openai_auth=false`) registered first in `built_in_model_providers`.
   Default id flipped `"openai"`→`"motyga"` at `core/src/config/mod.rs:3405`. `CODEX_API_KEY_ENV_VAR` const VALUE
   → `"MOTYGA_API_KEY"` (`login/src/auth/manager.rs:839`; identifier kept). NOTE: `disable_response_storage` is NOT a
   `ModelProviderInfo` field — the Responses `store` flag derives from `is_azure_responses_endpoint()` (false for
   api.motyga.com) so storage is already off; no change needed. **Verified:** `motyga exec` → `provider: motyga` +
   `ERROR: Missing environment variable: MOTYGA_API_KEY` (fails on env_key before any network call).
4. ✅ **Disable ChatGPT login** — reused the existing `forced_login_method` gate: defaulted it to
   `ForcedLoginMethod::Api` at `core/src/config/mod.rs:3547` (opt back in via `forced_login_method="chatgpt"`).
   Every OAuth entry point (CLI `login`, TUI onboarding, app-server, AuthManager) already honors it → no OpenAI
   identity endpoint contacted by default. **Verified:** `motyga login` → "ChatGPT login is disabled. Use API key
   login instead." Test ripple applied: 2 `cli/tests/login.rs` tests re-enable chatgpt via `-c`.
5. ✅ **Disable telemetry** — `analytics_enabled` defaulted to `Some(false)` at `core/src/config/mod.rs:3930`
   (`.or(Some(false))`; honors explicit `[analytics] enabled = true`). `AnalyticsEventsClient::new` then builds
   no delivery queue → no network events. (Kept opt-in; no dead code.)

### ⏳ Part B — remaining (owner / follow-up)
6. **`apply_patch` note** — freeform `apply_patch` needs a **Responses-native** model; `glm-5.2@zai` returns 400 on
   patch. Default coding to a Responses-native model; keep `glm-5.2@zai` for chat. Smoke-test a real `motyga exec`
   patch task once a live `MOTYGA_API_KEY` is available (owner / QA `user_id=32`).
7. **Publish pipeline** (owner, needs npm creds) — build per-platform Rust bins; publish `@motyga/cli` +
   `@motyga/cli-<plat>`; rebuild CI (`.github/workflows/`, needs `workflow` OAuth scope).
8. **Cosmetic display strings** (polish; NOT identity *calls*) — hardcoded "codex"/"OpenAI Codex" in help/banners:
   `cli/src/main.rs:103` `bin_name="codex"` + `:91` about "Codex CLI"; `exec/.../event_processor_with_human_output.rs:218`
   "OpenAI Codex v{VERSION}"; `tui/src/history_cell/session.rs:338/405` + `tui/src/status/card.rs:713` "OpenAI Codex";
   `marketplace_cmd.rs`/`plugin_cmd.rs` `bin_name="codex plugin …"`; `-c` help still shows `~/.codex/config.toml`.
   Distributed surface — do as a dedicated sweep.

## Definition of done
`npm i -g @motyga/cli` → `motyga` on PATH; `motyga exec "<prompt>"` runs against `api.motyga.com/v1`
(`wire_api=responses`), auth via `MOTYGA_API_KEY`, zero OpenAI-identity calls.
**Functional DoD met locally (items 1–5, live-verified); remaining = publish (7) + live apply_patch probe (6) + cosmetic sweep (8).**
