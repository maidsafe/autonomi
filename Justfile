#!/usr/bin/env just --justfile

build-release-artifacts arch nightly="false":
  #!/usr/bin/env bash
  set -e

  arch="{{arch}}"
  nightly="{{nightly}}"
  supported_archs=(
    "x86_64-pc-windows-msvc"
    "x86_64-apple-darwin"
    "aarch64-apple-darwin"
    "x86_64-unknown-linux-musl"
    "arm-unknown-linux-musleabi"
    "armv7-unknown-linux-musleabihf"
    "aarch64-unknown-linux-musl"
  )

  arch_supported=false
  for supported_arch in "${supported_archs[@]}"; do
    if [[ "$arch" == "$supported_arch" ]]; then
      arch_supported=true
      break
    fi
  done

  if [[ "$arch_supported" == "false" ]]; then
    echo "$arch is not supported."
    exit 1
  fi

  if [[ "$arch" == "x86_64-unknown-linux-musl" ]]; then
    if [[ "$(grep -E '^NAME="Ubuntu"' /etc/os-release)" ]]; then
      # This is intended for use on a fresh Github Actions agent
      sudo apt update -y
      sudo apt-get install -y musl-tools
    fi
  fi

  rustup target add {{arch}}

  rm -rf artifacts
  mkdir artifacts
  cargo clean

  if [ -n "$MAX_CHUNK_SIZE" ]; then
    echo "Overriding chunk size to $MAX_CHUNK_SIZE bytes"
  fi

  echo "================"
  echo "  Network Keys  "
  echo "================"
  echo "FOUNDATION_PK: $FOUNDATION_PK"
  echo "GENESIS_PK: $GENESIS_PK"
  echo "NETWORK_ROYALTIES_PK: $NETWORK_ROYALTIES_PK"
  echo "PAYMENT_FORWARD_PK: $PAYMENT_FORWARD_PK"

  cross_container_opts="--env \"GENESIS_PK=$GENESIS_PK\" --env \"GENESIS_SK=$GENESIS_SK\" --env \"FOUNDATION_PK=$FOUNDATION_PK\" --env \"NETWORK_ROYALTIES_PK=$NETWORK_ROYALTIES_PK\" --env \"PAYMENT_FORWARD_PK=$PAYMENT_FORWARD_PK\""
  export CROSS_CONTAINER_OPTS=$cross_container_opts

  nightly_feature=""
  if [[ "$nightly" == "true" ]]; then
    nightly_feature="--features nightly"
  fi

  if [[ $arch == arm* || $arch == armv7* || $arch == aarch64* ]]; then
    echo "Passing to cross CROSS_CONTAINER_OPTS=$CROSS_CONTAINER_OPTS"
    cargo binstall --no-confirm cross
    cross build --release --target $arch --bin node-launchpad $nightly_feature
    cross build --release --target $arch --bin ant $nightly_feature --features loud
    cross build --release --target $arch --bin antnode $nightly_feature
    cross build --release --target $arch --bin antctl $nightly_feature
    cross build --release --target $arch --bin antnode_rpc_client $nightly_feature
  else
    cargo build --release --target $arch --bin node-launchpad $nightly_feature
    cargo build --release --target $arch --bin ant $nightly_feature --features loud
    cargo build --release --target $arch --bin antnode $nightly_feature
    cargo build --release --target $arch --bin antctl $nightly_feature
    cargo build --release --target $arch --bin antnode_rpc_client $nightly_feature
  fi

  find target/$arch/release -maxdepth 1 -type f -exec cp '{}' artifacts \;
  rm -f artifacts/.cargo-lock

