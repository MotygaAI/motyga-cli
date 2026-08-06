# Getting Started

This guide gets a published Motyga Python SDK beta installation running
with a multi-turn thread.

## 1. Install

Install the SDK:

```bash
pip install motyga-sdk
```

Requirements:

- Python `>=3.10`
- An existing Motyga account session, or one of the login flows below

The SDK installs its compatible `motyga-cli-bin` runtime dependency
automatically. While beta releases are the only published SDK releases, this
normal install command selects the latest beta. After a stable release exists,
use `pip install --pre motyga-sdk` to opt into a newer prerelease.

## 2. Authenticate When Needed

Existing Motyga authentication is reused automatically. For ChatGPT browser
login:

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
    print(login.wait().success)
```

For API-key login:

```python
with Motyga() as motyga:
    motyga.login_api_key("sk-...")
    print(motyga.account().account)
```

## 3. Run A Turn

```python
from motyga_sdk import Motyga, Sandbox

with Motyga() as motyga:
    thread = motyga.thread_start(sandbox=Sandbox.workspace_write)
    result = thread.run("Say hello in one sentence.")

    print("Thread:", thread.id)
    print("Text:", result.final_response)
    print("Items:", len(result.items))
```

`Thread.run(...)` starts a turn, waits for completion, and returns
`TurnResult`. Plain strings are shorthand for `TextInput(...)`.

Use `Thread.turn(...)` when you need a `TurnHandle` for streaming, steering,
or interrupting an active turn.

## 4. Choose Sandbox Access

Use one enum for the initial thread and later turn overrides:

```python
from motyga_sdk import Motyga, Sandbox

with Motyga() as motyga:
    thread = motyga.thread_start(sandbox=Sandbox.workspace_write)
    thread.run("Make the requested changes.")
    review = thread.run("Review the diff only.", sandbox=Sandbox.read_only)
```

Available presets:

- `Sandbox.read_only`: read files without allowing writes.
- `Sandbox.workspace_write`: read files and write inside the workspace and
  configured writable roots; this is the normal default for workspace work.
- `Sandbox.full_access`: run without filesystem access restrictions.

When `sandbox=` is omitted, Motyga uses its configured default. A turn override
also applies to subsequent turns on that thread.

## 5. Continue A Thread

```python
from motyga_sdk import Motyga

with Motyga() as motyga:
    thread = motyga.thread_start()
    thread.run("Summarize Rust ownership in two bullets.")
    result = thread.run("Now explain it to a Python developer.")
    print(result.final_response)
```

To resume a stored thread later:

```python
with Motyga() as motyga:
    thread = motyga.thread_resume("thr_123")
    print(thread.run("Continue where we left off.").final_response)
```

## 6. Use The Async Client

```python
import asyncio

from motyga_sdk import AsyncMotyga, Sandbox


async def main() -> None:
    async with AsyncMotyga() as motyga:
        thread = await motyga.thread_start(sandbox=Sandbox.workspace_write)
        result = await thread.run("Continue where we left off.")
        print(result.final_response)


asyncio.run(main())
```

## 7. Get Help

Python's built-in documentation tools cover the curated SDK surface:

```python
import motyga_sdk
from motyga_sdk import Motyga, MotygaConfig

help(motyga_sdk)
help(Motyga)
help(MotygaConfig)
```

```bash
python -m pydoc motyga_sdk
```

## Developing From This Repository

Contributors working from a checkout can install development dependencies from
the repository:

```bash
cd sdk/python
uv sync --group dev
source .venv/bin/activate
```

## Next Stops

- [API reference](https://github.com/openai/codex/blob/main/sdk/python/docs/api-reference.md)
- [FAQ](https://github.com/openai/codex/blob/main/sdk/python/docs/faq.md)
- [Runnable examples](https://github.com/openai/codex/blob/main/sdk/python/examples/README.md)
