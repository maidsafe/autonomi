# Prepare for Auto Upgrade Test

@ant-node/Cargo.toml
@ant-node/src/bin/antnode/main.rs
@ant-node/src/bin/antnode/upgrade/mod.rs
@release-cycle-info
@ant-build-info/src/release_info.rs

I want you to help me prepare for a test of the automatic-upgrades process.

## Parameters

Before starting, the user must provide:

- **RELEASE_TAG**: The tag for the fake release that will be fetched during the test. For example,
  `rc-2026.2.2.2`.
- **UPSTREAM_BRANCH_NAME**: The name of the branch to create on the upstream repository. For
  example, `rc-2026.2.2`.

## Overview

The process has 4 phases:

* Obtain additional parameters
* Clear previous test artifacts
* Prepare the current branch for an auto-upgrade test
* Prepare a new branch with a fake release candidate

# Phase 1: Obtain additional parameters

We need to obtain additional parameters that will be used throughout the process.

Follow these steps:
- Read the current release info from the release cycle info file.
- Create a `RELEASE_CYCLE` parameter by adding 10 to the current `release-cycle` value.
- Create a `RELEASE_CYCLE_COUNTER` parameter by setting it to the value 1.
- Create a `BRANCH_NAME` parameter with the value `rc-<release-year>.<release-month>.<release-cycle> +
  10`. So you will be adding 10 to the current `release-cycle` value and should end up with
  something like, e.g., `rc-2026.2.12`.
- Create a `TAG_NAME` parameter by taking the value of `BRANCH_NAME` and appending `.1`. So you
  should end up with something like, e.g., `rc-2026.2.12.1`.
- Create an `ANTNODE_VERSION_RC_VERSION` number by reading the current `antnode` version number from
  the `Cargo.toml` file and adding 10 to the `PATCH` component and appending an `-rc.1` suffix. So for
  example, if the current version number is `0.4.16`, the new RC version should be `0.4.26-rc.1`.

Print the values you have obtained for these parameters and prompt me to verify them before
proceeding to the next phase.

# Phase 2: Clear previous test artifacts

It could be possible there was a previous test run that has not been cleared up. We need to get rid
of any artifacts from any previous test.

Follow these steps:
- If a `BRANCH_NAME` branch exists on the current repository, delete it.
- If a `BRANCH_NAME` branch exists on the upstream repository, delete it.
- If a `TAG_NAME` tag exists on the upstream repository, delete it.
- Use the `remove-s3-antnode-rc-binaries.sh` script by calling it with the value of the
 `ANTNODE_VERSION_RC_VERSION` parameter 

# Phase 3: Preparing the current branch

For the test, the current branch needs a temporary commit to reduce the upgrade check time and some
other details.

Follow these steps:

* Reduce the upgrade check time from 3 days to 30 minutes.
* Reduce the randomness in the upgrade check time to +/- 2 minutes.
* Change the `fetch_and_cache_release_info` function to replace the usage of
  `release_repo.get_latest_autonomi_release_info` to `release.get_autonomi_release_info`. This
  function takes a string, which is the value of `RELEASE_TAG`
* We have code that downloads the new binary from an `autonomi.com` URL and uses an S3 URL as a
  fallback. Another temporary change is required here to just use the S3 URL directly, since the
  `autonomi.com` URL always points to the latest release, which is not what we want for the test.
* Create a chore commit with these changes and indicate that it is temporary and will be removed.
  Let me review the commit before proceeding.
* Push the change to the remote branch. Force pushing is fine if necessary.

# Phase 4: Prepare fake release candidate branch

Now we need to prepare a fake release candidate branch that is based on the current branch.

Follow these steps:
* Create a new branch from the current one called `BRANCH_NAME`
* Run `cargo release version ANTNODE_VERSION_RC_VERSION --package ant-node --execute
  --no-confirm`.
* Update the `release-cycle-info` and `ant-build-info/src/release_info.rs` files with the values of
  `RELEASE_CYCLE_COUNTER` and `RELEASE_CYCLE`.
* Put these changes in a commit with the title `chore(release): release candidate
  <release_year>.<release_month>.RELEASE_CYCLE_COUNTER.RELEASE_CYCLE` and in the body, indicate that
  it's a fake release candidate.

Allow me to review the commit, and once I approve, push the branch to the `upstream` remote.
