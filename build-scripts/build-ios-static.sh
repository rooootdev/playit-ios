#!/usr/bin/env bash
set -euo pipefail

# Build playit-agent as static + dynamic libraries for iOS.
# Outputs:
#   build/ios/device/libplayit_agent.a
#   build/ios/device/libplayit_agent.dylib
#   build/ios/simulator/libplayit_agent.a
#   build/ios/simulator/libplayit_agent.dylib
#   build/ios/simulator-universal/libplayit_agent.a
#   build/ios/simulator-universal/libplayit_agent.dylib

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${ROOT_DIR}/build/ios"

CRATE="playit-agent-ffi"
LIB_NAME="libplayit_agent.a"
DYLIB_NAME="libplayit_agent.dylib"

IOS_TARGET="aarch64-apple-ios"
SIM_ARM64_TARGET="aarch64-apple-ios-sim"
SIM_X86_64_TARGET="x86_64-apple-ios"

TOOLCHAIN="$(rustup show active-toolchain | awk '{print $1}')"
TOOLCHAIN_ALIAS="${TOOLCHAIN}"
if [[ "${TOOLCHAIN}" == stable-* ]]; then
  TOOLCHAIN_ALIAS="stable"
elif [[ "${TOOLCHAIN}" == nightly-* ]]; then
  TOOLCHAIN_ALIAS="nightly"
elif [[ "${TOOLCHAIN}" == beta-* ]]; then
  TOOLCHAIN_ALIAS="beta"
fi

CARGO_CMD=(cargo)
if [[ -n "${TOOLCHAIN_ALIAS}" ]]; then
  CARGO_BIN="$(rustup which cargo --toolchain "${TOOLCHAIN_ALIAS}")"
  RUSTC_BIN="$(rustup which rustc --toolchain "${TOOLCHAIN_ALIAS}")"
  CARGO_CMD=("${CARGO_BIN}")
  export RUSTC="${RUSTC_BIN}"
fi

echo "==> Ensuring Rust targets are installed for ${TOOLCHAIN_ALIAS:-default}"
if [[ -n "${TOOLCHAIN_ALIAS}" ]]; then
  rustup target add --toolchain "${TOOLCHAIN_ALIAS}" "${IOS_TARGET}" "${SIM_ARM64_TARGET}" "${SIM_X86_64_TARGET}"
else
  rustup target add "${IOS_TARGET}" "${SIM_ARM64_TARGET}" "${SIM_X86_64_TARGET}"
fi

echo "==> Verifying iOS target std is available"
if ! rustup run "${TOOLCHAIN_ALIAS}" rustc --print target-libdir --target "${IOS_TARGET}" >/dev/null 2>&1; then
  echo "iOS target std not found for toolchain ${TOOLCHAIN_ALIAS}. Try:"
  echo "  rustup target add ${IOS_TARGET} --toolchain ${TOOLCHAIN_ALIAS}"
  exit 1
fi

echo "==> Building device (arm64)"
"${CARGO_CMD[@]}" build -p "${CRATE}" --release --target "${IOS_TARGET}" --manifest-path "${ROOT_DIR}/Cargo.toml"

echo "==> Building simulator (arm64)"
"${CARGO_CMD[@]}" build -p "${CRATE}" --release --target "${SIM_ARM64_TARGET}" --manifest-path "${ROOT_DIR}/Cargo.toml"

echo "==> Building simulator (x86_64)"
"${CARGO_CMD[@]}" build -p "${CRATE}" --release --target "${SIM_X86_64_TARGET}" --manifest-path "${ROOT_DIR}/Cargo.toml"

mkdir -p "${OUT_DIR}/device" "${OUT_DIR}/simulator" "${OUT_DIR}/simulator-universal"

cp "${ROOT_DIR}/target/${IOS_TARGET}/release/${LIB_NAME}" "${OUT_DIR}/device/${LIB_NAME}"
cp "${ROOT_DIR}/target/${IOS_TARGET}/release/${DYLIB_NAME}" "${OUT_DIR}/device/${DYLIB_NAME}"
cp "${ROOT_DIR}/target/${SIM_ARM64_TARGET}/release/${LIB_NAME}" "${OUT_DIR}/simulator/${LIB_NAME}"
cp "${ROOT_DIR}/target/${SIM_ARM64_TARGET}/release/${DYLIB_NAME}" "${OUT_DIR}/simulator/${DYLIB_NAME}"

echo "==> Creating universal simulator lib (arm64 + x86_64)"
lipo -create \
  "${ROOT_DIR}/target/${SIM_ARM64_TARGET}/release/${LIB_NAME}" \
  "${ROOT_DIR}/target/${SIM_X86_64_TARGET}/release/${LIB_NAME}" \
  -output "${OUT_DIR}/simulator-universal/${LIB_NAME}"

lipo -create \
  "${ROOT_DIR}/target/${SIM_ARM64_TARGET}/release/${DYLIB_NAME}" \
  "${ROOT_DIR}/target/${SIM_X86_64_TARGET}/release/${DYLIB_NAME}" \
  -output "${OUT_DIR}/simulator-universal/${DYLIB_NAME}"

echo "==> Done"
echo "Device: ${OUT_DIR}/device/${LIB_NAME}"
echo "Device: ${OUT_DIR}/device/${DYLIB_NAME}"
echo "Simulator (arm64): ${OUT_DIR}/simulator/${LIB_NAME}"
echo "Simulator (arm64): ${OUT_DIR}/simulator/${DYLIB_NAME}"
echo "Simulator (universal): ${OUT_DIR}/simulator-universal/${LIB_NAME}"
echo "Simulator (universal): ${OUT_DIR}/simulator-universal/${DYLIB_NAME}"
