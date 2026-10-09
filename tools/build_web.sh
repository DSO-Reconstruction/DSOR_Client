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
set -euo pipefail
cd "$(dirname "$0")/../client"
export PATH="$HOME/.cargo/bin:$PATH"
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
