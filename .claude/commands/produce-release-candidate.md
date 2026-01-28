# Produce Release Candidate

@release-cycle-info
@ant-build-info/src/release_info.rs
@CHANGELOG.md
@.github/workflows/release.yml

I need you to assist me in producing a new release candidate for this repository.

There are 9 phases:

* Preparing the new release candidate
* Determining merged PRs
* Analyzing changes and determining crate bumps
* Bumping Rust crates
* Bumping NodeJS packages
* Generating the changelog
* Creating the release candidate commit
* Pushing the release candidate branch
* Running the release workflow

## Slack Updates

Before we start the process, I want to inform you how to post any messages to Slack.

You should use the `slack_post_message` operation from the currently configured MCP server to post
messages to the #releases channel. All messages should be prefixed with `[timestamp] <release-year>.<release-month>.<release-cycle>.<release-cycle-counter>: `, which you can obtain from the `release-cycle-info` file.

## Phase 1: Preparing the new release candidate

First, prompt me to obtain the new package version for the release, which should be in the following
form:
```
<release-year>.<release-month>.<release-cycle>.<release-cycle-counter>
```

We need a new branch in the form `rc-<release-year>.<release-month>.<release-cycle>` based on the
*new* package version, not the current one. It could be the case the branch was already created and
we are on that branch. If we are not, create it and switch to it.

Now update the `release-cycle-info` and `release_info.rs` files with the values from the new package
version.

Post a message to Slack indicating that the release candidate process is beginning.

## Phase 2: Determining Merged PRs

Use `git log stable..main --oneline --merges` to determine the PR numbers that have been merged
since the last stable release. We use merge commits, so the PR numbers should be readily
identifiable.

Present me with a summary of the merged PRs in the following form:
```
@<author>
  <date> <pr-number> <pr-title>
```

For example:
```
@grumbach
  2025-12-17 #3364 -- Merkle CLI 
  2025-12-18 #3371 -- Merkle client optimized pool creation 
  2025-12-18 #3369 -- Fix merkle tests 
  2025-12-18 #3368 -- Merkle remove abusive xorname use 
  2025-12-23 #3386 -- Merkle high level retries 
  2025-12-23 #3387 -- feat: merkle chunk existance check progress reporting 
  2025-12-23 #3381 -- Merkle payment Client (CLI)

@maqi
  2025-12-18 #3375 -- fix(test): refactor verify_max_parallel_fetches test setup 
  2025-12-22 #3377 -- fix(node): avoid pruning irrelevant_records too aggressively 
  2025-12-22 #3383 -- fix(test): make verify_max_parallel_fetches test deterministic

@mickvandijke
  2025-12-19 #3378 -- fix: failing merkle payment txs

@dirvine
  2025-12-23 #3380 -- fix(ci): resolve ring crate ARM cross-compilation failure
```

So these are grouped by the author. You should be able to obtain this information using the `gh`
tool.

Give me the opportunity to remove any PR from the list. Sometimes we have PRs that were raised from
automated processes like `dependabot` and we are not interested in those PRs being part of the
release list.

Once I indicate we can proceed, post a message to Slack with the PR list summary.

## Phase 3: Analyzing Changes and Determining Crate Bumps

Analyze the changes from the merged PRs to determine which crates need to be bumped and by how much.

For each crate that has changes, determine:
- Whether it needs a `MAJOR`, `MINOR`, or `PATCH` bump based on Semantic Versioning rules
- Whether any changes are breaking changes

For our purposes right now, we only really have `MINOR` and `PATCH` bumps. When there is a breaking
change, we should use a `MINOR` bump.

**CRITICAL**: Check that `ant-protocol` does NOT have a MAJOR bump. If it does, stop immediately and
alert me. A major protocol bump requires special handling and should not proceed through the normal
release process.

Present me with a detailed summary of:
1. Each crate that needs a bump
2. The recommended bump level (MAJOR/MINOR/PATCH)
3. The reasoning for each bump (which PRs contributed to this)
4. Any breaking changes identified

Wait for my review and approval before proceeding. I may adjust the bump levels based on my
knowledge of the changes.

## Phase 4: Bumping Rust Crates

Based on my approved crate bump list, use the `cargo release` tool to bump each crate:

```
cargo release version <new-version>-rc.1 --package <crate-name> --execute --no-confirm
```

For crates that need bumping, the new version should include the `-rc.1` pre-release identifier.

Present me with the commands you intend to run and wait for my confirmation before executing them.

After executing, show me a summary of the version changes that were made and post the same summary
to Slack.

## Phase 5: Bumping NodeJS Language Binding Packages

We have two NodeJS packages in the repository: `autonomi-nodejs` and `ant-node-nodejs`. There are
also Rust crates in the same directories.

If the corresponding Rust crates were bumped in Phase 4, the NodeJS packages also need to be bumped
to match.

For each package that needs bumping, use:
```
npm version "<new-version>-rc.1" --no-git-tag-version
```
This should run from the corresponding crate directory.

Present me with the commands you intend to run and wait for my confirmation before executing. If no
bumps are required, let me know that too. In any case, wait for input from me before continuing.

## Phase 6: Generating the Changelog

Use the `/new-changelog-entry` skill to generate an initial changelog entry for this release
candidate. The entry should cover all changes since the last stable release. The last stable release
is denoted with a tag. This tag can be obtained from the latest release on the repository using the
`gh` command.

The date for the changelog entry should be today's date.

Present me with the generated changelog entry for review. I will likely want to make adjustments
and improvements to the wording. Wait for my approval before proceeding. Once approved, post the
changelog to Slack.

## Phase 7: Creating the Release Candidate Commit

Stage all the current changes then commit them. For the title of the commit, use:
`chore(release): release candidate <release-year>.<release-month>.<release-cycle>.<release-cycle-counter>`

For the body of the commit, you can run the Bash script at `resources/scripts/print-versions.sh`
and use its output.

Show me the commit message you intend to use and wait for my confirmation before creating the
commit.

## Phase 8: Pushing the Release Candidate Branch

Push the release candidate branch to the `origin` repository.

Wait for my approval before pushing. If there is an error on the push, do NOT force push. Stop and
let me inspect what is going on.

Post a message to Slack indicating the release candidate branch has been pushed.

## Phase 9: Running the Release Workflow

Now I want you to run the `release` workflow on this repository. The workflow has inputs for which
binaries to release (they are all `false` by default).

Before you run the workflow, present me with the available binary options and let me choose which
ones should be released. Then show me the complete workflow inputs for my confirmation before
dispatching the workflow.

Post a message to Slack indicating that the workflow has been dispatched. Include a link to the
workflow run.

We need to wait for it to complete. Monitor the run yourself, then when it completes, let me know
the result.

Post a message to Slack indicating the workflow result (success or failure). If successful, note
that the release candidate binaries are now available for testing.
