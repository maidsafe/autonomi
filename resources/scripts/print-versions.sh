#!/usr/bin/env bash

set -e

all_crates=($(awk '/members = \[/{flag=1; next} /\]/{flag=0} flag {gsub(/[",]/, ""); print $0}' \
  Cargo.toml))

echo "=================="
echo "  Crate Versions  "
echo "=================="
for crate in "${all_crates[@]}"; do
  version=$(grep "^version" < $crate/Cargo.toml | head -n 1 | awk '{ print $3 }' | sed 's/\"//g')
  echo "$crate: $version"
done

echo "==================="
echo "  Binary Versions  "
echo "==================="
echo "ant: $(grep "^version" < ant-cli/Cargo.toml | head -n 1 | awk '{ print $3 }' | sed 's/\"//g')"
echo "antctl: $(grep "^version" < ant-node-manager/Cargo.toml | head -n 1 | awk '{ print $3 }' | sed 's/\"//g')"
echo "antnode: $(grep "^version" < ant-node/Cargo.toml | head -n 1 | awk '{ print $3 }' | sed 's/\"//g')"
echo "antnode_rpc_client: $(grep "^version" < ant-node-rpc-client/Cargo.toml | head -n 1 | awk '{ print $3 }' | sed 's/\"//g')"
echo "nat-detection: $(grep "^version" < nat-detection/Cargo.toml | head -n 1 | awk '{ print $3 }' | sed 's/\"//g')"
echo "node-launchpad: $(grep "^version" < node-launchpad/Cargo.toml | head -n 1 | awk '{ print $3 }' | sed 's/\"//g')"
