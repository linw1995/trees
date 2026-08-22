#!/usr/bin/env bash

set -euxo pipefail

export CARGO_INCREMENTAL=0
export RUSTFLAGS="-Cinstrument-coverage -Ccodegen-units=1 -Copt-level=0 -Clink-dead-code"
workspace_root="$(pwd -P)"
export CARGO_TARGET_DIR="${workspace_root}/target/coverage"
export LLVM_PROFILE_FILE="${CARGO_TARGET_DIR}/data/trees-%p-%m.profraw"

rm -rf "${CARGO_TARGET_DIR}"
mkdir -p "${CARGO_TARGET_DIR}/data/" "${CARGO_TARGET_DIR}/result/"

cargo nextest run --workspace "$@"

grcov "${CARGO_TARGET_DIR}/data" \
  --llvm \
  --branch \
  --source-dir "${workspace_root}" \
  --ignore-not-existing \
  --ignore '../*' \
  --ignore '/*' \
  --binary-path "${CARGO_TARGET_DIR}/debug/deps" \
  --output-types html,cobertura,lcov,markdown \
  --output-path "${CARGO_TARGET_DIR}/result/"

test -s "${CARGO_TARGET_DIR}/result/lcov"
test -s "${CARGO_TARGET_DIR}/result/cobertura.xml"
cp "${CARGO_TARGET_DIR}/result/lcov" "${CARGO_TARGET_DIR}/result/lcov.info"
