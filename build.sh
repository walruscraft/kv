#!/bin/sh
# Build kv with minimal binary size
set -e

cargo build --release

# Strip unnecessary sections for minimal size
BINARY="target/x86_64-unknown-linux-gnu/release/kv"
if command -v objcopy >/dev/null 2>&1; then
    objcopy -R .eh_frame -R .comment "$BINARY"
fi

ls -la "$BINARY"
