import sys
from pathlib import Path

_EXAMPLES_ROOT = Path(__file__).resolve().parents[1]
if str(_EXAMPLES_ROOT) not in sys.path:
    sys.path.insert(0, str(_EXAMPLES_ROOT))

from _bootstrap import ensure_local_sdk_src, runtime_config

ensure_local_sdk_src()

from motyga_sdk import Motyga

with Motyga(config=runtime_config()) as motyga:
    thread = motyga.thread_start(model="gpt-5.4", config={"model_reasoning_effort": "high"})
    first = thread.turn("One sentence about structured planning.").run()
    second = thread.turn("Now restate it for a junior engineer.").run()

    reopened = motyga.thread_resume(thread.id)
    listing_active = motyga.thread_list(limit=20, archived=False)
    reading = reopened.read(include_turns=True)

    _ = reopened.set_name("sdk-lifecycle-demo")
    _ = motyga.thread_archive(reopened.id)
    listing_archived = motyga.thread_list(limit=20, archived=True)
    unarchived = motyga.thread_unarchive(reopened.id)

    resumed = motyga.thread_resume(
        unarchived.id,
        model="gpt-5.4",
        config={"model_reasoning_effort": "high"},
    )
    resumed_result = resumed.turn("Continue in one short sentence.").run()

    forked = motyga.thread_fork(unarchived.id, model="gpt-5.4")
    forked_result = forked.turn("Take a different angle in one short sentence.").run()

    compact_result = unarchived.compact()

    print("Lifecycle OK:", thread.id)
    print("first:", first.id, first.status)
    print("second:", second.id, second.status)
    print("read.turns:", len(reading.thread.turns))
    print("list.active:", len(listing_active.data))
    print("list.archived:", len(listing_archived.data))
    print("resumed:", resumed_result.id, resumed_result.status)
    print("forked:", forked_result.id, forked_result.status)
    print("compact:", compact_result.model_dump(mode="json", by_alias=True))
