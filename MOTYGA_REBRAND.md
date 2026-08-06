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
> (373 MB debug).
> **Superseded 2026-08-06 — see Part C.** This section originally forbade a global `sed codex→motyga` and
> kept internal crate names. That restriction is lifted: the crate layer and the `CODEX_*` env layer were
> renamed too. What survives the rename is now an explicit, enumerated set — read Part C before touching it.

1. ✅ **Installed binary name** — `motyga-rs/cli/Cargo.toml` `[[bin]] name = "codex"` → `"motyga"`;
   `codex-cli/bin/codex.js:91` (`codex.exe`/`codex`) → `motyga.exe`/`motyga`; `justfile` + `scripts/*.sh` +
   `.codex/environments/environment.toml` `--bin codex` → `--bin motyga` (kept `codex-file-search`/`codex-tui`/
   `codex-code-mode-host`/`codex-write-config-schema`). 32 `cargo_bin("codex")` test refs → `"motyga"`.
   `[package]`/`[lib]` names unchanged. **Verified:** `motyga.exe --version` → `codex-cli 0.0.0`.
2. ✅ **Config dir** — `motyga-rs/utils/home-dir/src/lib.rs::find_codex_home()`: read `MOTYGA_HOME`
   reads `MOTYGA_HOME` only (the legacy `CODEX_HOME` fallback was removed — the CLI has no OpenAI/Codex ties;
   all tests/harnesses set `MOTYGA_HOME`), default `~/.codex` → `~/.motyga`.
   Project-local `.codex/` → `.motyga/` **DONE 2026-08-04** (was deferred): the loader, the import targets in
   `external_agent_config`, both sandboxes' protected-subpath lists, the Windows cwd junction root and the
   `state` log-dir fallback all follow `.motyga/` now, and this repo's own `.codex/` was renamed. Left alone on
   purpose: `supply/vendor.rs` reads `~/.codex/auth.json` deliberately — that is the EXTERNAL OpenAI Codex CLI's
   credential, which supply mode resells — and the hyphenated names (`.codex-plugin`, `.codex-log`) are ecosystem
   conventions, not our config dir. **Verified:** `cargo check --workspace --all-targets` clean.
   Why it mattered: with project-local still `.codex/`, starting the CLI from `~` loaded `~/.codex/config.toml`
   — the *OpenAI Codex* config — as a project layer, silently importing its `model`/`model_reasoning_effort`.
   **Verified:** `MOTYGA_HOME=… motyga exec` honored the override.
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

## ✅ Part C — full rename, including internals (2026-08-06)

Owner call: the Codex identity comes out everywhere, not just off the user-visible surface. Three sweeps ran
over the tree with an explicit protected set; 3152 paths touched.

- **Repo surface** — `codex-cli/` → `npm/` (it is the npm wrapper, and the repo itself is already motyga-cli);
  `bin/codex.js` → `bin/motyga.js`; `scripts/build_codex_package.py` → `build_motyga_package.py`;
  `scripts/codex_package/` → `motyga_package/`; `.github/codex/` → `.github/motyga/`;
  `.devcontainer/codex-install/` → `motyga-install/`; release matrix `pkg: codex-*` → `motyga-*`.
- **SDKs** — the two OpenAI-branded packages are gone: python `openai-codex` → `motyga-sdk` (module
  `openai_codex` → `motyga_sdk`), runtime `openai-codex-cli-bin` → `motyga-cli-bin`, TypeScript
  `@openai/codex-sdk` → `@motyga/sdk`.
- **Crates** — 131 crates `codex-*` → `motyga-*`, six crate dirs renamed to match, plus every `codex_*` path
  in source. 571 files renamed, of which 543 are insta snapshots (their filenames encode the crate name, so
  they must move with it or the tests stop resolving).
- **Env** — `CODEX_*` → `MOTYGA_*` across the tree.

### Protected — do NOT rename these, any future sweep must keep excluding them
These are not leftovers. Each one is the *external, real* OpenAI Codex, and renaming it breaks behaviour:

| Survivor | Why it must stay |
|---|---|
| `motyga-rs/supply/**` (whole subtree) | supply mode **resells an OpenAI Codex subscription**. `CodexAdapter`, its doc comments and its wire values describe *their* product. |
| `.codex/auth.json` in `supply/vendor.rs` | the external Codex CLI's credential — the thing being resold. |
| `originator: codex_cli_rs` | sent to `chatgpt.com/backend-api/codex`; we identify *as* the Codex CLI there. Our own originator is separately `motyga_cli`. |
| `x-codex-*` headers | OpenAI response headers (quota percentages). |
| `gpt-*-codex*` model ids | real OpenAI model names, incl. the prompt files named after them (`gpt-5.2-codex_prompt.md` …). |
| `codex-mini-latest` | also a real OpenAI model id, but with no `gpt-` prefix — a `gpt-*-codex*` pattern does **not** catch it. A sweep already broke this one once. |
| `openai/codex` URLs | upstream provenance and backport references. |
| `.codex-plugin`, `.codex-log` | ecosystem conventions, not our config dir. |
| `LICENSE`, `NOTICE` | Apache-2.0 §4 attribution to OpenAI — legally required. |
| `x-openai-internal-codex-*` | OpenAI server-owned request headers (residency, responses-lite). An `x-codex-*` pattern does **not** catch these; a sweep already renamed both and they had to be reverted. |
| `com.openai.codex` | OpenAI's bundle id / keychain service. |
| anything under `github.com/openai/codex`, `developers.openai.com`, `chatgpt.com`, `auth.openai.com` | their URLs, their docs routes, and the **asset names in their GitHub releases** — `codex-zsh`, `codex-shell-tool-mcp`. The DotSlash manifest at `scripts/motyga_package/codex-zsh` fetches one of those artifacts, so renaming it breaks the build. |
| `OpenAI/Codex`, `GPT-5.x-Codex` | capitalised spellings. The first sweep's mask was case-sensitive and mangled both; match case-insensitively. |
| "fork of the open-source OpenAI Codex CLI" | attribution in the runtime base instructions and READMEs. A sweep turned this into the non-existent "OpenAI Motyga CLI", which shipped to the model at runtime. |

### Failure modes a text sweep creates that a compiler cannot catch
All three of these compiled cleanly and only surfaced when the tests were actually run:

1. **A hardcoded digest of a renamed string.** `storage_tests.rs` asserts `compute_store_key("~/.codex")`
   equals a literal sha256 prefix. Renaming the path without recomputing the digest breaks the test.
2. **A negative assertion that collides with a renamed constant.** The login tests used `"codex_cli"` as
   the value that is *not* first-party. Renamed to `"motyga_cli"` it became `DEFAULT_ORIGINATOR`, so the
   assertion demanded `false` from something that is now `true`.
3. **Fixed-width TUI snapshots.** `motyga` is one character longer than `codex`, so every right-aligned
   status line re-pads and every wrapped paragraph re-wraps. The text inside a `.snap` gets substituted,
   but its layout does not — those snapshots must be regenerated, not edited.

### Known cost, accepted
Backports from `openai/codex` (e.g. `a0b3fa9`) now conflict line-by-line on every renamed identifier.

## Definition of done
`npm i -g @motyga/cli` → `motyga` on PATH; `motyga exec "<prompt>"` runs against `api.motyga.com/v1`
(`wire_api=responses`), auth via `MOTYGA_API_KEY`, zero OpenAI-identity calls.
**Functional DoD met locally (items 1–5, live-verified); remaining = publish (7) + live apply_patch probe (6) + cosmetic sweep (8).**
