#!/usr/bin/env bash

# This script removes RC versions of the `node-launchpad` binary from S3.
# It is used to clean up after a release candidate test.
#
# Usage: ./remove-s3-node-launchpad-rc-binaries.sh <version>
# Example: ./remove-s3-node-launchpad-rc-binaries.sh 0.3.1-rc.1
#
# Safety: The script will only remove RC versions (containing '-rc').

set -e

if [[ -z "$1" ]]; then
  echo "Usage: $0 <version>"
  echo "Example: $0 0.3.1-rc.1"
  exit 1
fi

version="$1"

# Safety check: only allow RC versions
if [[ ! "$version" =~ -rc\. ]]; then
  echo "Error: This script only removes RC versions for safety."
  echo "The version must contain '-rc.' (e.g., 0.3.1-rc.1)"
  exit 1
fi

architectures=(
  "aarch64-apple-darwin"
  "aarch64-unknown-linux-musl"
  "arm-unknown-linux-musleabi"
  "armv7-unknown-linux-musleabihf"
  "x86_64-apple-darwin"
  "x86_64-pc-windows-msvc"
  "x86_64-unknown-linux-musl"
)

bucket_name="autonomi-cli"

echo "Removing node-launchpad binary version $version from S3..."

for arch in "${architectures[@]}"; do
  zip_filename="node-launchpad-${version}-${arch}.zip"
  tar_filename="node-launchpad-${version}-${arch}.tar.gz"

  dest="s3://${bucket_name}/${zip_filename}"
  if aws s3 ls "$dest" > /dev/null 2>&1; then
    aws s3 rm "$dest"
    echo "Removed $dest"
  else
    echo "$dest did not exist"
  fi

  dest="s3://${bucket_name}/${tar_filename}"
  if aws s3 ls "$dest" > /dev/null 2>&1; then
    aws s3 rm "$dest"
    echo "Removed $dest"
  else
    echo "$dest did not exist"
  fi
done

echo "Done."
