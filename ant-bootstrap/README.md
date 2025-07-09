# Bootstrap Cache

A simple peer caching system for the Autonomi Network that provides persistent storage and management of network peer addresses. This crate handles peer discovery and FIFO caching with support for concurrent access across multiple processes.

## Features

### Storage and Accessibility
- System-wide accessible cache location
- Configurable primary cache location
- Cross-process safe with file locking
- Atomic write operations to prevent cache corruption

### Data Management
- Automatic cleanup of stale and unreliable peers
- Configurable maximum peer limit
- Atomic file operations for data integrity

## License

This Autonomi Network software is licensed under the General Public License (GPL), version 3 ([LICENSE](http://www.gnu.org/licenses/gpl-3.0.en.html)).
