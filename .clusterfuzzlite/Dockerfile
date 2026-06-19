# ClusterFuzzLite build environment for Coraline.
#
# Uses the upstream `base-builder-rust` image, which ships the latest
# nightly Rust toolchain and the `cargo fuzz` subcommand pre-installed.
# We only need to copy the project source in; `build.sh` does the rest.
#
# Reference: https://google.github.io/clusterfuzzlite/build-integration/rust-lang/

FROM gcr.io/oss-fuzz-base/base-builder-rust

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
