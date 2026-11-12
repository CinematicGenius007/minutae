#!/bin/sh
set -eu
swiftlint lint --strict
swift-format lint --recursive --strict App Packages
