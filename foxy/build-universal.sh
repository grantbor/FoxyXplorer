#!/bin/bash
set -e
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
mkdir -p target/universal-apple-darwin/release
lipo -create \
  target/aarch64-apple-darwin/release/foxyXplorer \
  target/x86_64-apple-darwin/release/foxyXplorer \
  -output target/universal-apple-darwin/release/foxyXplorer
