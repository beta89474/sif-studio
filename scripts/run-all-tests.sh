#!/usr/bin/env bash
# run-all-tests.sh —— M2.5 收尾全量回归
set -u
cd "C:/Users/Beta/WorkBuddy/2026-09-12-09-46-53/sif-studio/src-tauri" || exit 1
export PATH="/c/cargo/bin:/c/Users/Beta/.cargo/bin:$PATH"
export CARGO_BUILD_JOBS=1
export RUSTFLAGS="-C codegen-units=1"

echo "===== 1/4 cargo test --lib (unit) ====="
cargo test --lib 2>&1 | tail -4

echo "===== 2/4 integration_test ====="
cargo test --test integration_test -- --test-threads=1 2>&1 | tail -4

echo "===== 3/4 m24_history_test ====="
cargo test --test m24_history_test -- --test-threads=1 2>&1 | tail -4

echo "===== 4/4 m25_export_test ====="
cargo test --test m25_export_test -- --test-threads=1 2>&1 | tail -4

echo "===== ALL DONE ====="
