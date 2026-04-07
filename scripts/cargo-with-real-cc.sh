#!/usr/bin/env bash
# pnpm/npm 脚本在非交互 shell 中也可能继承把 `cc` 指到非编译器的环境；
# cc-rs 会调用 `cc`，此处在未显式设置 CC 时强制使用系统 Clang/GCC。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}/src-tauri"

# 显式 SDK：cc-rs 使用 --target=*-apple-macosx 时必须能解析系统头文件。
# 环境中可能残留指向旧版 Xcode 的 SDKROOT（例如 MacOSX12.3.sdk），会导致 stdlib.h 找不到；
# 此处始终用当前 xcrun 的 macOS SDK，除非显式设置 CC_SWITCH_KEEP_SDKROOT=1。
if [[ "$(uname -s)" == "Darwin" && -z "${CC_SWITCH_KEEP_SDKROOT:-}" ]]; then
  _SDK="$(xcrun --sdk macosx --show-sdk-path 2>/dev/null || true)"
  if [[ -n "${_SDK}" ]]; then
    export SDKROOT="${_SDK}"
    # cc-rs 子进程对 SDKROOT 的传递偶有不一致，CFLAGS 更稳妥
    if [[ "${CFLAGS:-}" != *"-isysroot"* ]]; then
      export CFLAGS="${CFLAGS:+$CFLAGS }-isysroot ${_SDK}"
    fi
    if [[ "${CXXFLAGS:-}" != *"-isysroot"* ]]; then
      export CXXFLAGS="${CXXFLAGS:+$CXXFLAGS }-isysroot ${_SDK}"
    fi
  fi
fi

if [[ -z "${CC:-}" ]]; then
  case "$(uname -s)" in
    Darwin)
      if [[ -x /usr/bin/clang ]]; then
        export CC=/usr/bin/clang
      elif command -v clang >/dev/null 2>&1; then
        export CC="$(command -v clang)"
      fi
      ;;
    *)
      if command -v clang >/dev/null 2>&1; then
        export CC="$(command -v clang)"
      elif command -v gcc >/dev/null 2>&1; then
        export CC="$(command -v gcc)"
      fi
      ;;
  esac
fi

if [[ -z "${CXX:-}" ]]; then
  case "$(uname -s)" in
    Darwin)
      if [[ -x /usr/bin/clang++ ]]; then
        export CXX=/usr/bin/clang++
      elif command -v clang++ >/dev/null 2>&1; then
        export CXX="$(command -v clang++)"
      fi
      ;;
    *)
      if command -v clang++ >/dev/null 2>&1; then
        export CXX="$(command -v clang++)"
      elif command -v g++ >/dev/null 2>&1; then
        export CXX="$(command -v g++)"
      fi
      ;;
  esac
fi

exec cargo "$@"
