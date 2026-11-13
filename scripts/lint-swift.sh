#!/bin/sh
set -eu
for tool in swiftlint swift-format; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "Error: $tool is required to lint Swift; install it and ensure it is on PATH." >&2
        exit 1
    fi
done
swiftlint lint --strict
swift-format lint --recursive --strict App Packages
