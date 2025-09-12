#!/usr/bin/env bash

set -e

# This script must run from the root of the repository.

# This allows for, e.g., "alpha" to be passed when calling the script.
pre_release_identifer=${1:-"rc"}

all_crates=($(awk '/members = \[/{flag=1; next} /\]/{flag=0} flag {gsub(/[",]/, ""); print $0}' \
  Cargo.toml))

if ! cargo set-version --help > /dev/null 2>&1; then
  echo "cargo set-version not found"
  echo "Please install cargo-edit: cargo install cargo-edit --features vendored-openssl"
  exit 1
fi

declare -A crates_bumped
crates_bumped_with_version=()

release-plz update 2>&1 | tee bump_version_output

while IFS= read -r line; do
  # Sometimes this list can include crates that were not bumped. The presence of "->" indicates
  # whether a bump occurred.
  if [[ "$line" == *"->"* ]]; then
    name=$(echo "$line" | awk -F"\`" '{print $2}')
    version=$(echo "$line" | awk -F"-> " '{print $2}')
    crates_bumped["$name"]=1
    crates_bumped_with_version+=("${name}-v${version}")
  fi
done < <(cat bump_version_output | grep "^\*")

# The bumps performed by release-plz need to be reverted, because going to an `rc` pre-release
# specifier is considered a downgrade, so `set-version` won't do it. We will take the bumps that
# release-plz provided and use `set-version` to put the `rc` specifier on them.
git checkout -- .

for crate in "${crates_bumped_with_version[@]}"; do
  name=$(echo "$crate" | sed -E 's/-v.*$//')
  version=$(echo "$crate" | sed -E 's/^.*-v(.*)$/\1/')
  new_version="${version}-${pre_release_identifer}.1"
  echo "Setting $crate to $new_version"
  cargo set-version --package $name $new_version
done

echo "Now performing safety bumps for any crates not bumped by release-plz..."
for crate in "${all_crates[@]}"; do
  if [[ -z "${crates_bumped[$crate]}" ]]; then
    echo "==============================="
    echo " Safety bump for $crate"
    echo "==============================="
    echo "release-plz did not bump $crate"
    version=$(grep "^version" < $crate/Cargo.toml | head -n 1 | awk '{ print $3 }' | sed 's/\"//g')
    echo "Current version is $version"

    IFS='.' read -r major minor patch <<< "$version"
    patch=$((patch + 1))
    new_version="${major}.${minor}.${patch}-${pre_release_identifer}.1"

    echo "Safety bump to $new_version"
    cargo set-version --package $crate $new_version
  fi
done

echo "======================"
echo "  New Crate Versions  "
echo "======================"
for crate in "${all_crates[@]}"; do
  version=$(grep "^version" < $crate/Cargo.toml | head -n 1 | awk '{ print $3 }' | sed 's/\"//g')
  echo "$crate: $version"
done

echo "======================="
echo "  New Binary Versions  "
echo "======================="
echo "ant: $(grep "^version" < ant-cli/Cargo.toml | head -n 1 | awk '{ print $3 }' | sed 's/\"//g')"
echo "antctl: $(grep "^version" < ant-node-manager/Cargo.toml | head -n 1 | awk '{ print $3 }' | sed 's/\"//g')"
echo "antnode: $(grep "^version" < ant-node/Cargo.toml | head -n 1 | awk '{ print $3 }' | sed 's/\"//g')"
echo "nat-detection: $(grep "^version" < nat-detection/Cargo.toml | head -n 1 | awk '{ print $3 }' | sed 's/\"//g')"
echo "node-launchpad: $(grep "^version" < node-launchpad/Cargo.toml | head -n 1 | awk '{ print $3 }' | sed 's/\"//g')"
