#!/usr/bin/env bash
set -euo pipefail

toolchain="${FUZZ_TOOLCHAIN:-nightly-2026-11-26}"
seconds="${FUZZ_SECONDS:-30}"
cargo +"$toolchain" fuzz build --no-cfg-fuzzing
for target in jwt_raw jwt_claims credentials authorization; do
  mkdir -p "fuzz/corpus/$target"
  options=()
  if [ "$target" = jwt_claims ]; then
    options+=(-dict=fuzz/jwt.dict)
  fi
  cargo +"$toolchain" fuzz run "$target" \
    "fuzz/corpus/$target" "fuzz/seeds/$target" --no-cfg-fuzzing -- \
    -max_total_time="$seconds" -max_len=8192 -timeout=5 -rss_limit_mb=1024 "${options[@]}"
done
