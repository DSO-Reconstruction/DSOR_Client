#!/usr/bin/env bash
# Build the browser client twice -- WebGPU and WebGL2 -- under client/dist, with
# the loader page that picks one (client/web/index.html).
#
#   tools/build_web.sh            both builds
#   tools/build_web.sh webgl2     only one of them (webgl2 | webgpu)
#
# bevy specialises its renderer for one graphics API at compile time, so each
# build has its own feature (client/Cargo.toml) and its own target directory
# (switching features in one directory would rebuild bevy every time).
#   tools/build_web.sh --fast     WebGPU only, quick to rebuild (profile wasm-fast:
#                                 no LTO, no wasm-opt, incremental) -- for iterating
set -euo pipefail
cd "$(dirname "$0")/../client"
export PATH="$HOME/.cargo/bin:$PATH"
if [ "${1:-}" = "--fast" ]; then
    echo "[web] webgpu (fast)"
    # index.html names its profile (wasm-release, wasm-opt 3), and that wins over
    # --cargo-profile: the fast build reads a copy naming wasm-fast, no wasm-opt.
    sed -e 's/data-cargo-profile="wasm-release"/data-cargo-profile="wasm-fast"/' \
        -e 's/data-wasm-opt="3"/data-wasm-opt="0"/' index.html > index.fast.html
    CARGO_TARGET_DIR="../target/web-fast" trunk build index.fast.html \
        --no-default-features --features webgpu \
        --dist "dist/webgpu" --filehash false --public-url "./"
    [ -f dist/webgpu/index.fast.html ] && mv dist/webgpu/index.fast.html dist/webgpu/index.html
    cp web/index.html dist/index.html
    ln -sfn ../../assets dist/assets
    echo "[web] done (fast, WebGPU only): client/dist"
    exit 0
fi
kinds=("${@:-webgl2 webgpu}")
for kind in ${kinds[@]}; do
    echo "[web] $kind"
    CARGO_TARGET_DIR="../target/web-$kind" trunk build --release \
        --no-default-features --features "$kind" \
        --dist "dist/$kind" --filehash false --public-url "./"
done
cp web/index.html dist/index.html
# The converted game data is never copied: the page reads it through a symlink.
ln -sfn ../../assets dist/assets
echo "[web] done: client/dist (open index.html?map=a0200_kingscity)"
