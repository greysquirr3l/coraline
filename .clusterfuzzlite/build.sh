#!/bin/bash -eu
#
# ClusterFuzzLite build script for Coraline.
#
# Builds each `cargo fuzz` target in `fuzz/fuzz_targets/` with the OSS-Fuzz
# nightly toolchain and copies the resulting binaries into `$OUT/` for the
# `run-fuzzers` action. Binary names mirror the target source names
# (`.rs` extension stripped, per ClusterFuzzLite's requirement that
# fuzzer binary names contain only `[A-Za-z0-9_-]`).
#
# Reference: https://google.github.io/clusterfuzzlite/build-integration/rust-lang/

set -o pipefail

# Force the nightly toolchain — libsqlite3-sys 0.38's build.rs uses
# `cfg_select!`, which is unstable in build scripts (rust-lang/rust#115585).
# The Dockerfile already sets RUSTUP_TOOLCHAIN=nightly, but exporting it
# here as well makes the script self-sufficient when invoked outside the
# image (e.g. locally).
export RUSTUP_TOOLCHAIN=nightly

cd "$SRC/coraline"

# Build all fuzz targets in release mode with debug assertions enabled so
# the fuzzer exercises more `debug_assert!`/`unwrap` paths in the library
# code. `-O` enables optimisation; `--debug-assertions` keeps runtime
# invariants checked.
cargo fuzz build --fuzz-dir fuzz -O --debug-assertions

# Locate the compiled fuzz binaries. `cargo fuzz build` can place them in
# different directories depending on whether the fuzz crate is a workspace
# member (the workspace target dir is used) or standalone (cargo-fuzz falls
# back to `fuzz/target/...`). Check the common layouts in priority order.
FUZZ_BIN_DIR=""
for candidate in \
    "fuzz/target/x86_64-unknown-linux-gnu/release" \
    "target/x86_64-unknown-linux-gnu/release" \
    "fuzz/target/release" \
    "target/release"; do
    if [[ -d "$candidate" && -n "$(ls -A "$candidate" 2>/dev/null)" ]]; then
        FUZZ_BIN_DIR="$candidate"
        break
    fi
done

if [[ -z "$FUZZ_BIN_DIR" ]]; then
    echo "::error::Could not locate compiled fuzz binaries in any standard target dir" >&2
    exit 1
fi

for src in fuzz/fuzz_targets/*.rs; do
    name="$(basename "${src%.rs}")"
    bin="$FUZZ_BIN_DIR/$name"
    if [[ -x "$bin" ]]; then
        cp -v "$bin" "$OUT/$name"
    else
        echo "::warning::Fuzz target '$name' produced no binary at $bin" >&2
    fi
done