# Debugging target that builds an `artifacts` directory to be used with packaging targets.
#
# To use, download the artifact zip files from the workflow run and put them in an `artifacts`
# directory here. Then run the target.
make-artifacts-directory:
  #!/usr/bin/env bash
  set -e

  architectures=(
    "x86_64-pc-windows-msvc"
    "x86_64-apple-darwin"
    "aarch64-apple-darwin"
    "x86_64-unknown-linux-musl"
    "arm-unknown-linux-musleabi"
    "armv7-unknown-linux-musleabihf"
    "aarch64-unknown-linux-musl"
  )
  cd artifacts
  for arch in "${architectures[@]}" ; do
    mkdir -p $arch/release
    unzip autonomi-$arch.zip -d $arch/release
    rm autonomi-$arch.zip
  done

package-all-bins:
  #!/usr/bin/env bash
  set -e
  just package-bin "node-launchpad"
  just package-bin "ant"
  just package-bin "antnode"
  just package-bin "antctl"
  just package-bin "antnode_rpc_client"

package-bin bin version="":
  #!/usr/bin/env bash
  set -e

  architectures=(
    "x86_64-pc-windows-msvc"
    "x86_64-apple-darwin"
    "aarch64-apple-darwin"
    "x86_64-unknown-linux-musl"
    "arm-unknown-linux-musleabi"
    "armv7-unknown-linux-musleabihf"
    "aarch64-unknown-linux-musl"
  )

  bin="{{bin}}"

  supported_bins=(\
    "node-launchpad" \
    "ant" \
    "antnode" \
    "antctl" \
    "antnode_rpc_client")
  crate_dir_name=""

  bin="{{bin}}"
  case "$bin" in
    node-launchpad)
      crate_dir_name="node-launchpad"
      ;;
    ant)
      crate_dir_name="ant-cli"
      ;;
    antnode)
      crate_dir_name="ant-node"
      ;;
    antctl)
      crate_dir_name="ant-node-manager"
      ;;
    antnode_rpc_client)
      crate_dir_name="ant-node-rpc-client"
      ;;
    *)
      echo "The $bin binary is not supported"
      exit 1
      ;;
  esac

  if [[ -z "{{version}}" ]]; then
    version=$(grep "^version" < $crate_dir_name/Cargo.toml | \
        head -n 1 | awk '{ print $3 }' | sed 's/\"//g')
  else
    version="{{version}}"
  fi

  if [[ -z "$version" ]]; then
    echo "Error packaging $bin. The version number was not retrieved."
    exit 1
  fi

  rm -rf packaged_bins/$bin
  find artifacts/ -name "$bin" -exec chmod +x '{}' \;
  for arch in "${architectures[@]}" ; do
    echo "Packaging for $arch..."
    if [[ $arch == *"windows"* ]]; then bin_name="${bin}.exe"; else bin_name=$bin; fi
    zip -j $bin-$version-$arch.zip artifacts/$arch/release/$bin_name
    tar -C artifacts/$arch/release -zcvf $bin-$version-$arch.tar.gz $bin_name
  done

  mkdir -p packaged_bins/$bin
  mv *.tar.gz packaged_bins/$bin
  mv *.zip packaged_bins/$bin

upload-all-packaged-bins-to-s3:
  #!/usr/bin/env bash
  set -e

  binaries=(
    node-launchpad
    ant
    antnode
    antctl
    antnode_rpc_client
  )
  for binary in "${binaries[@]}"; do
    just upload-packaged-bin-to-s3 "$binary"
  done

upload-packaged-bin-to-s3 bin_name:
  #!/usr/bin/env bash
  set -e

  case "{{bin_name}}" in
    node-launchpad)
      bucket="node-launchpad"
      ;;
    ant)
      bucket="autonomi-cli"
      ;;
    antnode)
      bucket="antnode"
      ;;
    antctl)
      bucket="antctl"
      ;;
    antnode_rpc_client)
      bucket="antnode-rpc-client"
      ;;
    *)
      echo "The {{bin_name}} binary is not supported"
      exit 1
      ;;
  esac

  cd packaged_bins/{{bin_name}}
  for file in *.zip *.tar.gz; do
    dest="s3://$bucket/$file"
    if [[ "$file" == *latest* ]]; then
      echo "Allowing overwrite for 'latest' version..."
      aws s3 cp "$file" "$dest" --acl public-read
    else
      if aws s3 ls "$dest" > /dev/null 2>&1; then
        echo "$dest already exists. Will not overwrite."
      else
        # This command outputs a lot text which makes the build log difficult to read, so we will
        # suppress it.
        aws s3 cp "$file" "$dest" --acl public-read > /dev/null 2>&1
        echo "$dest uploaded"
      fi
    fi
  done

