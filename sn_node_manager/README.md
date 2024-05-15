# Safenode Manager

Safenode Manager is a command-line application for installing, managing, and operating `safenode` as a service.

It runs on Linux, macOS and Windows.

## Installation

The latest version can be installed via [safeup](https://github.com/maidsafe/safeup):
```
safeup node-manager
```

A binary can also be obtained for your platform from the releases in this repository.

## Nodes as Services

The primary use case for Safenode Manager is to setup `safenode` as a long-running background service, using the service infrastructure provided by the operating system.

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

Windows does not support user-mode services at all, and therefore, the node manager must always be
used in an elevated, administrative session.

### Create Services

First, use the `add` command to create some services:
```
$ safenode-manager add --count 3 --peer /ip4/46.101.80.187/udp/58070/quic-v1/p2p/12D3KooWKgJQedzCxrp33u3dBD1mUZ9HTjEjgrxskEBvzoQWkRT9
```

This downloads the latest version of the `safenode` binary and creates three services that will initially connect to the specified peer. Soon, specification of a peer will not be required.

There are many arguments available for customising the service. For example, you can choose the port the node will run on, or the version of `safenode`. Run `safenode-manager add --help` to see all available options.

_Note_: elevated privileges are required for creating services, on all platforms.

Now run the `status` command:
```
$ safenode-manager status
=================================================
                Safenode Services
=================================================
Refreshing the node registry...
Service Name       Peer ID                                              Status  Connected Peers
safenode1          -                                                    ADDED               -
safenode2          -                                                    ADDED               -
safenode3          -                                                    ADDED               -
```

We can see the services have been added, but they are not running yet.

### Start Services

Use the `start` command to start each service:
```
$ safenode-manager start
```

Providing no arguments will start all available services. If need be, it's possible to start services individually, using the `--service-name` argument.

With the services started, run the `status` command again:
```
$ safenode-manager status
=================================================
                Safenode Services
=================================================
Refreshing the node registry...
Service Name       Peer ID                                              Status  Connected Peers
safenode1          12D3KooWGQu92xCXuiK6AysbHn6kHyfXqyzNDxNGnnDTgd56eveq RUNNING              81
safenode2          12D3KooWQGVfcwrPFvC6PyCva1cJu8NZVhdZCuPHJ4vY79yuKC3A RUNNING              82
safenode3          12D3KooWMqRH6EF1Km61TAW9wTuv9LgDabKMY9DJSGyrxUafXP6b RUNNING              79
```

We can see our services are running and the nodes have connections to other peers.

Now, run the `status` command again, but with the `--details` flag:
```
=================================================
                Safenode Services
=================================================
Refreshing the node registry...
============================
safenode1 - RUNNING
============================
Version: 0.105.0
Peer ID: 12D3KooWGQu92xCXuiK6AysbHn6kHyfXqyzNDxNGnnDTgd56eveq
RPC Socket: 127.0.0.1:41785
Listen Addresses: Some["/ip4/127.0.0.1/udp/34653/quic-v1/p2p/12D3KooWGQu92xCXuiK6AysbHn6kHyfXqyzNDxNGnnDTgd56eveq", "/ip4/192.168.121.7/udp/34653/quic-v1/p2p/12D3KooWGQu92xCXuiK6AysbHn6kHyfXqyzNDxNGnnDTgd56eveq"]
PID: 3137
Data path: /var/safenode-manager/services/safenode1
Log path: /var/log/safenode/safenode1
Bin path: /var/safenode-manager/services/safenode1/safenode
Connected peers: 10
<remaining output snipped>
```

We get some more details for each node, including the path to its logs. Using the location provided, feel free to take a look at the logs being generated by the node.

The nodes could now be left running like this, but for the purposes of this guide, we will do some more things.

### Add More Nodes

It's possible to run the `add` command again, as before:
```
safenode-manager add --count 3 --peer /ip4/46.101.80.187/udp/58070/quic-v1/p2p/12D3KooWKgJQedzCxrp33u3dBD1mUZ9HTjEjgrxskEBvzoQWkRT9
```

The subsequent `status` command will show us an additional three nodes, for a total of six:
```
$ safenode-manager status
=================================================
                Safenode Services
=================================================
Refreshing the node registry...
Service Name       Peer ID                                              Status  Connected Peers
safenode1          12D3KooWGQu92xCXuiK6AysbHn6kHyfXqyzNDxNGnnDTgd56eveq RUNNING               4
safenode2          12D3KooWQGVfcwrPFvC6PyCva1cJu8NZVhdZCuPHJ4vY79yuKC3A RUNNING               4
safenode3          12D3KooWMqRH6EF1Km61TAW9wTuv9LgDabKMY9DJSGyrxUafXP6b RUNNING               3
safenode4          -                                                    ADDED               -
safenode5          -                                                    ADDED               -
safenode6          -                                                    ADDED               -
```

Again, the new nodes have not been started.

Run the `start` command to start them, then observe the status:
```
$ safenode-manager status
=================================================
                Safenode Services
=================================================
Refreshing the node registry...
Service Name       Peer ID                                              Status  Connected Peers
safenode1          12D3KooWGQu92xCXuiK6AysbHn6kHyfXqyzNDxNGnnDTgd56eveq RUNNING             138
safenode2          12D3KooWQGVfcwrPFvC6PyCva1cJu8NZVhdZCuPHJ4vY79yuKC3A RUNNING             177
safenode3          12D3KooWMqRH6EF1Km61TAW9wTuv9LgDabKMY9DJSGyrxUafXP6b RUNNING             144
safenode4          12D3KooWLH9VRAoUMj4bUjtzcKS3mqfzyc46TxBkBzvUXfV1bjaT RUNNING               2
safenode5          12D3KooWEcbpvSSTmSyuzqP3gE9bE7uqYFatHhkJXr8PBiqmESEG RUNNING               1
safenode6          12D3KooWBip2g5FakT1dZHdrhdmnctgKqhbRBQA5ZpvtHh4XPRXJ RUNNING              30
```

### Removing Nodes

If for some reason we want to remove one of our nodes, we can do so using the `remove` command.

Suppose we wanted to remove the 5th service. First of all, we need to stop the service. Run the following command:
```
$ safenode-manager stop --service-name safenode5
```

Observe that `safenode5` has been stopped, but the others are still running:
```
$ safenode-manager status
=================================================
                Safenode Services
=================================================
Refreshing the node registry...
Service Name       Peer ID                                              Status  Connected Peers
safenode1          12D3KooWGQu92xCXuiK6AysbHn6kHyfXqyzNDxNGnnDTgd56eveq RUNNING              10
safenode2          12D3KooWQGVfcwrPFvC6PyCva1cJu8NZVhdZCuPHJ4vY79yuKC3A RUNNING               5
safenode3          12D3KooWMqRH6EF1Km61TAW9wTuv9LgDabKMY9DJSGyrxUafXP6b RUNNING               2
safenode4          12D3KooWLH9VRAoUMj4bUjtzcKS3mqfzyc46TxBkBzvUXfV1bjaT RUNNING               2
safenode5          12D3KooWEcbpvSSTmSyuzqP3gE9bE7uqYFatHhkJXr8PBiqmESEG STOPPED               -
safenode6          12D3KooWBip2g5FakT1dZHdrhdmnctgKqhbRBQA5ZpvtHh4XPRXJ RUNNING              29
```

Now that it's been stopped, remove it:
```
$ safenode-manager remove --service-name safenode5
```

The `status` command will no longer show the service:
```
vagrant@ubuntu2204:~$ safenode-manager status
=================================================
                Safenode Services
=================================================
Refreshing the node registry...
Service Name       Peer ID                                              Status  Connected Peers
safenode1          12D3KooWGQu92xCXuiK6AysbHn6kHyfXqyzNDxNGnnDTgd56eveq RUNNING               2
safenode2          12D3KooWQGVfcwrPFvC6PyCva1cJu8NZVhdZCuPHJ4vY79yuKC3A RUNNING              96
safenode3          12D3KooWMqRH6EF1Km61TAW9wTuv9LgDabKMY9DJSGyrxUafXP6b RUNNING             127
safenode4          12D3KooWLH9VRAoUMj4bUjtzcKS3mqfzyc46TxBkBzvUXfV1bjaT RUNNING              76
safenode6          12D3KooWBip2g5FakT1dZHdrhdmnctgKqhbRBQA5ZpvtHh4XPRXJ RUNNING             133
```

However, we will still see it in the detailed view:
```
$ safenode-manager status --details
=================================================
                Safenode Services
=================================================
Refreshing the node registry...
<output snipped>
============================
safenode5 - REMOVED
============================
Version: 0.105.0
Peer ID: 12D3KooWEcbpvSSTmSyuzqP3gE9bE7uqYFatHhkJXr8PBiqmESEG
RPC Socket: 127.0.0.1:38579
Listen Addresses: Some(["/ip4/127.0.0.1/udp/58354/quic-v1/p2p/12D3KooWEcbpvSSTmSyuzqP3gE9bE7uqYFatHhkJXr8PBiqmESEG", "/ip4/192.168.121.7/udp/58354/quic-v1/p2p/12D3KooWEcbpvSSTmSyuzqP3gE9bE7uqYFatHhkJXr8PBiqmESEG"])
PID: -
Data path: /var/safenode-manager/services/safenode5
Log path: /var/log/safenode/safenode5
Bin path: /var/safenode-manager/services/safenode5/safenode
Connected peers: -
<output snipped>
```

## Upgrades

The node manager can be used to continually upgrade node services.

Suppose we have five services:
```
$ safenode-manager status
=================================================
                Safenode Services
=================================================
Refreshing the node registry...
Service Name       Peer ID                                              Status  Connected Peers
safenode1          12D3KooWKaNFPoRf8E2vsdSwBNyhWpe7csNkqwXaunNVxXGarxap RUNNING               1
safenode2          12D3KooWNbRBR43rdFR44EAbwzBrED3jWUiWKkHD2oabw2jKZ9eF RUNNING               1
safenode3          12D3KooWGYEuqXRhKVF2WK499oCBpkh9c6K7jy8BNSfqGosSzNZ8 RUNNING               1
safenode4          12D3KooWS6WGnhbSfLywaepfZbbqgxTLhr66N1PXU4GVLWDBRZRF RUNNING               1
safenode5          12D3KooWNdEYQAutzcGo26rZayew2rzE24y5VFVxTnsefNevc1Ly RUNNING               1
```

Using the `--details` flag, we can see they are not at the latest version:
```
$ safenode-manager status --details
=================================================
                Safenode Services
=================================================
Refreshing the node registry...
============================
safenode1 - RUNNING
============================
Version: 0.104.38
Peer ID: 12D3KooWKaNFPoRf8E2vsdSwBNyhWpe7csNkqwXaunNVxXGarxap
RPC Socket: 127.0.0.1:37931
Listen Addresses: Some(["/ip4/127.0.0.1/udp/39890/quic-v1/p2p/12D3KooWKaNFPoRf8E2vsdSwBNyhWpe7csNkqwXaunNVxXGarxap", "/ip4/192.168.121.114/udp/39890/quic-v1/p2p/12D3KooWKaNFPoRf8E2vsdSwBNyhWpe7csNkqwXaunNVxXGarxap"])
PID: 3285
Data path: /var/safenode-manager/services/safenode1
Log path: /var/log/safenode/safenode1
Bin path: /var/safenode-manager/services/safenode1/safenode
Connected peers: 0
<remaining output snipped>
```

For brevity, the remaining output is snipped, but the four others are also at `0.104.38`. At the time of writing, the latest version is `0.105.3`.

We can use the `upgrade` command to get each service on the latest version:
```
$ safenode-manager upgrade
=================================================
           Upgrade Safenode Services
=================================================
Retrieving latest version of safenode...
Latest version is 0.105.3
Downloading safenode version 0.105.3...
Download completed: /tmp/ae310e50-d104-45bc-9619-22e1328d8c8b/safenode
Refreshing the node registry...
<output snipped>
Upgrade summary:
✓ safenode1 upgraded from 0.104.38 to 0.105.3
✓ safenode2 upgraded from 0.104.38 to 0.105.3
✓ safenode3 upgraded from 0.104.38 to 0.105.3
✓ safenode4 upgraded from 0.104.38 to 0.105.3
✓ safenode5 upgraded from 0.104.38 to 0.105.3
```

Again, for brevity some output from the command was snipped, but the summary indicates that each service was upgraded from `0.104.38` to `0.105.3`.

As with other commands, if no arguments are supplied, `upgrade` operates over all services, but it's possible to use the `--service-name` or `--peer-id` arguments to upgrade specific services. Both those arguments can be used multiple times to operate over several services.

The node manager will determine the latest version of `safenode`, download it, then for each running service, if the service is older than the latest, it will stop it, copy the new binary over the old one, and start the service again.

### Downgrading

In some situations, it may be necessary to downgrade `safenode` to a previous version. The `upgrade` command supports this by providing `--version` and `--force` arguments. Each of those can be used to force the node manager to accept a lower version.

## Local Networks

Safenode Manager can also create local networks, which are useful for development or quick experimentation. In a local network, nodes will run as processes rather than services. Local operations are defined under the `local` subcommand.

To create a local network, use the `run` command:
```
$ safenode-manager local run
=================================================
             Launching Local Network
=================================================
Retrieving latest version for faucet...
Downloading faucet version 0.4.3...
Download completed: /tmp/4dc310dd-74ef-4dc5-af36-3bc92a882db1/faucet
Retrieving latest version for safenode...
Downloading safenode version 0.105.3...
Download completed: /tmp/f63d3ca8-2b8e-4630-9df5-a13418d5f826/safenode
Launching node 1...
Logging to directory: "/home/chris/.local/share/safe/node/12D3KooWPArH2XAw2sapcthNNcJRbbSuUtC3eBZrJtxi8DfcN1Yn/logs"

Node started

<remaining output snipped>
```

_Note_: elevated privileges are not required for local networks.

Check the output of the `status` command:
```
$ safenode-manager status
=================================================
                Local Network
=================================================
Refreshing the node registry...
Service Name       Peer ID                                              Status  Connected Peers
safenode-local1    12D3KooWPArH2XAw2sapcthNNcJRbbSuUtC3eBZrJtxi8DfcN1Yn RUNNING               7
safenode-local2    12D3KooWShWom22VhgkDX7APqSzCmXPNsfZA17Y2GSJpznunAp8M RUNNING               0
safenode-local3    12D3KooWJwLaqsHvVaBkTHLn8Zf5hZdBaoC9pUNtgANymjF3XEmR RUNNING               0
safenode-local4    12D3KooWP1dwBpCQa6mNY62h9LYN5w4gsTqpQfsH1789pvbNVkSQ RUNNING               0
safenode-local5    12D3KooWADWar7uP8pgxahjcgNsvpzVdp2HxtwQoc5ytvgjjFN8r RUNNING               0
safenode-local6    12D3KooWEvPZzdGXPFNGBR5xjt55tSTFJa9ByqLvZAWZ9uYRqYh1 RUNNING               0
safenode-local7    12D3KooWAbLW3UfF9VdeTxtha7TMuMmFyhZGpXi9Anv9toNLQgfv RUNNING               0
safenode-local8    12D3KooWMYhdDsp2eUjGGgqGrStGxyVzoZsui9YQH4N9B6Fh36H3 RUNNING               2
safenode-local9    12D3KooWFMQ9rumJKjayipPgfnCP355oXcD6uzoBFZq985ij1MZP RUNNING               7
safenode-local10   12D3KooWEN8bW2yPfBhJPG9w5xT3zkWGqA9JYY7qkgc1LmuWJshF RUNNING               0
safenode-local11   12D3KooWSUi43YFYQxoRk8iyh7XE3SSeFvLYvANjRjSTS2CAXTwF RUNNING               0
safenode-local12   12D3KooWNhwMVs8jBSwsfM6gD4vhwksVUaP2EMmwReNiibMqPBYT RUNNING               0
safenode-local13   12D3KooWDqgKpbrenxeWyAAw2j45wW7tCpiHYxNnTL7tFioBCTSv RUNNING               1
safenode-local14   12D3KooWAxzJjhxrr2QD4UwkrovVTy5PnjWCFkBPrUJdPVzdNmDP RUNNING               0
safenode-local15   12D3KooWCE3Ccp1GEiXLU8pQdYJued5G6xAiRiarSSgXRhHwG6XJ RUNNING               7
safenode-local16   12D3KooWRC9wjjsnUTEjP8F6pNVu4LacgPMYNP8p3WNeBcgqEGZH RUNNING               0
safenode-local17   12D3KooWKNnLBkDXvdyPV8FALGApnZjtyuxhfzBED4boBQX8gwvD RUNNING               7
safenode-local18   12D3KooWGvMXmnGU3s7g8XZXSExmscXfV8cqHrAQkVKicRxJrx5E RUNNING               1
safenode-local19   12D3KooWHFzdXEiajdSbJRRLnJq56qw2pke9HvneeziuWZB7TTsD RUNNING               2
safenode-local20   12D3KooWMWuuiPwz1mASasxDuT2QpkDFg46RjNiY6FXprFrgFAbT RUNNING               7
safenode-local21   12D3KooWAkgCaCPMBG2gkZJRQJwfM5XYyJ66LmCSidXK6R8x2b7q RUNNING               6
safenode-local22   12D3KooWPep6B7YfsXWdmjDtyNvm8TZ3bvmn9dZ9w9CPtssW2Wtz RUNNING               7
safenode-local23   12D3KooWF486Rjn5DZ7VXcZi99bTabZsWNf73dnnfmpdjusdeEu9 RUNNING               0
safenode-local24   12D3KooWLLWGzyFtB3i1WNrsdu2eW4k3gT7Wewf9D8srgb1dNwcj RUNNING               0
safenode-local25   12D3KooWPpVim2rRHeAYTrM8mSkZjUt5SjQ4v5xPF2h7wi8H1jRj RUNNING               0
faucet             -                                                    RUNNING               -
```

So by default, 25 node processes have been launched, along with a faucet. The faucet dispenses tokens for use when uploading files. We can now run `safe` commands against the local network.

The most common scenario for using a local network is for development, but you can also use it to exercise a lot of features locally. For more details, please see the 'Using a Local Network' section of the [main README](https://github.com/maidsafe/safe_network/tree/node-man-readme?tab=readme-ov-file#using-a-local-network).

Once you've finished, run `safenode-manager local kill` to dispose the local network.
