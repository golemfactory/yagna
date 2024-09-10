# Golem CLI Overview

The Golem Command Line Interface (CLI) provides a user-friendly way to interact with the Yagna daemon and manage Golem network operations. It simplifies the process of running a Golem provider node and performing various tasks related to the Golem network.

## Key Features

1. **Provider Management**: Easily start, stop, and configure Golem provider nodes.
2. **Task Management**: Submit, monitor, and manage compute tasks on the Golem network.
3. **Network Diagnostics**: Run network tests and diagnostics to ensure optimal performance.
4. **Wallet Operations**: Manage payments, check balances, and perform transactions.
5. **Configuration**: Set up and modify Yagna and provider settings through a command-line interface.

## Basic Usage

To use the Golem CLI, you typically start by running the Yagna daemon:

\```bash
yagna service run
\```

Then, you can use various commands to interact with the Golem network. For example:

\```bash
# Start a provider node
golemsp run

# Check your node's status
yagna node status

# List active agreements
yagna agreement list
\```

## Components

The Golem CLI consists of two main components:

1. **yagna**: The core CLI for interacting with the Yagna daemon.
2. **golemsp**: A specialized CLI for managing Golem provider nodes.

Each of these components offers a range of subcommands for different operations.

For detailed information on available commands and their usage, please refer to the specific command documentation or use the `--help` flag with any command.

The Golem CLI is an essential tool for both providers and requestors in the Golem network, offering a straightforward way to participate in and benefit from decentralized computing.