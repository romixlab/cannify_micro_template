# /// script
# requires-python = ">=3.11"
# ///
"""Give the template project your own name: `just rename my_board [short]` or `uv run scripts/rename.py my_board`.

Renames the crates and their directories (`cannify_micro_api` -> `my_board_api`, ...), the API trait
(`CannifyMicroApi` -> `MyBoardApi`), the generated GID constant, the Python module, the command line binary (`cannify`
-> SHORT, the new name itself when SHORT is left out) and its environment variables (`CANNIFY_PROBE` -> `MY_BOARD_PROBE`).
The API snapshots in `cannify_micro_api/api_snapshots/` describe the template's API crate, not the renamed one: they
are removed, and the renamed crate's first snapshot is saved with `ww api save` (or left to `just api-save`, which
the renamed crate's test now requires before it passes).
Prose that names the board ("CANnify Micro", the chip, the LED pin) stays: it is listed at the end for a look.
Run it once, in a clean checkout, before changing anything else.
"""

import re
import shutil
import subprocess
import sys
from pathlib import Path

OLD = "cannify_micro"
OLD_API = "cannify_micro_api"
OLD_SHORT = "cannify"
SKIP_DIRS = {".git", "target", ".venv", "__pycache__"}
TEXT_SUFFIXES = {".rs", ".toml", ".lock", ".md", ".py", ".json", ".yml", ".yaml", ".x", ""}


def pascal(snake: str) -> str:
    return "".join(part.capitalize() for part in snake.split("_"))


def upper_snake(snake: str) -> str:
    """The form WireWeaver uses for a crate's GID constant: `nucleo_g0b1re_api` -> `NUCLEO_G_0_B_1_RE_API`."""
    with_digit_bounds = re.sub(r"(?<=[a-zA-Z])(?=\d)|(?<=\d)(?=[a-zA-Z])", "_", snake)
    return with_digit_bounds.upper()


def replacements(new: str, short: str) -> list[tuple[str, str]]:
    # longest and most specific first, so a shorter pattern never eats a longer one
    return [
        (upper_snake(OLD_API), upper_snake(OLD_API.replace(OLD, new))),
        (pascal(OLD_API), pascal(OLD_API.replace(OLD, new))),
        (OLD, new),
        (OLD_SHORT.upper() + "_", short.upper() + "_"),
        (OLD_SHORT, short),
    ]


def main() -> int:
    if len(sys.argv) not in (2, 3):
        print(__doc__)
        return 2
    new = sys.argv[1]
    short = sys.argv[2] if len(sys.argv) == 3 else new
    for name in (new, short):
        if not re.fullmatch(r"[a-z][a-z0-9_]*", name):
            print(f"not a crate name (lower case, digits, underscores): {name!r}")
            return 2
    root = Path(__file__).resolve().parent.parent
    pairs = replacements(new, short)
    for snapshot in sorted((root / OLD_API / "api_snapshots").glob(f"{OLD_API}_*.ron")):
        snapshot.unlink()
    changed_files = 0
    for path in sorted(root.rglob("*")):
        if any(part in SKIP_DIRS for part in path.relative_to(root).parts):
            continue
        if not path.is_file() or path.suffix not in TEXT_SUFFIXES or path == Path(__file__).resolve():
            continue
        try:
            text = path.read_text()
        except UnicodeDecodeError:
            continue
        new_text = text
        for old, replacement in pairs:
            new_text = new_text.replace(old, replacement)
        if new_text != text:
            path.write_text(new_text)
            changed_files += 1
    renamed = 0
    for path in sorted(root.rglob(f"*{OLD}*"), key=lambda p: len(p.parts), reverse=True):
        if any(part in SKIP_DIRS for part in path.relative_to(root).parts):
            continue
        path.rename(path.with_name(path.name.replace(OLD, new)))
        renamed += 1
    api_crate = OLD_API.replace(OLD, new)
    if shutil.which("ww"):
        subprocess.run(["ww", "api", "save", api_crate], cwd=root, check=False)
    else:
        print("ww is not installed (just setup installs it): record the API with `just api-save` before committing")
    print(f"{changed_files} files edited, {renamed} paths renamed: {OLD} -> {new}, {OLD_SHORT} -> {short}")
    print("Left as they were, have a look: README.md and firmware/README.md prose, the chip (STM32G0B1CE / STM32G0B1CETx),")
    print("the LED pin (PB14) in firmware/src/main.rs, `description` lines in Cargo.toml files.")
    print("Then: cargo generate-lockfile is not needed (path names changed only), `just lint` checks everything.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
