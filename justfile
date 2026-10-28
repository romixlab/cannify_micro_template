# CANnify Micro template: the firmware (firmware/, its .cargo/config.toml sets the target), the host workspace at
# the root (API crate, client library, `cannify` command line, Python module).

# Windows runs the recipes with cmd (`cd x && y` works there too): not every PC has Git's sh on PATH
set windows-shell := ["cmd.exe", "/c"]
# CANNIFY_PROBE (below) can live in .env, kept out of git
set dotenv-load

chip := "STM32G0B1CETx"
fw_dir := "firmware/target/thumbv6m-none-eabi"
# the same from inside firmware/
fw_elf_dir := "target/thumbv6m-none-eabi"
# The probe every recipe uses: the ST-LINK on J2 (0483:374b), never another probe on the PC; with two
# CANnify boards attached, CANNIFY_PROBE=SERIAL (`probe-rs list`) picks one, `cannify` takes the same variable
probe := "0483:374b" + if env("CANNIFY_PROBE", "") == "" { "" } else { ":" + env("CANNIFY_PROBE") }
runner := "target.thumbv6m-none-eabi.runner='probe-rs run --chip STM32G0B1CE --probe " + probe + "'"

default:
    @just --list

# Set up this PC; prints the steps that need root (`--root` runs them too, `--check` only reports what is missing)
setup *args:
    uv run scripts/setup.py {{args}}

# Turn on the project's git hooks in .githooks/ (the API check before a commit), after the hooks already there;
# `--check` reports, `--remove` puts the old ones back (`just setup` runs it too)
hooks *args:
    uv run scripts/hooks.py {{args}}

# Host tests (the workspace; the Python module's tests: `just test-py`)
test *flags:
    cargo test --workspace --locked {{flags}}

# Compare the API crate with its snapshots in cannify_micro_api/api_snapshots/: fails without a snapshot of the current
# version or on a change without the version bump it needs, and says which version to put into cannify_micro_api/Cargo.toml
# (also part of `just test`)
api-check:
    cargo test -p cannify_micro_api --test evolution

# Save (or refresh) the current version's snapshot; refuses a version not bumped enough. Commit api_snapshots/ with it
api-save:
    cargo test -p cannify_micro_api --test evolution --config "env.WW_API_SAVE.value='1'" -- --nocapture

# Format check and clippy for the host workspace and the firmware; the firmware's release build must fit
lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cd firmware && cargo fmt --all --check
    cd firmware && cargo clippy --bins --locked -- -D warnings
    cd firmware && cargo build --release --locked

# Format the host workspace and the firmware
fmt:
    cargo fmt --all
    cd firmware && cargo fmt --all

# Build, flash and run the dev firmware with its defmt log (probe-rs); Ctrl-C stops the log, the firmware keeps running.
# The ELF goes into the ELF directory by its build id (cnt elf add), so `cannify counters|log` decode from anywhere
run *args:
    cd firmware && cargo build {{args}} && cnt elf add {{fw_elf_dir}}/debug/cannify_micro
    cd firmware && cargo run --config "{{runner}}" {{args}}

# Build, flash and run the dev firmware showing the defmt log plus live counters as they change (cnt run; cargo install cnt_cli)
cnt *args:
    cd firmware && cargo build && cnt run --probe {{probe}} {{args}}

# Drive, pull and read every free pin live, laid out on the headers (ww pins tui); quitting releases them
pins *args:
    ww --api cannify_micro_api pins tui --board docs/board/b129a_cannify.toml {{args}}

# Build the release firmware
build-fw:
    cd firmware && cargo build --release --locked

# Flash the release firmware through the probe and reset the board; its ELF goes into the ELF directory (cnt elf add)
flash: build-fw
    cnt elf add {{fw_dir}}/release/cannify_micro
    probe-rs download --chip {{chip}} --probe {{probe}} {{fw_dir}}/release/cannify_micro
    probe-rs reset --chip {{chip}} --probe {{probe}}

# Attach to the running dev firmware's defmt log without flashing (stop it before using `cannify`: one probe, one user)
attach:
    probe-rs attach --chip {{chip}} --probe {{probe}} {{fw_dir}}/debug/cannify_micro

# Build the `cannify` command line and install it on this PC (with tpm on PATH through tpm install, else cargo install)
[unix]
install:
    cargo build --profile deploy --locked -p cannify_micro_cli
    if command -v tpm >/dev/null; then tpm install target/deploy/cannify; else cargo install --locked --path cannify_micro_cli; fi

# Build the `cannify` command line and install it on this PC (cargo install)
[windows]
install:
    cargo install --locked --path cannify_micro_cli

# Bring this PC and the board on the probe up to main: pull --ff-only (refuses on local changes), install, flash, check `cannify --version`
deploy:
    #!/usr/bin/env bash
    set -eu
    dirty=$(git status --porcelain)
    if [ -n "$dirty" ]; then echo "local changes, not deploying:"; echo "$dirty"; exit 1; fi
    git pull --ff-only
    just install
    just flash
    cannify --version | grep -q "($(git rev-parse --short HEAD)" || { echo "cannify --version does not show $(git rev-parse --short HEAD):"; cannify --version; exit 1; }
    cannify --version

# Build the Python module into cannify_micro_py/.venv (when Rust sources changed) and open a prompt with the board as `dev`
py *args:
    cd cannify_micro_py && uv sync && uv run python -i -c "import cannify_micro; dev = cannify_micro.Device({{args}}); print(dev, dev.firmware())"

# Run a Python example of cannify_micro_py/examples: `blink` (the default) blinks the LED and prints what runs on the
# board, `device` reads the device contract (board, ids, chip, counters, log, settings, boot, update node)
py-example name="blink" *args:
    cd cannify_micro_py && uv sync && uv run python examples/{{name}}.py {{args}}

# Build the Python module and run its tests (no board needed)
test-py:
    cd cannify_micro_py && uv run --group dev pytest

# Give the project your own name: `just rename my_board` (crates, directories, the command line name, env vars)
rename name short="":
    uv run scripts/rename.py {{name}} {{short}}

# Re-draw the board pictures in docs/board/ (top and bottom views, power tree, pin table) from the board description
# with boardmap: `cargo install --git https://github.com/romixlab/embedded_bedrock bedrock_board_cli`
board:
    boardmap render docs/board/b129a_cannify.toml

# Re-draw the logo in docs/logo/ from logo.typ: logo.svg and logo-dark.svg for the README, and social-preview.png
# (1280 x 640) for GitHub's social preview; needs typst
logo:
    typst compile --ignore-system-fonts --font-path docs/logo/fonts docs/logo/logo.typ docs/logo/logo.svg
    typst compile --ignore-system-fonts --font-path docs/logo/fonts --input theme=dark docs/logo/logo.typ docs/logo/logo-dark.svg
    typst compile --ignore-system-fonts --font-path docs/logo/fonts --ppi 72 docs/logo/social.typ docs/logo/social-preview.png
