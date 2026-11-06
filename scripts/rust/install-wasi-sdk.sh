#!/usr/bin/env bash
# Copyright (c) 2026 Trent Polack. All Rights Reserved.
# Licensed under the MIT License.

# Install the reproducible Linux CI toolchain. Other platforms use the corresponding official WASI SDK 34.0 archive.
set -euo pipefail

sdk_cache_root="${1:?Usage: install-wasi-sdk.sh <cache-directory>}"
sdk_version='34.0'
sdk_archive_name="wasi-sdk-${sdk_version}-x86_64-linux.tar.gz"
# Official release asset digest: https://github.com/WebAssembly/wasi-sdk/releases/tag/wasi-sdk-34
sdk_archive_sha256='b761e3a0721dbae9c09a0059e5fdb2bf917d1b4a8a7b430fb3b5aafb0984b2c4'
sdk_archive_url="https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-34/${sdk_archive_name}"

if [[ "$(uname -s)" != 'Linux' || "$(uname -m)" != 'x86_64' ]]; then
  echo 'This CI installer supports x86_64 Linux. Install the matching official WASI SDK 34.0 release for your host.' >&2
  exit 1
fi

mkdir -p "$sdk_cache_root"
sdk_cache_root="$(cd "$sdk_cache_root" && pwd)"
sdk_directory="$sdk_cache_root/wasi-sdk-${sdk_version}-x86_64-linux"
sdk_archive="$sdk_cache_root/$sdk_archive_name"
sdk_partial="$sdk_archive.partial"

if [[ -x "$sdk_directory/bin/clang" && -f "$sdk_directory/share/wasi-sysroot/lib/wasm32-wasip1/libsetjmp.a" ]] &&
   [[ "$(head -n 1 "$sdk_directory/VERSION")" == "$sdk_version" ]]; then
  printf '%s\n' "$sdk_directory"
  exit 0
fi

trap 'rm -f "$sdk_partial"' EXIT
curl --fail --location --retry 3 --output "$sdk_partial" "$sdk_archive_url"
printf '%s  %s\n' "$sdk_archive_sha256" "$sdk_partial" | sha256sum --check --status
mv "$sdk_partial" "$sdk_archive"
tar -xzf "$sdk_archive" -C "$sdk_cache_root"
rm "$sdk_archive"

if [[ "$(head -n 1 "$sdk_directory/VERSION")" != "$sdk_version" ]]; then
  echo 'The verified WASI SDK archive did not install the expected version.' >&2
  exit 1
fi
printf '%s\n' "$sdk_directory"
