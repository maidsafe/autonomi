#!/usr/bin/env bash

# This script can be useful in rare cases where you need to run the stable or rc build again. It
# will clear out all the binary archives from S3. The version numbers used are from the crates on
# the branch on which the script is running. That fact should make it pretty difficult to delete
# anything unintentionally, but obviously, just use care with the script.

architectures=(
  "aarch64-apple-darwin"
  "aarch64-unknown-linux-musl"
  "arm-unknown-linux-musleabi"
  "armv7-unknown-linux-musleabihf"
  "x86_64-apple-darwin"
  "x86_64-pc-windows-msvc"
  "x86_64-unknown-linux-musl"
)
declare -A binary_crate_dir_mappings=(
  ["nat-detection"]="nat-detection"
  ["node-launchpad"]="node-launchpad"
  ["ant"]="ant-cli"
  ["antnode"]="ant-node"
  ["antctl"]="ant-node-manager"
)
declare -A binary_s3_bucket_mappings=(
  ["nat-detection"]="nat-detection"
  ["node-launchpad"]="node-launchpad"
  ["ant"]="autonomi-cli"
  ["antnode"]="antnode"
  ["antctl"]="antctl"
)

for arch in "${architectures[@]}"; do
  for binary in "${!binary_crate_dir_mappings[@]}"; do
    crate_dir="${binary_crate_dir_mappings[$binary]}"
    bucket_name="${binary_s3_bucket_mappings[$binary]}"
    version=$(grep "^version" < $crate_dir/Cargo.toml | head -n 1 | awk '{ print $3 }' | sed 's/\"//g')
    zip_filename="${binary}-${version}-${arch}.zip"
    tar_filename="${binary}-${version}-${arch}.tar.gz"

    dest="s3://${bucket_name}/${zip_filename}"
    if aws s3 ls "$dest" > /dev/null 2>&1; then
      aws s3 rm $dest
      echo "Removed $dest"
    else
      echo "$dest did not exist"
    fi

    dest="s3://${bucket_name}/${tar_filename}"
    if aws s3 ls "$dest" > /dev/null 2>&1; then
      aws s3 rm $dest
      echo "Removed $dest"
    else
      echo "$dest did not exist"
    fi
  done
done
