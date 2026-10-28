"""Read everything the board's device contract serves: board, ids, chip, counters, log, settings, boot state and the
update node. From the repo root: `just py-example device [SERIAL]`, or `uv run --project cannify_micro_py python
cannify_micro_py/examples/device.py [SERIAL]`. Counters and the log are decoded with the firmware's ELF: `just
flash` or `just run` puts it where it is found by its build id."""

import sys

import cannify_micro

serial = sys.argv[1] if len(sys.argv) > 1 else None
with cannify_micro.Device(serial=serial) as dev:
    print("board:", dev.board())
    print("uid:", dev.uid())
    print("variant:", dev.variant())
    print("last reset:", dev.last_reset())
    print("counters:", dev.counters())
    print("led at boot:", dev.led_at_boot)
    print("boot state:", dev.boot_state())
    print(dev.dfu_node())
    print("last log lines:")
    for line in dev.log()[-5:]:
        print("  ", line)
