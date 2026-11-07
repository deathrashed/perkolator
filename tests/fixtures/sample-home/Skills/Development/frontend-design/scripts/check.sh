#!/bin/sh
# Fixture script: lists stylesheet files. Never executed by Perkolator scans.
set -eu
find "${1:-.}" -name '*.css' -print
