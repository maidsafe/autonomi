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

Wait for the deployment to complete by polling every 2 minutes.

Once complete, wait for my approval to advance to the next phase.

Post a message to Slack indicating the environment was successfully deployed.

## Phase 2: Node-side smoke test for the `STG-05` testnet

Run the node smoke tests for `STG-05`.

Whether they pass or fail, inform me of the results and wait for my input before proceeding.

Post the results to Slack.

## Phase 3: Bootstrap a `STG-06` testnet from `STG-05` using an older version

Bootstrap a `STG-06` network from the `STG-05` network using the following configuration:
* 5 generic VMs with 10 nodes per VM
* `antnode` version 0.4.9
* `antctl` version 0.13.3
* peer: <from STG-01>
* network-id: 23
* I need to use a custom evm network with the following:
  - rpc-url: https://sepolia-rollup.arbitrum.io/rpc
  - payment-token-address: 0x4bc1aCE0E66170375462cB4E6Af42Ad4D5EC689C
  - data-payments-address: 0xfE875D65021A7497a5DC7762c59719c8531f7146
  - merkle-payments-address: 0x393F6825C248a29295A7f9Bfa03e475decb44dc0

You need to prompt me for the peer address from the `STG-05` network.

Post a message to Slack to indicate the second staging environment is now being deployed.

After you have dispatched the workflow, wait for it to complete by polling every 2 minutes.

Then wait for me to give you the OK to proceed to the next phase.

Post a message to Slack indicating the environment was successfully deployed.

## Phase 4: Node-side smoke test for the `STG-06` testnet

Run the node smoke tests for `STG-06`.

Whether they pass or fail, inform me of the results and wait for my input before proceeding.

Post the results to Slack.

## Phase 5: Run uploads and downloads using the `ant` client from the previous release

Start the uploaders and downloaders for the `STG-05` environment.

Post a message to Slack to say the workflows for starting clients have been dispatched.

Now wait for my input before proceeding to the next phase.

## Phase 6: Client-side smoke test for the `STG-05` testnet

Run the client smoke tests for `STG-01`.

Whether they pass or fail, inform me of the results and wait for my input before proceeding.

Post the results to Slack.

## Phase 7: Let uploads and downloads run using the `ant` client from the previous release

Wait for 10 minutes to allow some uploads and downloads to accumulate using the previous client
version.

Now stop both the uploaders and downloaders for `STG-05`.

Post a message to Slack to say the workflows for stopping clients have been dispatched.

Wait for my signal before proceeding to the next phase. I need to verify that both networks have
received payments. For now this is done manually.

## Phase 8: Upgrade the `ant` client to the RC version

For now this is a step that will be done manually.

So just wait here for my signal to advance to the next phase.

Post a message to Slack to `ant` has been upgraded to the RC version.

## Phase 9: Start clients again

Start the uploaders and downloaders for the `STG-05` environment.

Wait for my signal to proceed to the next phase.

## Phase 10: Upgrade hosts on the `STG-06` bootstrap network

I want to upgrade testnet `STG-06` with the following configuration:
* The `antnode` version is the same RC version you obtained from me in phase 1
* Use the `force` argument

Post a message to Slack to say the upgrade to the RC version is beginning for `STG-06` hosts.

Wait for 30 minutes then prompt for my approval to proceed to the next phase.

Post a message to Slack to say the upgrade has completed for `STG-06` hosts.

## Phase 11: Upgrade hosts on the `STG-05` bootstrap network

I want to upgrade testnet `STG-05` with the following configuration:
* The `antnode` version is the same RC version you obtained from me in phase 1
* Use the `force` argument

Post a message to Slack to say the upgrade to the RC version is beginning for `STG-05` hosts.

Wait for 30 minutes then prompt for my approval to proceed to the next phase.

Post a message to Slack to say the upgrade has completed for `STG-05` hosts.

## Phase 12: Verify there have been no upload or download failures

For now this is manual step.

Wait for my input to inform you of the result.

Post a message to slack to say the backwards compatibility test has completed successfully.
