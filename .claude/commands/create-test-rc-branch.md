# Create Release Candidate Branch for Testing

@release-cycle-info
@ant-build-info/src/release_info.rs
@ant-cli/src/main.rs
@resources/scripts/remove-s3-ant-rc-binaries.sh

**CRITICAL**:
The current branch must not have any outstanding changes. If this is the case, stop and ask the user
to commit those changes.

I need to create a release candidate branch for testing a change to the release process. To
facilitate this I want a quick change to the `ant` binary, just so we will have a different binary
in the release that could potentially be tested if we need to.

Please follow these steps:

- Read the current release info from the release cycle info file
- Create a branch from the current branch called `rc-<release-year>.<release-month>.<release-cycle>
  + 1`. So the current release cycle value should be incremented by 1. If a branch with the same
  name already exists, prompt the user to ask if they want to delete it. This may have been left
  behind from a previous test.
- Make a quick change to the `ant` binary by changing the `main.rs` file to print out the text
  "Welcome to ant".
- Make a quick change to the `node-launchpad` binary by changing the `status.rs` file. Add some text
  under the "Each node will use..." line that says "Welcome to launchpad".
- Commit both of those changes in a `chore` commit and use the body of the commit message to
  indicate it is change to facilitate testing.
- Update the `release-cycle-info` file by incrementing the `release-cycle` value by 1 and resetting
  the `release-cycle-counter` value to 1.
- Do the same to the equivalent values in the `release_info.rs` file.
- Get the current version of `ant-cli` from the `Cargo.toml` and increment the patch version by 1
  with an `-rc.1` suffix, using `cargo release`. You can use the command `cargo release version
  <current major>.<current minor>.<patch incremented by 1>-rc.1 --package ant-cli --execute
  --no-confirm`. Do the same for `node-launchpad`.
- Create a `chore(release): release candidate
  <release-year>.<release-month>.<release-cycle>.<release-cycle-counter>` commit. Use the body of
  the commit message to indicate it is a testing RC and will be removed.
- We need to make sure there are no previous `ant` test binaries with the same RC version. You can
  use the `remove-s3-ant-rc-binaries.sh` script to do this.
- We need to make sure there are no previous `node-launchpad` test binaries with the same RC
  version. You can use the `remove-s3-node-launchpad-rc-binaries.sh` script to do this.
- We also need to make sure the `upstream` repository does not have a tag in the form
  `rc-<release-year>.<release-month>.<release-cycle>.<release-cycle-counter>`. This is because the
  fake release candidate we will use is going to need this tag, and it may have been left behind
  from a previous test.
- Push the branch to the `upstream` repository. If there is an error on the push, do *not* force
  push to the branch since it is on `upstream`. The user can inspect what is going on here.
