# Perform Release from Release Candidate Branch

@release-cycle-info
@.github/workflows/release.yml

I need you to assist me in doing a new release for this repository.

We are currently working from a release candidate branch and we already have a changelog prepared.

There are 9 phases:

* Reviewing secrets
* Removing pre-release identifiers from any Rust crate versions
* Removing pre-release identifiers from any NodeJS packages with language bindings
* Creating the release commit
* Submit a PR with for the current release candidate branch
* Merge the release candidate branch into the `stable` branch
* Publish crates from the `stable` branch
* Running the `release` workflow
* Update the release description

## Slack Updates

Before we start the process, I want to inform you how to post any messages to Slack.

You should use the `slack_post_message` operation from the currently configured MCP server to post
messages to the #releases channel. All messages should be prefixed with `[timestamp] <release-year>.<release-month>.<release-cycle>.<release-cycle-counter>: `, which you can obtain from the `release-cycle-info` file.

## Phase 1: Reviewing Secrets

Post a message to Slack indicating that the release process is beginning.

Before we proceed with the process, I need you to remind me to check my personal access tokens on
the `Maidsafe-QA` user on Github. If the `autonomi-github_release` token has expired, it needs to be
regenerated, and I need to update my `~/.bashrc` to set the `AUTONOMI_RELEASE_TOKEN` variable with
the new token.

After this, you will need to run `source ~/.bashrc` to get the updated token. It is going to be used
in the crate publishing phase.

Do not proceed to the next phase until I have confirmed with you that I have either updated my token
or the current token is still good.

## Phase 2: Removing pre-release identifiers from Rust crates

First, I need you to inspect all our crates and take note of any that currently have an `rc`
pre-release identifier. For each of these crates, use the `cargo release` tool to remove the
pre-release identifier:
```
cargo release version release --package <crate-name> --execute --no-confirm
```

Give me a summary of the crates you intend to bump and let me review them before executing the
commands.

## Phase 3: Removing pre-release identifiers from NodeJS packages

We have two NodeJS packages in the repository: `autonomi-nodejs` and `ant-node-nodejs`.

You need to check if either of these packages currently have an `rc` pre-release identifier. If so,
they also need to have it removed. This can be done using the following:
```
npm version "<current version without the pre-release identifier>" --no-git-tag-version
```

Give me a summary of the bumps you intend to make and let me review them before executing the
commands. If no bumps are required let me know that too. In any case, wait for input from me before
continuing.

## Phase 4: Create the release commit

Stage all the current changes then commit them. For the title of the commit, use `chore(release):
stable release <release-year>.<release-month>.<release-cycle>.<release-cycle-counter>`. You can get
all the release information from the `release-cycle-info` file.

For the body of the commit, you can run the Bash script at `resources/scripts/print-versions.sh` and
use its output.

## Phase 5: Submit a PR to main for the release candidate branch

The release candidate branch now needs to be merged into the `main` branch. You should submit a
pull request to the `maidsafe/autonomi` repository. The title and body/description of the PR should
be the same as the title/body of the release commit.

The test suite takes a long time to run, but we need to wait here for it to finish and the branch to
be merged. I can let you know when to proceed.

Post a message to Slack indicating the release candidate branch has been submitted as a PR to the
`main` branch. A link to the PR would be handy in the message.

## Phase 6: Merge the release candidate branch into the stable branch

The release candidate branch now needs to be merged into the `stable` branch. Since the test suite
ran on the `main` branch, it's OK to just merge it to `stable` without submitting a PR. You can
checkout the `stable` branch then use `git merge --no-ff <release candidate branch name>`. Then the
`stable` branch can be pushed to the `origin`. Wait for me to approve before pushing.

Post a message to Slack indicating the release candidate branch has been merged into the `stable`
branch and that it was pushed to the origin.

## Phase 7: Publish crates

Post a message to Slack indicating that we are starting the crate publishing phase.

Continuing to work from our checkout of the `stable` branch, use the `release-plz` tool to publish
the Rust crates:
```
release-plz release --git-token $AUTONOMI_RELEASE_TOKEN
```

This command can run for a long time, depending on how many crates need published. We need to wait
for it to finish before proceeding to the next phase. If it fails, you need to stop here and not
continue before I instruct you to.

Post a message to Slack indicating the result, whether it passed or failed.

## Phase 8: Running the release workflow

Now I want you to run the `release` workflow on this repository. You can see it has inputs for which
binaries to release and they are all `false` by default. Before you run the workflow, I need you to
allow me to choose which of the binaries will be released. Then you can run the workflow with my
choices.

Before you run the workflow, please ask for my confirmation to review the inputs you intend to run
it with.

Post a message to Slack indicating that the workflow has been dispatched. A link to the workflow run
would be useful in the message.

We need to wait for it to complete. Monitor the run yourself, then when it completes, prompt me to
confirm on moving to the next phase.

Post a message to Slack indicating that the workflow has completed either successfully or with an
error. If successful, the message should indicate that the release is now available.

Once it's been done, check the output of the `s3 release` job to make sure it only uploaded the
binary that was chosen. The others should have retained their current copy.

## Phase 9: Update the release description

Once the release has been created, the body of the release should be updated with a "## Detailed
Changes" section. Important: this should be *appended* to the body; do *not* delete what is in the
body of the release currently.

The detailed changes should be the latest entry in the `CHANGELOG.md` file.
