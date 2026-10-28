"""Watch the board's shared clock follow this PC's UTC: the connection steers it by itself (WireWeaver's clock sync,
`WW_CLOCK_SYNC=off` turns it off). Prints, once a second, the offset the board still slews in and the residual: the
board's shared time minus UTC, known to within half the round trip. From the repo root:
`just py-example clock [SECONDS] [SERIAL]`."""

import statistics
import sys
import time

import cannify_micro

seconds = int(sys.argv[1]) if len(sys.argv) > 1 else 30
serial = sys.argv[2] if len(sys.argv) > 2 else None


def ms(us):
    return "-" if us is None else f"{us / 1000:+.3f} ms"


with cannify_micro.Device(serial=serial) as dev:
    print("waiting for the clock to lock...")
    dev.wait_clock_locked(timeout=60)
    residuals, rtts = [], []
    for _ in range(seconds):
        c = dev.clock_check()
        print(f"{c['state']:<9} offset (slew) {ms(c['slew_us'])}  residual {ms(c['residual_us'])} "
              f"± {c['rtt_us'] / 2000:.3f} ms  rate {c['rate_ppm'] or 0:+.1f} ppm")
        if c["residual_us"] is not None:
            residuals.append(c["residual_us"] / 1000)
            rtts.append(c["rtt_us"] / 1000)
        time.sleep(1)
    if residuals:
        print(f"residual over {len(residuals)} checks: mean {statistics.mean(residuals):+.3f} ms, "
              f"stdev {statistics.pstdev(residuals):.3f} ms, max |{max(map(abs, residuals)):.3f}| ms; "
              f"round trip median {statistics.median(rtts):.3f} ms, max {max(rtts):.3f} ms")
