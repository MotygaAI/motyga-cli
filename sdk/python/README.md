# Motyga Python SDK (Beta)

Build Python applications that start Motyga threads, run turns, stream progress,
and control workspace access.

## Install

Install the SDK:

```bash
pip install motyga-sdk
```

## Quickstart

The SDK reuses your existing Motyga authentication when one is already
available:

```python
from motyga_sdk import Motyga

with Motyga() as motyga:
    thread = motyga.thread_start()
    result = thread.run("Explain this repository in three bullets.")
    print(result.final_response)
```

`thread.run(...)` returns a `TurnResult` containing the final response,
collected items, and token usage.

## Authentication

Existing Motyga authentication is reused automatically. To start ChatGPT
browser login explicitly:

```python
from motyga_sdk import Motyga

with Motyga() as motyga:
    login = motyga.login_chatgpt()
    print(login.auth_url)
    print(login.wait().success)
```

For device-code login:

```python
with Motyga() as motyga:
    login = motyga.login_chatgpt_device_code()
    print(login.verification_url, login.user_code)
    login.wait()
```

For API-key login:

```python
with Motyga() as motyga:
    motyga.login_api_key("sk-...")
```

## Built-In Help

Use Python's standard `help(motyga_sdk)`, `help(Motyga)`, or
`python -m pydoc motyga_sdk` documentation tools.

## Documentation

- [Getting started](https://github.com/openai/codex/blob/main/sdk/python/docs/getting-started.md)
- [API reference](https://github.com/openai/codex/blob/main/sdk/python/docs/api-reference.md)
- [FAQ](https://github.com/openai/codex/blob/main/sdk/python/docs/faq.md)
- [Examples](https://github.com/openai/codex/blob/main/sdk/python/examples/README.md)

The package is licensed under the
[repository Apache License 2.0](https://github.com/openai/codex/blob/main/LICENSE).
