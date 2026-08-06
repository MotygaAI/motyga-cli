#!/bin/bash

# Set "chatgpt.cliExecutable": "/Users/<USERNAME>/code/motyga/scripts/debug-motyga.sh" in VSCode settings to always get the 
# latest motyga-rs binary when debugging Motyga Extension.


set -euo pipefail

MOTYGA_RS_DIR=$(realpath "$(dirname "$0")/../motyga-rs")
(cd "$MOTYGA_RS_DIR" && cargo run --quiet --bin motyga -- "$@")