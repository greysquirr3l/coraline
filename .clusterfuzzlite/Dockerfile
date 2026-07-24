# ClusterFuzzLite build environment for Coraline.
#
# Uses the upstream `base-builder-rust` image, which ships the latest
# nightly Rust toolchain and the `cargo fuzz` subcommand pre-installed.
# We only need to copy the project source in; `build.sh` does the rest.
#
# Reference: https://google.github.io/clusterfuzzlite/build-integration/rust-lang/

FROM gcr.io/oss-fuzz-base/base-builder-rust

# Default to nightly for every cargo invocation in this image.
# ClusterFuzzLite's auto-build for Rust runs `cargo build
# --manifest-path fuzz/Cargo.toml` with the image's default toolchain
# (stable), which fails because libsqlite3-sys 0.38's build.rs uses
# `cfg_select!` — an unstable feature in build scripts (see
# rust-lang/rust#115585). Setting RUSTUP_TOOLCHAIN here covers both
# the auto-build path and our own build.sh.
ENV RUSTUP_TOOLCHAIN=nightly

# Copy the entire repository into the OSS-Fuzz `$SRC/coraline` workspace,
# matching the convention used by OSS-Fuzz Rust projects (the project name
# is taken from the root directory, which ClusterFuzzLite uses to set
# `$SRC/<name>` and `$OUT`).
COPY . $SRC/coraline

# ClusterFuzzLite's `compile` entrypoint invokes `bash $SRC/build.sh`,
# but we keep the script under `.clusterfuzzlite/build.sh` in the repo
# for organization. Symlink it into the expected location so the
# compile step can find it without forcing a build.sh at the repo root.
RUN ln -sf "$SRC/coraline/.clusterfuzzlite/build.sh" "$SRC/build.sh"
