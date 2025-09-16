# Antctl

Antctl is a command-line application for installing, managing, and operating `antnode` as a service.

It runs on Linux, macOS and Windows.

## Architecture Overview

- `antctl` (this crate's binary) is the entry point; CLI arguments are defined in [`src/bin/antctl/commands.rs`](src/bin/antctl/commands.rs) and forwarded into the library.
- The crate leans on [`ant_service_management`](../ant-service-management) for the node registry (`NodeRegistryManager`), filesystem helpers, metrics polling, and cross-platform service control.
- Service workflows (`add`, `start`, `stop`, `remove`, `reset`, `upgrade`, `balance`) live in `src/cmd/node.rs` and operate on OS services.
- Local network orchestration (`local run|join|kill|status`) lives in `src/cmd/local.rs`, which also manages the bundled `evm-testnet` helper process.
- `batch_service_manager` coordinates batch start/stop/upgrade/remove operations with per-service progress reporting.
- Downloads, version detection, WinSW bootstrapping, and port-range validation are shared via `helpers.rs` and the `add_services` module.
- Registry data is stored at `/var/antctl/node_registry.json` for root installs, under `~/.local/share/autonomi/node` for user-mode services, and `C:\\ProgramData\\antctl\\node_registry.json` on Windows.

## Installation

The latest version can be installed via [antup](https://github.com/maidsafe/antup):
```
antup antctl
```

A binary can also be obtained for your platform from the releases in this repository.

## Nodes as Services

The primary use case for Antctl is to setup `antnode` as a long-running background service, using
the service infrastructure provided by the operating system.

On macOS and most distributions of Linux, user-mode services are supported. Traditionally, services
are system-wide infrastructure that require elevated privileges to create and work with. However,
with user-mode services, they can be defined and used without sudo. The main difference is, a
user-mode service requires an active user session, whereas a system-wide service can run completely
in the background, without any active session. It's a user decision as to which is more appropriate
for their use case. On Linux, some service managers, like OpenRC, used on Alpine, do not support
user-mode services. Most distributions use Systemd, which does have support for them.

The commands defined in the rest of this guide will operate on the basis of a user-mode service, and
so will not use `sudo`. If you would like to run system-wide services, you can go through the same
guide, but just prefix each command with `sudo`.

Windows does not support user-mode services at all, and therefore, Antctl must always be
used in an elevated, administrative session.

### Create Services

First, use the `add` command to create some services:
```
$ antctl add --count 3 --peer /ip4/139.59.168.228/udp/56309/quic-v1/p2p/12D3KooWFTMtaqu24ddDSXk9v5YxnuhJmTLFRunER1CG4wZ2XLUU
```

This downloads the latest version of the `antnode` binary and creates three services that will initially connect to the specified peer. Soon, specification of a peer will not be required.

There are many arguments available for customising the service. For example, you can choose the port the node will run on, or the version of `antnode`. Run `antctl add --help` to see all available options.

_Note_: elevated privileges are required for creating services, on all platforms.

Now run the `status` command:
```
$ antctl status
=================================================
                Antnode Services
=================================================
Refreshing the node registry...
Service Name       Peer ID                                              Status  Connected Peers
antnode1          -                                                    ADDED               -
antnode2          -                                                    ADDED               -
antnode3          -                                                    ADDED               -
```

We can see the services have been added, but they are not running yet.

### Start Services

Use the `start` command to start each service:
```
$ antctl start
```

Providing no arguments will start all available services. If need be, it's possible to start services individually, using the `--service-name` argument.

With the services started, run the `status` command again:
```
$ antctl status
=================================================
                Antnode Services
=================================================
Refreshing the node registry...
Service Name       Peer ID                                              Status  Connected Peers
antnode1          12D3KooWGQu92xCXuiK6AysbHn6kHyfXqyzNDxNGnnDTgd56eveq RUNNING              81
antnode2          12D3KooWQGVfcwrPFvC6PyCva1cJu8NZVhdZCuPHJ4vY79yuKC3A RUNNING              82
antnode3          12D3KooWMqRH6EF1Km61TAW9wTuv9LgDabKMY9DJSGyrxUafXP6b RUNNING              79
```

We can see our services are running and the nodes have connections to other peers.

Now, run the `status` command again, but with the `--details` flag:
```
=================================================
                Antnode Services
=================================================
Refreshing the node registry...
============================
antnode1 - RUNNING
============================
Version: 0.105.0
Peer ID: 12D3KooWGQu92xCXuiK6AysbHn6kHyfXqyzNDxNGnnDTgd56eveq
RPC Socket: 127.0.0.1:41785
Listen Addresses: Some["/ip4/127.0.0.1/udp/34653/quic-v1/p2p/12D3KooWGQu92xCXuiK6AysbHn6kHyfXqyzNDxNGnnDTgd56eveq", "/ip4/192.168.121.7/udp/34653/quic-v1/p2p/12D3KooWGQu92xCXuiK6AysbHn6kHyfXqyzNDxNGnnDTgd56eveq"]
PID: 3137
Data path: /var/antctl/services/antnode1
Log path: /var/log/antnode/antnode1
Bin path: /var/antctl/services/antnode1/antnode
Connected peers: 10
<remaining output snipped>
```

We get some more details for each node, including the path to its logs. Using the location provided, feel free to take a look at the logs being generated by the node.

The nodes could now be left running like this, but for the purposes of this guide, we will do some more things.

### Service Lifecycle Cheat Sheet

Most service commands refresh the node registry automatically, but you can always inspect the persisted state with `antctl status --json`.

| Command | Description |
| --- | --- |
| `antctl add` | Downloads (or reuses) an `antnode` binary, prepares per-service data/log directories, installs OS services, and records environment variables provided with `--env KEY=VALUE`. Accepts single ports (`--metrics-port 13000`) or ranges (`--metrics-port 13000-13005`). |
| `antctl start` | Starts all or filtered services, staggering launches by the default 10s interval (`--interval` overrides). Performs reachability checks unless `--no-startup-check` is set. |
| `antctl stop` | Stops running services while keeping install directories intact. No-ops for services already stopped or never started. |
| `antctl upgrade` | Fetches a target binary (latest, specific `--version`, `--url`, or `--path`) and replaces service binaries. Supports `--force`, `--do-not-start`, and reuses stored env vars when new ones are omitted. |
| `antctl remove` | Uninstalls services and optionally removes data/log directories unless `--keep-directories` is supplied. Refuses to remove services still running. |
| `antctl reset` | Convenience helper that stops and removes every service, then deletes the registry file (prompts unless `--force`). |

All service subcommands accept repeated `--service-name` and `--peer-id` flags to operate on subsets of nodes.

### Add More Nodes

It's possible to run the `add` command again, as before:
```
antctl add --count 3 --peer /ip4/46.101.80.187/udp/58070/quic-v1/p2p/12D3KooWKgJQedzCxrp33u3dBD1mUZ9HTjEjgrxskEBvzoQWkRT9
```

The subsequent `status` command will show us an additional three nodes, for a total of six:
```
$ antctl status
=================================================
                Antnode Services
=================================================
Refreshing the node registry...
Service Name       Peer ID                                              Status  Connected Peers
antnode1          12D3KooWGQu92xCXuiK6AysbHn6kHyfXqyzNDxNGnnDTgd56eveq RUNNING               4
antnode2          12D3KooWQGVfcwrPFvC6PyCva1cJu8NZVhdZCuPHJ4vY79yuKC3A RUNNING               4
antnode3          12D3KooWMqRH6EF1Km61TAW9wTuv9LgDabKMY9DJSGyrxUafXP6b RUNNING               3
antnode4          -                                                    ADDED               -
antnode5          -                                                    ADDED               -
antnode6          -                                                    ADDED               -
```

Again, the new nodes have not been started.

Run the `start` command to start them, then observe the status:
```
$ antctl status
=================================================
                Antctl Services
=================================================
Refreshing the node registry...
Service Name       Peer ID                                              Status  Connected Peers
antnode1          12D3KooWGQu92xCXuiK6AysbHn6kHyfXqyzNDxNGnnDTgd56eveq RUNNING             138
antnode2          12D3KooWQGVfcwrPFvC6PyCva1cJu8NZVhdZCuPHJ4vY79yuKC3A RUNNING             177
antnode3          12D3KooWMqRH6EF1Km61TAW9wTuv9LgDabKMY9DJSGyrxUafXP6b RUNNING             144
antnode4          12D3KooWLH9VRAoUMj4bUjtzcKS3mqfzyc46TxBkBzvUXfV1bjaT RUNNING               2
antnode5          12D3KooWEcbpvSSTmSyuzqP3gE9bE7uqYFatHhkJXr8PBiqmESEG RUNNING               1
antnode6          12D3KooWBip2g5FakT1dZHdrhdmnctgKqhbRBQA5ZpvtHh4XPRXJ RUNNING              30
```

### Removing Nodes

If for some reason we want to remove one of our nodes, we can do so using the `remove` command.

Suppose we wanted to remove the 5th service. First of all, we need to stop the service. Run the following command:
```
$ antctl stop --service-name antnode5
```

Observe that `antnode5` has been stopped, but the others are still running:
```
$ antctl status
=================================================
                Antnode Services
=================================================
Refreshing the node registry...
Service Name       Peer ID                                              Status  Connected Peers
antnode1          12D3KooWGQu92xCXuiK6AysbHn6kHyfXqyzNDxNGnnDTgd56eveq RUNNING              10
antnode2          12D3KooWQGVfcwrPFvC6PyCva1cJu8NZVhdZCuPHJ4vY79yuKC3A RUNNING               5
antnode3          12D3KooWMqRH6EF1Km61TAW9wTuv9LgDabKMY9DJSGyrxUafXP6b RUNNING               2
antnode4          12D3KooWLH9VRAoUMj4bUjtzcKS3mqfzyc46TxBkBzvUXfV1bjaT RUNNING               2
antnode5          12D3KooWEcbpvSSTmSyuzqP3gE9bE7uqYFatHhkJXr8PBiqmESEG STOPPED               -
antnode6          12D3KooWBip2g5FakT1dZHdrhdmnctgKqhbRBQA5ZpvtHh4XPRXJ RUNNING              29
```

Now that it's been stopped, remove it:
```
$ antctl remove --service-name antnode5
```

The `status` command will no longer show the service:
```
vagrant@ubuntu2204:~$ antctl status
=================================================
                Antnode Services
=================================================
Refreshing the node registry...
Service Name       Peer ID                                              Status  Connected Peers
antnode1          12D3KooWGQu92xCXuiK6AysbHn6kHyfXqyzNDxNGnnDTgd56eveq RUNNING               2
antnode2          12D3KooWQGVfcwrPFvC6PyCva1cJu8NZVhdZCuPHJ4vY79yuKC3A RUNNING              96
antnode3          12D3KooWMqRH6EF1Km61TAW9wTuv9LgDabKMY9DJSGyrxUafXP6b RUNNING             127
antnode4          12D3KooWLH9VRAoUMj4bUjtzcKS3mqfzyc46TxBkBzvUXfV1bjaT RUNNING              76
antnode6          12D3KooWBip2g5FakT1dZHdrhdmnctgKqhbRBQA5ZpvtHh4XPRXJ RUNNING             133
```

However, we will still see it in the detailed view:
```
$ antctl status --details
=================================================
                Antnode Services
=================================================
Refreshing the node registry...
<output snipped>
============================
antnode5 - REMOVED
============================
Version: 0.105.0
Peer ID: 12D3KooWEcbpvSSTmSyuzqP3gE9bE7uqYFatHhkJXr8PBiqmESEG
RPC Socket: 127.0.0.1:38579
Listen Addresses: Some(["/ip4/127.0.0.1/udp/58354/quic-v1/p2p/12D3KooWEcbpvSSTmSyuzqP3gE9bE7uqYFatHhkJXr8PBiqmESEG", "/ip4/192.168.121.7/udp/58354/quic-v1/p2p/12D3KooWEcbpvSSTmSyuzqP3gE9bE7uqYFatHhkJXr8PBiqmESEG"])
PID: -
Data path: /var/antctl/services/antnode5
Log path: /var/log/antnode/antnode5
Bin path: /var/antctl/services/antnode5/antnode
Connected peers: -
<output snipped>
```

## Upgrades

Antctl can be used to continually upgrade node services.

Suppose we have five services:
```
$ antctl status
=================================================
                Antnode Services
=================================================
Refreshing the node registry...
Service Name       Peer ID                                              Status  Connected Peers
antnode1          12D3KooWKaNFPoRf8E2vsdSwBNyhWpe7csNkqwXaunNVxXGarxap RUNNING               1
antnode2          12D3KooWNbRBR43rdFR44EAbwzBrED3jWUiWKkHD2oabw2jKZ9eF RUNNING               1
antnode3          12D3KooWGYEuqXRhKVF2WK499oCBpkh9c6K7jy8BNSfqGosSzNZ8 RUNNING               1
antnode4          12D3KooWS6WGnhbSfLywaepfZbbqgxTLhr66N1PXU4GVLWDBRZRF RUNNING               1
antnode5          12D3KooWNdEYQAutzcGo26rZayew2rzE24y5VFVxTnsefNevc1Ly RUNNING               1
```

Using the `--details` flag, we can see they are not at the latest version:
```
$ antctl status --details
=================================================
                Antnode Services
=================================================
Refreshing the node registry...
============================
antnode1 - RUNNING
============================
Version: 0.104.38
Peer ID: 12D3KooWKaNFPoRf8E2vsdSwBNyhWpe7csNkqwXaunNVxXGarxap
RPC Socket: 127.0.0.1:37931
Listen Addresses: Some(["/ip4/127.0.0.1/udp/39890/quic-v1/p2p/12D3KooWKaNFPoRf8E2vsdSwBNyhWpe7csNkqwXaunNVxXGarxap", "/ip4/192.168.121.114/udp/39890/quic-v1/p2p/12D3KooWKaNFPoRf8E2vsdSwBNyhWpe7csNkqwXaunNVxXGarxap"])
PID: 3285
Data path: /var/antctl/services/antnode1
Log path: /var/log/antnode/antnode1
Bin path: /var/antctl/services/antnode1/antnode
Connected peers: 0
<remaining output snipped>
```

For brevity, the remaining output is snipped, but the four others are also at `0.104.38`. At the time of writing, the latest version is `0.105.3`.

We can use the `upgrade` command to get each service on the latest version:
```
$ antctl upgrade
=================================================
           Upgrade Antnode Services
=================================================
Retrieving latest version of antnode...
Latest version is 0.105.3
Downloading antnode version 0.105.3...
Download completed: /tmp/ae310e50-d104-45bc-9619-22e1328d8c8b/antnode
Refreshing the node registry...
<output snipped>
Upgrade summary:
✓ antnode1 upgraded from 0.104.38 to 0.105.3
✓ antnode2 upgraded from 0.104.38 to 0.105.3
✓ antnode3 upgraded from 0.104.38 to 0.105.3
✓ antnode4 upgraded from 0.104.38 to 0.105.3
✓ antnode5 upgraded from 0.104.38 to 0.105.3
```

Again, for brevity some output from the command was snipped, but the summary indicates that each service was upgraded from `0.104.38` to `0.105.3`.

As with other commands, if no arguments are supplied, `upgrade` operates over all services, but it's possible to use the `--service-name` or `--peer-id` arguments to upgrade specific services. Both those arguments can be used multiple times to operate over several services.

Antctl will determine the latest version of `antnode`, download it, then for each running service, if the service is older than the latest, it will stop it, copy the new binary over the old one, and start the service again.

### Downgrading

In some situations, it may be necessary to downgrade `antnode` to a previous version. The `upgrade` command supports this by providing `--version` and `--force` arguments. Each of those can be used to force Antctl to accept a lower version.

## Local Networks

Antctl can also create local networks, which are useful for development or quick experimentation. In a local network, nodes will run as processes rather than services. Local operations are defined under the `local` subcommand.

To create a local network, use the `run` command:
```
$ antctl local run
=================================================
             Launching Local Network
=================================================
Retrieving latest version for faucet...
Downloading faucet version 0.4.3...
Download completed: /tmp/4dc310dd-74ef-4dc5-af36-3bc92a882db1/faucet
Retrieving latest version for antnode...
Downloading antnode version 0.105.3...
Download completed: /tmp/f63d3ca8-2b8e-4630-9df5-a13418d5f826/antnode
Launching node 1...
Logging to directory: "/home/chris/.local/share/autonomi/node/12D3KooWPArH2XAw2sapcthNNcJRbbSuUtC3eBZrJtxi8DfcN1Yn/logs"

Node started

<remaining output snipped>
```

_Note_: elevated privileges are not required for local networks.

Check the output of the `status` command:
```
$ antctl status
=================================================
                Local Network
=================================================
Refreshing the node registry...
Service Name       Peer ID                                              Status  Connected Peers
antnode-local1    12D3KooWPArH2XAw2sapcthNNcJRbbSuUtC3eBZrJtxi8DfcN1Yn RUNNING               7
antnode-local2    12D3KooWShWom22VhgkDX7APqSzCmXPNsfZA17Y2GSJpznunAp8M RUNNING               0
antnode-local3    12D3KooWJwLaqsHvVaBkTHLn8Zf5hZdBaoC9pUNtgANymjF3XEmR RUNNING               0
antnode-local4    12D3KooWP1dwBpCQa6mNY62h9LYN5w4gsTqpQfsH1789pvbNVkSQ RUNNING               0
antnode-local5    12D3KooWADWar7uP8pgxahjcgNsvpzVdp2HxtwQoc5ytvgjjFN8r RUNNING               0
antnode-local6    12D3KooWEvPZzdGXPFNGBR5xjt55tSTFJa9ByqLvZAWZ9uYRqYh1 RUNNING               0
antnode-local7    12D3KooWAbLW3UfF9VdeTxtha7TMuMmFyhZGpXi9Anv9toNLQgfv RUNNING               0
antnode-local8    12D3KooWMYhdDsp2eUjGGgqGrStGxyVzoZsui9YQH4N9B6Fh36H3 RUNNING               2
antnode-local9    12D3KooWFMQ9rumJKjayipPgfnCP355oXcD6uzoBFZq985ij1MZP RUNNING               7
antnode-local10   12D3KooWEN8bW2yPfBhJPG9w5xT3zkWGqA9JYY7qkgc1LmuWJshF RUNNING               0
antnode-local11   12D3KooWSUi43YFYQxoRk8iyh7XE3SSeFvLYvANjRjSTS2CAXTwF RUNNING               0
antnode-local12   12D3KooWNhwMVs8jBSwsfM6gD4vhwksVUaP2EMmwReNiibMqPBYT RUNNING               0
antnode-local13   12D3KooWDqgKpbrenxeWyAAw2j45wW7tCpiHYxNnTL7tFioBCTSv RUNNING               1
antnode-local14   12D3KooWAxzJjhxrr2QD4UwkrovVTy5PnjWCFkBPrUJdPVzdNmDP RUNNING               0
antnode-local15   12D3KooWCE3Ccp1GEiXLU8pQdYJued5G6xAiRiarSSgXRhHwG6XJ RUNNING               7
antnode-local16   12D3KooWRC9wjjsnUTEjP8F6pNVu4LacgPMYNP8p3WNeBcgqEGZH RUNNING               0
antnode-local17   12D3KooWKNnLBkDXvdyPV8FALGApnZjtyuxhfzBED4boBQX8gwvD RUNNING               7
antnode-local18   12D3KooWGvMXmnGU3s7g8XZXSExmscXfV8cqHrAQkVKicRxJrx5E RUNNING               1
antnode-local19   12D3KooWHFzdXEiajdSbJRRLnJq56qw2pke9HvneeziuWZB7TTsD RUNNING               2
antnode-local20   12D3KooWMWuuiPwz1mASasxDuT2QpkDFg46RjNiY6FXprFrgFAbT RUNNING               7
antnode-local21   12D3KooWAkgCaCPMBG2gkZJRQJwfM5XYyJ66LmCSidXK6R8x2b7q RUNNING               6
antnode-local22   12D3KooWPep6B7YfsXWdmjDtyNvm8TZ3bvmn9dZ9w9CPtssW2Wtz RUNNING               7
antnode-local23   12D3KooWF486Rjn5DZ7VXcZi99bTabZsWNf73dnnfmpdjusdeEu9 RUNNING               0
antnode-local24   12D3KooWLLWGzyFtB3i1WNrsdu2eW4k3gT7Wewf9D8srgb1dNwcj RUNNING               0
antnode-local25   12D3KooWPpVim2rRHeAYTrM8mSkZjUt5SjQ4v5xPF2h7wi8H1jRj RUNNING               0
faucet             -                                                    RUNNING               -
```

So by default, 25 node processes have been launched, along with a faucet. The faucet dispenses tokens for use when uploading files. We can now run `safe` commands against the local network.

The most common scenario for using a local network is for development, but you can also use it to exercise a lot of features locally. For more details, please see the 'Using a Local Network' section of the [main README](../README.md#using-a-local-network).

Once you've finished, run `antctl local kill` to dispose the local network.

## Running Integration Tests

Sometimes it will be necessary to run the integration tests in a local setup. The problem is, the system-wide tests need root access to run, and they will also create real services, which you don't necessarily want on your development machine.

The tests can be run from a VM, which is provided by a `Vagrantfile` in the `ant_node_manager` crate directory. The machine is defined to use libvirt rather than Virtualbox, so an installation of that is required, but that is beyond the scope of this document.

Assuming that you did have an installation of libvirt, you can get the VM by running `vagrant up`. Once the machine is available, run `vagrant ssh` to get a shell session inside it. For running the tests, switch to the root user using `sudo su -`. As part of the provisioning process, the current `autonomi` code is copied to the root user's home directory. To run the tests:
```
cd autonomi
just node-man-integration-tests
```

The target in the `Justfile` will create a local network and the tests will then run against that.
