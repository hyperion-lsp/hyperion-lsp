#!/usr/bin/env sh
# Retain the existing entry point for local and archived CI workflows.
set -eu
exec python3 "$(dirname "$0")/check_regression.py" "$@"
