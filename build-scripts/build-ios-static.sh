#!/usr/bin/env bash
set -euo pipefail

# Build playit-agent as a static library for iOS.
# Outputs:
#   build/ios/device/libplayit_agent.a
#   build/ios/simulator/libplayit_agent.a
#   build/ios/simulator-universal/libplayit_agent.a

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${ROOT_DIR}/build/ios"

CRATE="playit-agent-ffi"
LIB_NAME="libplayit_agent.a"

IOS_TARGET="aarch64-apple-ios"
SIM_ARM64_TARGET="aarch64-apple-ios-sim"
SIM_X86_64_TARGET="x86_64-apple-ios"

echo "==> Ensuring Rust targets are installed"
rustup target add "${IOS_TARGET}" "${SIM_ARM64_TARGET}" "${SIM_X86_64_TARGET}"

echo "==> Building device (arm64)"
cargo build -p "${CRATE}" --release --target "${IOS_TARGET}" --manifest-path "${ROOT_DIR}/Cargo.toml"

echo "==> Building simulator (arm64)"
cargo build -p "${CRATE}" --release --target "${SIM_ARM64_TARGET}" --manifest-path "${ROOT_DIR}/Cargo.toml"

echo "==> Building simulator (x86_64)"
cargo build -p "${CRATE}" --release --target "${SIM_X86_64_TARGET}" --manifest-path "${ROOT_DIR}/Cargo.toml"

mkdir -p "${OUT_DIR}/device" "${OUT_DIR}/simulator" "${OUT_DIR}/simulator-universal"

cp "${ROOT_DIR}/target/${IOS_TARGET}/release/${LIB_NAME}" "${OUT_DIR}/device/${LIB_NAME}"
cp "${ROOT_DIR}/target/${SIM_ARM64_TARGET}/release/${LIB_NAME}" "${OUT_DIR}/simulator/${LIB_NAME}"

echo "==> Creating universal simulator lib (arm64 + x86_64)"
lipo -create \
  "${ROOT_DIR}/target/${SIM_ARM64_TARGET}/release/${LIB_NAME}" \
  "${ROOT_DIR}/target/${SIM_X86_64_TARGET}/release/${LIB_NAME}" \
  -output "${OUT_DIR}/simulator-universal/${LIB_NAME}"

echo "==> Done"
echo "Device: ${OUT_DIR}/device/${LIB_NAME}"
echo "Simulator (arm64): ${OUT_DIR}/simulator/${LIB_NAME}"
echo "Simulator (universal): ${OUT_DIR}/simulator-universal/${LIB_NAME}"
