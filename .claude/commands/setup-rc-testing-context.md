I need you to setup two testnets and two client deployments that will be used for testing a release
candidate.

There are several phases:

* Deploy a `STG-01` testnet with the release candidate
* Deploy a `STG-02` testnet with the stable release
* Node-side smoke test for the `STG-01` testnet
* Node-side smoke test for the `STG-02` testnet
* Start the clients for both testnets
* Client-side smoke test for the `STG-01` testnet
* Client-side smoke test for the `STG-02` testnet
* Create a comparison in the Testnet Registry database
* Create a Linear issue for the comparison in the release candidate project
* Create a Slack post for the comparison
* Deploy a `STG-03` client for testing production downloads
* Deploy a `STG-04` client for testing production uploads

## Slack Updates

Before we start the process, I want to inform you how to post any messages to Slack.

You should use the `slack_post_message` operation from the currently configured MCP server to post
messages to the #releases channel. All messages should be prefixed with `[timestamp]
<release-year>.<release-month>.<release-cycle>.<release-cycle-counter>: `, which you can obtain from
the `release-cycle-info` file. The timestamp should have both the date and time and be in the
ISO-style format (though the timezone doesn't need to be included).

## Phase 1: Deploy a `STG-01` testnet with the release candidate

Use the `gh` command to obtain the recent releases for this repository and find the most recent
release candidate pre-release.

Obtain the versions of the `ant`, `antnode` and `antctl` binaries from the description for that
release. You should ensure they all have an `-rc.X` suffix, where `X` would be an integer. The
release description has a `Binary Versions` section at the top.

Prompt me to confirm the versions you obtained are correct.

Now launch a `STG-01` testnet with the `rc` preset using the version numbers you obtained.

Post a message to Slack to indicate the test staging environment is now being deployed.

Wait for the deployment to complete by polling every 2 minutes for its status. If you find an error
in the status query, inform me and wait till I direct you further before proceeding.

Post a message to Slack indicating the environment was successfully deployed.

## Phase 2: Deploy a `STG-02` testnet with the latest version

The stable release can be obtained by getting the latest release from the `maidsafe/autonomi` Github
repository. The release description has a `Binary Versions` section at the top.

Prompt me to confirm the versions you obtained are correct.

Now launch a `STG-02` testnet with the `rc` preset using the version numbers you obtained.

Post a message to Slack to indicate the reference staging environment is now being deployed.

Wait for the deployment to complete by polling every 2 minutes for its status. If you find an error
in the status query, inform me and wait till I direct you further before proceeding.

Post a message to Slack indicating the environment was successfully deployed.

## Phase 3: Node-side smoke test for the `STG-01` testnet

Run the node smoke tests for `STG-01`.

Whether they pass or fail, inform me of the results and wait for my input before proceeding.

Post the results to Slack.

## Phase 4: Node-side smoke test for the `STG-02` testnet

Run the node smoke tests for `STG-02`.

Whether they pass or fail, inform me of the results and wait for my input before proceeding.

Post the results to Slack.

## Phase 5: Start the clients for both testnets

Now we need to start the clients for both testnets.

Start the uploaders and downloaders for both `STG-01` and `STG-02`.

Post a message to Slack to say the workflows for starting clients have been dispatched.

Now wait for my input before proceeding to the next phase.

## Phase 6: Client-side smoke test for the `STG-01` testnet

Run the client smoke tests for `STG-01`.

Whether they pass or fail, inform me of the results and wait for my input before proceeding.

Post the results to Slack.

## Phase 7: Client-side smoke test for the `STG-02` testnet

Run the client smoke tests for `STG-02`.

Whether they pass or fail, inform me of the results and wait for my input before proceeding.

Post the results to Slack.

## Phase 8: Create a comparison in the Testnet Registry database

Create a comparison between `STG-01` and `STG-02`, where the former is the test environment and the
latter is the reference.

The label to be used should be: `<RC package version> vs <stable release package version>`. The
package version is the title of the respective Github releases for the RC and the latest stable
release. It is in the form `YYYY.M.X.Y`, where each of those are integers.

Prompt me to confirm the label you are going to use before you create the comparison.

Take note of the ID of the comparison because it will be used in the next phase.

## Phase 9: Create a Linear issue for the comparison in the release candidate project

Post comparison `<id>` to Linear with the following details:
  * `Releases` team
  * The `Release Candidate <package version>` project
  * The `Environment Comparison` test type
  * Label: `<RC package version> RC [STG-01] vs <stable release package version> [STG-02]`

The `<id>` is the ID of the comparison you created in the last phase.

Prompt me to confirm exactly which value you are going to use.

## Phase 10: Create a Slack post for the comparison

Post the `<id>` comparison to Slack.

Again, the ID comes from the comparison created in phase downloads.

## Phase 11: Deploy a `STG-03` client for testing production downloads

Launch a `STG-03` client deployment with the following details:
* Use the `production-downloads` preset
* The `ant` version should be the RC version obtained from phase 1

Prompt me to confirm you are using the correct version.

Post a message to Slack to indicate the `STG-03` environment is now being deployed.

Wait for the deployment to complete by polling every 2 minutes for its status. If you find an error
in the status query, inform me and wait till I direct you further before proceeding.

Post a message to Slack indicating the environment was successfully deployed.

## Phase 12: Deploy a `STG-04` client for testing production uploads

Launch a `STG-04` client deployment with the following details:
* Use the `production-uploads` preset
* The `ant` version should be the RC version obtained from phase 1

Prompt me to confirm you are using the correct version.

Post a message to Slack to indicate the `STG-04` environment is now being deployed.

Wait for the deployment to complete by polling every 2 minutes for its status. If you find an error
in the status query, inform me and wait till I direct you further before proceeding.

Post a message to Slack indicating the environment was successfully deployed.
