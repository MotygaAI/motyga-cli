import sys
from pathlib import Path

_EXAMPLES_ROOT = Path(__file__).resolve().parents[1]
if str(_EXAMPLES_ROOT) not in sys.path:
    sys.path.insert(0, str(_EXAMPLES_ROOT))

from _bootstrap import ensure_local_sdk_src, runtime_config, server_label

ensure_local_sdk_src()

from motyga_sdk import Motyga

with Motyga(config=runtime_config()) as motyga:
    print("server:", server_label(motyga.metadata))
    models = motyga.models()
    print("models.count:", len(models.data))
    print("models:", ", ".join(model.id for model in models.data[:5]))
