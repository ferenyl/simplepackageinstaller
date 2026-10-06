#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

target=x86_64-unknown-linux-gnu
if [ -d /usr/lib/rustlib/x86_64-unknown-linux-musl ] || rustup target list --installed 2>/dev/null | grep -q musl; then
    target=x86_64-unknown-linux-musl
fi

cargo build --release --target "$target"
mkdir -p bin
cp "target/$target/release/simplepackageinstaller" bin/simplepackageinstaller
echo "bin/simplepackageinstaller ($target)"
