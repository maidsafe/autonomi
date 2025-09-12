# Autonomi Node Launchpad

A terminal user interface (TUI) for managing Autonomi network nodes. This tool provides an easy way to set up, monitor, and maintain nodes on the Autonomi decentralized network.

## Features

- **Simple node management**: Start, stop, and monitor multiple nodes from a single interface
- **Resource monitoring**: Track memory usage, bandwidth, and rewards earned by your nodes
- **Configuration options**: Customize connection modes, port settings, and storage locations
- **Wallet integration**: Link your wallet address to collect node rewards

## Installation

Download the latest version from [docs.autonomi.com/node/downloads](https://docs.autonomi.com/node/downloads) or build from source:

```bash
git clone https://github.com/maidsafe/autonomi
cd autonomi
cargo run --release --bin node-launchpad
```

## Requirements

- 35GB of storage space per node
- Stable internet connection
- Windows, macOS, or Linux operating system
- Administrator/root privileges (required for Windows)

## Usage

The usage guides can be found here [docs.autonomi.com/node/guides/how-to-guides](https://docs.autonomi.com/node/guides/how-to-guides)

## Developer Notes

### Connecting to a Custom Network

The launchpad supports connecting to different Autonomi networks. Here is an example on how to spawn nodes using a
pre-built node binary and connect it to a network with a custom network ID.


| Option | Description |
|--------|-------------|
| `--network-id <ID>` | Specify the network ID to connect to. Default is 1 for mainnet |
| `--antnode-path <PATH>` | Path to the pre-built node binary |
| `--network-contacts-url <URL>` | Comma-separated list of URL containing the bootstrap cache. Can be ignored if `--peer` is used |
| `--peer <MULTIADDR>` | Comma-separated list of peer multiaddresses. Can be ignored if `--network-contacts-url` is used |


```bash
./node-launchpad --network-id 2 --antnode-path /path/to/antnode --peer /ip4/1.2.3.4/tcp/12000/p2p/12D3KooWAbCxMV2Zm3Pe4HcAokWDG9w8UMLpDiKpMxwLK3mixpkL
```

## Testing

The node-launchpad includes comprehensive testing for its TUI components using ratatui's testing framework.

### Running Tests

```bash
# Run all tests
cargo test

# Run specific test file
cargo test --test header_test

# Run with verbose output
cargo test -- --nocapture

# Run tests with clippy and formatting
cgc
```

### Test Structure

Tests are organized into:
- **Unit Tests**: Individual component logic in `src/` modules with `#[cfg(test)]`
- **Integration Tests**: Full component rendering and user interactions in `tests/` directory

### Example Test Pattern

```rust
use node_launchpad::components::header::{Header, SelectedMenuItem};
use ratatui::{backend::TestBackend, Terminal, widgets::StatefulWidget};

#[test]
fn test_header_renders_correctly() {
    // Create test backend with specific dimensions
    let backend = TestBackend::new(80, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    
    // Set up component state
    let mut state = SelectedMenuItem::Status;
    
    // Render the component
    terminal.draw(|f| {
        let header = Header::new();
        header.render(f.area(), f.buffer_mut(), &mut state);
    }).unwrap();
    
    // Verify output
    let buffer = terminal.backend().buffer();
    let content = buffer.content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    
    assert!(content.contains("Autonomi Node Launchpad"));
    assert!(content.contains("[S]tatus"));
}
```

### Testing Best Practices

1. **Use TestBackend**: Create deterministic tests with `TestBackend::new(width, height)`
2. **Test All States**: Verify component behavior in different states
3. **Verify Content**: Check both text content and styling/colors
4. **Mock Dependencies**: Use mock implementations for external services
5. **Test Edge Cases**: Handle empty data, overflow, and error conditions

### Working Example

See `tests/header_test.rs` for a complete working example that tests:
- Basic component rendering
- State-dependent menu highlighting
- Version display formatting
- Content verification

This testing approach ensures the TUI interface is reliable and maintains visual consistency across different scenarios.
