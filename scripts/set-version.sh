#!/bin/sh
# Writes a version into the workspace's Cargo.toml (`[workspace.package]`),
# which every crate takes its version from. The release workflow runs it with
# the tag's version before building, so releases need no version-bump commit.
#
#   sh scripts/set-version.sh 1.5.4
#
# Run it after restoring the Rust cache: the cache key includes Cargo.toml.
set -eu
version="$1"
case "$version" in
    *[!0-9.]* | "" | .* | *. | *..*) echo "not a version like 1.5.4: $version" >&2; exit 1 ;;
esac
root="$(cd "$(dirname "$0")/.." && pwd)"
# The first `version = ` line is [workspace.package]'s.
awk -v v="$version" '!done && /^version = "/ { print "version = \"" v "\""; done = 1; next } { print }' \
    "$root/Cargo.toml" > "$root/Cargo.toml.new"
mv "$root/Cargo.toml.new" "$root/Cargo.toml"
grep -m1 '^version = ' "$root/Cargo.toml"
