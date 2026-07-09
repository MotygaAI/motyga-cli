# Configuration

For basic configuration instructions, see [this documentation](https://motyga.com/docs).

For advanced configuration instructions, see [this documentation](https://motyga.com/docs).

For a full configuration reference, see [this documentation](https://motyga.com/docs).

## Lifecycle hooks

Admins can set top-level `allow_managed_hooks_only = true` in
`requirements.toml` to ignore user, project, and session hook configs while
still allowing managed hooks from requirements and managed config layers. This
setting is only supported in `requirements.toml`; putting it in `config.toml`
does not enable managed-hooks-only mode.
