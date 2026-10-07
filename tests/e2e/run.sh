#!/usr/bin/env bash
# End-to-end download test: fake HTTP tracker + fake peer swarm, then
# `ferrite download`, verifying the output byte-for-byte against the source.
set -euo pipefail
cd "$(dirname "$0")/../.."
cargo build --release
python3 tests/e2e/harness.py "$(mktemp -d)" ./target/release/ferrite