delete-s3-bin bin_name version:
  #!/usr/bin/env bash
  set -e

  case "{{bin_name}}" in
    node-launchpad)
      bucket="node-launchpad"
      ;;
    ant)
      bucket="autonomi-cli"
      ;;
    antnode)
      bucket="antnode"
      ;;
    antctl)
      bucket="antctl"
      ;;
    antnode_rpc_client)
      bucket="antnode-rpc-client"
      ;;
    *)
      echo "The {{bin_name}} binary is not supported"
      exit 1
      ;;
  esac

  architectures=(
    "x86_64-pc-windows-msvc"
    "x86_64-apple-darwin"
    "aarch64-apple-darwin"
    "x86_64-unknown-linux-musl"
    "arm-unknown-linux-musleabi"
    "armv7-unknown-linux-musleabihf"
    "aarch64-unknown-linux-musl"
  )

  for arch in "${architectures[@]}"; do
    zip_filename="{{bin_name}}-{{version}}-${arch}.zip"
    tar_filename="{{bin_name}}-{{version}}-${arch}.tar.gz"
    s3_zip_path="s3://$bucket/$zip_filename"
    s3_tar_path="s3://$bucket/$tar_filename"
    aws s3 rm "$s3_zip_path"
    echo "deleted $s3_zip_path"
    aws s3 rm "$s3_tar_path"
    echo "deleted $s3_tar_path"
  done

package-all-architectures:
  #!/usr/bin/env bash
  set -e

  architectures=(
    "x86_64-pc-windows-msvc"
    "x86_64-apple-darwin"
    "aarch64-apple-darwin"
    "x86_64-unknown-linux-musl"
    "arm-unknown-linux-musleabi"
    "armv7-unknown-linux-musleabihf"
    "aarch64-unknown-linux-musl"
  )

  rm -rf packaged_architectures
  for arch in "${architectures[@]}" ; do
    echo "Packaging artifacts for $arch..."
    just package-arch "$arch"
  done

package-arch arch:
  #!/usr/bin/env bash
  set -e

  if [[ -n $PACKAGE_VERSION ]]; then
    version="$PACKAGE_VERSION"
  else
    release_year=$(grep 'release-year:' release-cycle-info | awk '{print $2}')
    release_month=$(grep 'release-month:' release-cycle-info | awk '{print $2}')
    release_cycle=$(grep 'release-cycle:' release-cycle-info | awk '{print $2}')
    release_cycle_counter=$(grep 'release-cycle-counter:' release-cycle-info | awk '{print $2}')
    version="$release_year.$release_month.$release_cycle.$release_cycle_counter"
  fi
  architecture="{{arch}}"
  zip_filename="${version}.autonomi.${architecture}.zip"

  mkdir -p packaged_architectures
  cd artifacts/$architecture/release

  binaries=(
    node-launchpad
    ant
    antnode
    antctl
    antnode_rpc_client
  )

  if [[ "$architecture" == *"windows"* ]]; then
    for binary in "${binaries[@]}"; do
      binaries_with_extension+=("$binary.exe")
    done
    zip "../../../packaged_architectures/$zip_filename" "${binaries_with_extension[@]}"
  else
    zip "../../../packaged_architectures/$zip_filename" "${binaries[@]}"
  fi

  cd ../../..
