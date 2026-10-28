# /// script
# requires-python = ">=3.11"
# ///
"""Turn on the project's tracked git hooks (`.githooks/`): `just hooks` (`just setup` runs it too).

For every hook in `.githooks/` (today `pre-commit`, the API evolution check) it puts a small hook of the same name into
git's own hooks folder (`.git/hooks/`, shared by all worktrees of the clone). That hook first runs the hook that was
there before, kept next to it as `<name>.chained`, and then the tracked one from the checkout it runs in, so hooks you
or other tools installed keep working. `core.hooksPath` is not touched: when it is set, git reads hooks from that folder
only, and this script says what to add there instead of writing into it.

`--check` reports whether the hooks are on (exit 1 if not), `--remove` takes them out and puts the old hooks back.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TRACKED = ROOT / ".githooks"
MARK = "installed by scripts/hooks.py"


def git(*args: str) -> str | None:
    try:
        out = subprocess.run(["git", "-C", str(ROOT), *args], capture_output=True, text=True, check=False)
    except FileNotFoundError:
        return None
    return out.stdout.strip() if out.returncode == 0 else None


def hook_shim(name: str) -> str:
    return f"""#!/bin/sh
# {MARK} (`just hooks`): runs the hook that was here before ({name}.chained), then the project's tracked
# .githooks/{name} of the checkout git runs in. `just hooks --remove` takes it out and puts the old hook back.
chained="$(dirname "$0")/{name}.chained"
if [ -x "$chained" ]; then "$chained" "$@" || exit $?; fi
tracked="$(git rev-parse --show-toplevel)/.githooks/{name}"
if [ -x "$tracked" ]; then exec "$tracked" "$@"; fi
"""


def tracked_hooks() -> list[str]:
    return sorted(p.name for p in TRACKED.iterdir() if p.is_file()) if TRACKED.is_dir() else []


def installed(path: Path) -> bool:
    try:
        return MARK in path.read_text(errors="replace")
    except OSError:
        return False


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--check", action="store_true", help="only report whether the hooks are on; exit 1 if not")
    ap.add_argument("--remove", action="store_true", help="take the hooks out, put the hooks that were there back")
    args = ap.parse_args()

    if git("rev-parse", "--git-dir") is None:
        print("not a git checkout: no hooks to turn on")
        return 0
    names = tracked_hooks()
    hooks_path = git("config", "core.hooksPath")
    if hooks_path:
        print(
            f"git reads hooks from {hooks_path} (core.hooksPath), so .git/hooks is not used: have the hooks there run"
        )
        for name in names:
            print(f'  "$(git rev-parse --show-toplevel)/.githooks/{name}" "$@" || exit $?   # in {hooks_path}/{name}')
        return 0 if args.remove else 1
    hooks_dir = (ROOT / git("rev-parse", "--git-path", "hooks")).resolve()

    if args.check:
        missing = [n for n in names if not installed(hooks_dir / n)]
        print(f"not on: {', '.join(missing)} (just hooks)" if missing else f"on: {', '.join(names)} in {hooks_dir}")
        return 1 if missing else 0

    for name in names:
        hook, chained = hooks_dir / name, hooks_dir / f"{name}.chained"
        if args.remove:
            if installed(hook):
                hook.unlink()
                if chained.exists():
                    chained.rename(hook)
                print(f"{name}: removed" + (", the old hook is back" if hook.exists() else ""))
            continue
        if hook.exists() and not installed(hook):
            if chained.exists():
                print(f"{name}: both {hook} and {chained} exist and neither is this script's: leaving them alone")
                return 1
            hook.rename(chained)
            print(f"{name}: the hook that was there runs first, as {chained.name}")
        hooks_dir.mkdir(parents=True, exist_ok=True)
        hook.write_text(hook_shim(name), newline="\n")
        hook.chmod(0o755)
        print(f"{name}: on ({hook} runs .githooks/{name})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
