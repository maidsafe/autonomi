I need to carry out a backwards compatibility test that involves working with testnets.

There are several phases:

* Create a `STG-05` testnet with the previously released version
* Node-side smoke test for the `STG-05` testnet
* Bootstrap a `STG-06` testnet from `STG-05` using an older version
* Node-side smoke test for the `STG-06` testnet
* Start uploads and downloads using the `ant` client from the previous release
* Client-side smoke test for the `STG-06` testnet
* Let uploads and downloads run using the `ant` client from the previous release
* Upgrade the `ant` client to the RC version
* Start clients again
* Upgrade hosts on the `STG-06` bootstrap network
* Upgrade hosts on the `STG-05` main network
* Verify there have been no upload or download failures

## Slack Updates

Before we start the process, I want to inform you how to post any messages to Slack.

You should use the `slack_post_message` operation from the currently configured MCP server to post
messages to the #releases channel. All messages should be prefixed with `[timestamp]
<release-year>.<release-month>.<release-cycle>.<release-cycle-counter>: `, which you can obtain from
the `release-cycle-info` file. The timestamp should have both the date and time and be in the
ISO-style format (though the timezone doesn't need to be included).

## Phase 1: Create a `STG-05` testnet with the previously released version

Post a message to Slack to indicate the backwards compatibility test is now beginning.

Prompt me to supply the package version for the previous release.

After you have that, use the `gh` tool to get the releases for this repository and find the release
with title matching the package version I provided. From the description for that release you can
obtain the version numbers for `ant`, `antnode` and `antctl`.

Before you proceed, let me review the versions you have obtained.

Now deploy a `STG-05` testnet with the following details:

* 5 generic node VMs with 20 nodes per VM
* The `ant` version should be the version you obtained from me
* The `antnode` version should be the version you obtained from me
* The `antctl` version should be the version you obtained from me
* A custom evm network with the following:
  - rpc-url: https://sepolia-rollup.arbitrum.io/rpc
  - payment-token-address: 0x4bc1aCE0E66170375462cB4E6Af42Ad4D5EC689C
  - data-payments-address: 0xfE875D65021A7497a5DC7762c59719c8531f7146
  - merkle-payments-address: 0x393F6825C248a29295A7f9Bfa03e475decb44dc0

Post a message to Slack to indicate the first staging environment is now being deployed.

Wait for the deployment to complete by polling every 2 minutes. If there is a failure, inform me and
wait for my input.

When we have a successful deployment, post a message to Slack indicating the environment was
deployed and move to the next phase.

## Phase 2: Node-side smoke test for the `STG-05` testnet

Run the node-side smoke tests for `STG-05`.

If there is a failure, inform me and wait for my input before proceeding. Otherwise, post the
results to Slack and proceed to the next phase.

## Phase 3: Bootstrap a `STG-06` testnet from `STG-05` using an older version

Bootstrap a `STG-06` network from the `STG-05` network using the following configuration:
* 5 generic VMs with 10 nodes per VM
* `antnode` <prompt for version>
* `antctl` <prompt for version>
* peer: <from STG-05>
* network-id: 23
* I need to use a custom evm network with the following:
  - rpc-url: https://sepolia-rollup.arbitrum.io/rpc
  - payment-token-address: 0x4bc1aCE0E66170375462cB4E6Af42Ad4D5EC689C
  - data-payments-address: 0xfE875D65021A7497a5DC7762c59719c8531f7146
  - merkle-payments-address: 0x393F6825C248a29295A7f9Bfa03e475decb44dc0

You need to prompt me for the peer address for the `STG-05` network and the previous versions of
`antnode` and `antctl`.

Before you proceed, let me review the versions and peer address you are going to use.

Post a message to Slack to indicate the second staging environment is now being deployed.

Wait for the deployment to complete by polling every 2 minutes. If there is a failure, inform me and
wait for my input.

When we have a successful deployment, post a message to Slack indicating the environment was
deployed and move to the next phase.

## Phase 4: Node-side smoke test for the `STG-06` testnet

Run the node-side smoke tests for `STG-06`.

If there is a failure, inform me and wait for my input before proceeding. Otherwise, post the
results to Slack and proceed to the next phase.

## Phase 5: Run uploads and downloads using the `ant` client from the previous release

Start the uploaders and downloaders for the `STG-05` environment.

Post a message to Slack to say the workflows for starting clients have been dispatched and we will
wait for 10 minutes for some uploads to accumulate.

Now wait for those 10 minutes. After that, proceed to the next phase.

## Phase 6: Client-side smoke test for the `STG-05` testnet

Run the client-side smoke tests for `STG-05`.

If there is a failure, inform me and wait for my input before proceeding. Otherwise, post the
results to Slack and proceed to the next phase.

## Phase 7: Let uploads and downloads run using the `ant` client from the previous release

Post a message to Slack to say we are going to allow uploads and downloads to accumulate for 10
minutes using the previous client version.

Now wait for those 10 minutes.

After that, stop both the uploaders and downloaders for `STG-05`.

Post a message to Slack to say the workflows for stopping clients have been dispatched.

Stop here and remind me to verify we have had no upload or download failures. Then proceed to the
next phase when I signal.

## Phase 8: Upgrade the `ant` client to the RC version

For now this is a step that will be done manually.

So just wait here for my signal to advance to the next phase.

Post a message to Slack to `ant` has been upgraded to the RC version.

## Phase 9: Start clients again

Start the uploaders and downloaders for the `STG-05` environment.

Wait for 10 minutes for some uploads and downloads to accumulate again, then proceed to the next
phase.

## Phase 10: Upgrade hosts on the `STG-06` bootstrap network

I want to upgrade testnet `STG-06` with the following configuration:
* The `antnode` version is the same RC version you obtained from me in phase 1
* Use the `force` argument

Post a message to Slack to say the upgrade to the RC version is beginning for `STG-06` hosts.

Wait for 20 minutes, then remind me to check the upgrade workflow to make sure there were no errors
during the upgrade. I will then signal when you can proceed to the next phase.

Before we move on, post a message to Slack to say the upgrade has completed for `STG-06` hosts.

## Phase 11: Upgrade hosts on the `STG-05` bootstrap network

I want to upgrade testnet `STG-05` with the following configuration:
* The `antnode` version is the same RC version you obtained from me in phase 1
* Use the `force` argument

Post a message to Slack to say the upgrade to the RC version is beginning for `STG-05` hosts.

Wait for 15 minutes, then remind me to check the upgrade workflow to make sure there were no errors
during the upgrade. I will then signal when you can proceed to the next phase.

Before we move on, post a message to Slack to say the upgrade has completed for `STG-05` hosts.

## Phase 12: Verify there have been no upload or download failures

For now this is manual step.

Wait for my input to inform you of the result.

Post a message to Slack to say the backwards compatibility test has completed successfully.
