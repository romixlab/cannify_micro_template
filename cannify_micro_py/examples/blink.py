"""Blink the Cannify's LED and print what runs on it. From the repo root: `just py-example`, or
`uv run --project cannify_micro_py python cannify_micro_py/examples/blink.py [SERIAL]`."""

import sys

import cannify_micro

serial = sys.argv[1] if len(sys.argv) > 1 else None
with cannify_micro.Device(serial=serial) as dev:
    print(dev.api())
    print(dev.firmware())
    print("label:", dev.label or "(none)")
    print("boot count:", dev.boot_count())
    dev.blink(count=3, period_ms=400)
