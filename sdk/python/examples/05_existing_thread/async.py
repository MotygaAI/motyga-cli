import sys
from pathlib import Path

_EXAMPLES_ROOT = Path(__file__).resolve().parents[1]
if str(_EXAMPLES_ROOT) not in sys.path:
    sys.path.insert(0, str(_EXAMPLES_ROOT))

from _bootstrap import ensure_local_sdk_src, runtime_config

ensure_local_sdk_src()

import asyncio

from motyga_sdk import AsyncMotyga


async def main() -> None:
    async with AsyncMotyga(config=runtime_config()) as motyga:
        original = await motyga.thread_start(
            model="gpt-5.4", config={"model_reasoning_effort": "high"}
        )

        first_turn = await original.turn("Tell me one fact about Saturn.")
        _ = await first_turn.run()
        print("Created thread:", original.id)

        resumed = await motyga.thread_resume(original.id)
        second_turn = await resumed.turn("Continue with one more fact.")
        second = await second_turn.run()
        print(second.final_response)


if __name__ == "__main__":
    asyncio.run(main())
