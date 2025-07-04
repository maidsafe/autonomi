# Bootstrap Cache Migration Addendum

## Critical Requirement: Maintain Bootstrap Cache Throughout Migration

The bootstrap cache is vital for network connectivity and peer discovery. This addendum provides specific guidance for preserving and adapting the bootstrap cache system during the libp2p to iroh migration.

## Current Bootstrap Cache Architecture

The bootstrap cache (`ant-bootstrap`) provides:
- Persistent peer storage with success/failure tracking
- Atomic file operations for concurrent access
- Network version tracking
- Automatic cleanup of old/unreliable peers
- Initial peer discovery from web endpoints

## Key Challenges

1. **Address Format Changes**: libp2p uses Multiaddr with PeerId, iroh uses NodeId with direct addresses
2. **Cache File Compatibility**: Need to maintain cache during transition
3. **Peer Identity Migration**: Mapping between libp2p PeerId and iroh NodeId
4. **Version Compatibility**: Ensuring nodes can discover peers across versions

## Migration Strategy for Bootstrap Cache

### Phase 1 Modifications: Dual Identity Support

Update `ant-bootstrap/src/lib.rs`:
```rust
use iroh::net::NodeId;
use serde::{Deserialize, Serialize};

/// Extended BootstrapAddr that supports both libp2p and iroh
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapAddr {
    /// The multiaddress of the peer (libp2p format)
    pub addr: Multiaddr,
    /// Optional iroh node address
    pub iroh_addr: Option<IrohNodeAddr>,
    /// The number of successful connections to this address
    pub success_count: u32,
    /// The number of failed connection attempts to this address
    pub failure_count: u32,
    /// The last time this address was successfully contacted
    pub last_seen: SystemTime,
    /// Which transport was last successful
    pub last_successful_transport: Option<TransportType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrohNodeAddr {
    pub node_id: String, // Serialized NodeId
    pub direct_addresses: Vec<String>,
    pub relay_url: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TransportType {
    LibP2p,
    Iroh,
}
```

### Phase 2 Modifications: Cache Store Updates

Update `ant-bootstrap/src/cache_store.rs`:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheData {
    pub peers: HashMap<PeerId, BootstrapAddresses>,
    /// New: Mapping between PeerId and NodeId
    pub peer_id_mapping: HashMap<PeerId, NodeId>,
    pub last_updated: SystemTime,
    pub network_version: String,
    /// Track migration phase
    pub migration_phase: MigrationPhase,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MigrationPhase {
    LibP2pOnly,
    DualStack,
    IrohOnly,
}

impl CacheData {
    /// Add iroh address information to existing peer
    pub fn add_iroh_addr(&mut self, peer_id: PeerId, node_id: NodeId, iroh_addr: IrohNodeAddr) {
        // Store mapping
        self.peer_id_mapping.insert(peer_id, node_id);
        
        // Update existing bootstrap addresses
        if let Some(bootstrap_addresses) = self.peers.get_mut(&peer_id) {
            for addr in &mut bootstrap_addresses.0 {
                addr.iroh_addr = Some(iroh_addr.clone());
            }
        }
    }
    
    /// Convert cache for iroh-only operation
    pub fn to_iroh_format(&self) -> IrohCacheData {
        let mut iroh_peers = HashMap::new();
        
        for (peer_id, addresses) in &self.peers {
            if let Some(node_id) = self.peer_id_mapping.get(peer_id) {
                let iroh_addrs = addresses.0.iter()
                    .filter_map(|addr| addr.iroh_addr.as_ref())
                    .cloned()
                    .collect();
                    
                iroh_peers.insert(*node_id, iroh_addrs);
            }
        }
        
        IrohCacheData {
            peers: iroh_peers,
            last_updated: self.last_updated,
            network_version: self.network_version.clone(),
        }
    }
}
```

### Phase 3 Modifications: Dual-Stack Cache Operations

Create `ant-bootstrap/src/dual_stack.rs`:
```rust
/// Bootstrap cache that works with both transports
pub struct DualStackBootstrapCache {
    store: BootstrapCacheStore,
    preferred_transport: TransportType,
}

impl DualStackBootstrapCache {
    /// Get bootstrap addresses with transport preference
    pub fn get_bootstrap_addrs(&self) -> Vec<ContactInfo> {
        let mut contacts = Vec::new();
        
        for addr in self.store.get_all_addrs() {
            let contact = match (self.preferred_transport, &addr.iroh_addr, &addr.last_successful_transport) {
                // Prefer iroh if available and configured
                (TransportType::Iroh, Some(iroh_addr), _) => {
                    ContactInfo::Iroh(iroh_addr.clone())
                }
                // Fall back based on last successful transport
                (_, Some(iroh_addr), Some(TransportType::Iroh)) => {
                    ContactInfo::Iroh(iroh_addr.clone())
                }
                // Default to libp2p
                _ => ContactInfo::LibP2p(addr.addr.clone()),
            };
            
            contacts.push(contact);
        }
        
        // Sort by reliability
        contacts.sort_by_key(|c| match c {
            ContactInfo::LibP2p(addr) => self.get_failure_rate_for_addr(addr),
            ContactInfo::Iroh(addr) => self.get_failure_rate_for_iroh(addr),
        });
        
        contacts
    }
    
    /// Update cache based on connection result
    pub fn update_connection_result(
        &mut self,
        contact: &ContactInfo,
        success: bool,
        transport: TransportType,
    ) {
        match contact {
            ContactInfo::LibP2p(addr) => {
                self.store.update_addr_status(addr, success);
                if success {
                    self.set_last_successful_transport(addr, transport);
                }
            }
            ContactInfo::Iroh(node_addr) => {
                self.update_iroh_status(node_addr, success);
            }
        }
    }
}
```

### Phase 4 Modifications: Migration Tools

Create `ant-bootstrap/src/migration.rs`:
```rust
/// Tool to migrate bootstrap cache between formats
pub struct BootstrapCacheMigrator {
    old_cache_path: PathBuf,
    new_cache_path: PathBuf,
}

impl BootstrapCacheMigrator {
    /// Migrate from libp2p-only to dual-stack format
    pub fn migrate_to_dual_stack(&self) -> Result<()> {
        let old_data = CacheData::load_from_file(&self.old_cache_path)?;
        let mut new_data = old_data.clone();
        
        new_data.migration_phase = MigrationPhase::DualStack;
        
        // Initialize peer_id_mapping if not present
        if new_data.peer_id_mapping.is_empty() {
            warn!("No peer ID mappings found, will be populated as nodes connect");
        }
        
        new_data.save_to_file(&self.new_cache_path)?;
        info!("Successfully migrated cache to dual-stack format");
        
        Ok(())
    }
    
    /// Migrate from dual-stack to iroh-only format
    pub fn migrate_to_iroh_only(&self) -> Result<()> {
        let dual_stack_data = CacheData::load_from_file(&self.old_cache_path)?;
        
        // Verify we have enough iroh addresses
        let iroh_addr_count = dual_stack_data.peers.values()
            .flat_map(|addrs| &addrs.0)
            .filter(|addr| addr.iroh_addr.is_some())
            .count();
            
        if iroh_addr_count < 10 {
            return Err(Error::InsufficientIrohAddresses);
        }
        
        let iroh_data = dual_stack_data.to_iroh_format();
        iroh_data.save_to_file(&self.new_cache_path)?;
        
        info!("Successfully migrated cache to iroh-only format");
        Ok(())
    }
    
    /// Create backup before migration
    pub fn backup_cache(&self) -> Result<PathBuf> {
        let backup_path = self.old_cache_path.with_extension("bak");
        fs::copy(&self.old_cache_path, &backup_path)?;
        info!("Created cache backup at {:?}", backup_path);
        Ok(backup_path)
    }
}
```

### Phase 5 Modifications: Final iroh-only Cache

Create new cache structure for iroh:
```rust
/// Bootstrap cache for iroh-only network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrohBootstrapCache {
    /// NodeId -> Addresses mapping
    pub nodes: HashMap<NodeId, IrohNodeInfo>,
    pub last_updated: SystemTime,
    pub network_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrohNodeInfo {
    pub direct_addresses: Vec<SocketAddr>,
    pub relay_url: Option<String>,
    pub success_count: u32,
    pub failure_count: u32,
    pub last_seen: SystemTime,
    pub supports_kademlia: bool,
}

impl IrohBootstrapCache {
    /// Get bootstrap nodes for initial connection
    pub fn get_bootstrap_nodes(&self, count: usize) -> Vec<NodeAddr> {
        let mut nodes: Vec<_> = self.nodes.iter()
            .filter(|(_, info)| info.supports_kademlia)
            .filter(|(_, info)| info.is_reliable())
            .map(|(node_id, info)| {
                NodeAddr {
                    node_id: *node_id,
                    direct_addresses: info.direct_addresses.clone().into_iter().collect(),
                    relay_url: info.relay_url.as_ref().map(|s| s.parse().ok()).flatten(),
                }
            })
            .collect();
            
        // Sort by reliability
        nodes.sort_by_key(|n| {
            self.nodes.get(&n.node_id)
                .map(|info| info.failure_rate() as u64)
                .unwrap_or(u64::MAX)
        });
        
        nodes.into_iter().take(count).collect()
    }
}
```

## Critical Implementation Details

### 1. Cache File Locations

Maintain separate cache files during migration:
```rust
pub fn get_cache_paths(phase: MigrationPhase) -> (PathBuf, Option<PathBuf>) {
    let base_dir = get_cache_dir();
    
    match phase {
        MigrationPhase::LibP2pOnly => {
            (base_dir.join("bootstrap_cache.json"), None)
        }
        MigrationPhase::DualStack => {
            (
                base_dir.join("bootstrap_cache_dual.json"),
                Some(base_dir.join("bootstrap_cache.json")), // Keep old cache as fallback
            )
        }
        MigrationPhase::IrohOnly => {
            (base_dir.join("bootstrap_cache_iroh.json"), None)
        }
    }
}
```

### 2. Network Version Handling

Update version string to indicate transport:
```rust
pub fn get_network_version() -> String {
    let base_version = format!("{}_{}", get_network_id_str(), get_truncate_version_str());
    
    match current_phase() {
        MigrationPhase::LibP2pOnly => base_version,
        MigrationPhase::DualStack => format!("{}_dual", base_version),
        MigrationPhase::IrohOnly => format!("{}_iroh", base_version),
    }
}
```

### 3. Initial Peers Handling

Update contacts fetcher to support both formats:
```rust
impl ContactsFetcher {
    pub async fn fetch_contacts(&self) -> Result<Vec<ContactInfo>> {
        let response = self.fetch_from_endpoints().await?;
        
        // Parse based on format version
        if response.contains("iroh_addr") {
            self.parse_dual_stack_contacts(&response)
        } else {
            self.parse_libp2p_contacts(&response)
        }
    }
}
```

### 4. Backwards Compatibility

Ensure nodes can read old cache formats:
```rust
pub fn load_cache_with_migration(path: &Path) -> Result<CacheData> {
    let contents = fs::read_to_string(path)?;
    
    // Try current format first
    if let Ok(data) = serde_json::from_str::<CacheData>(&contents) {
        return Ok(data);
    }
    
    // Try old format and migrate
    if let Ok(old_data) = serde_json::from_str::<OldCacheData>(&contents) {
        info!("Migrating cache from old format");
        return Ok(migrate_old_format(old_data));
    }
    
    Err(Error::InvalidCacheFormat)
}
```

## Testing Requirements

1. **Cache Migration Tests**:
   - Test upgrading from libp2p-only to dual-stack cache
   - Test upgrading from dual-stack to iroh-only cache
   - Verify no peer loss during migration

2. **Compatibility Tests**:
   - Old nodes can read new cache format (with libp2p addresses)
   - New nodes can read old cache format
   - Dual-stack nodes can populate both address types

3. **Reliability Tests**:
   - Cache continues to track success/failure correctly
   - Cleanup still works with dual addresses
   - Atomic writes work with larger cache format

## Rollback Procedures

If issues arise, maintain ability to rollback:

1. Keep backup of original cache before each migration phase
2. Maintain separate cache files for each phase
3. Implement cache downgrade tools
4. Test rollback procedures regularly

## Monitoring

Add metrics for bootstrap cache health:
- Cache hit rate for each transport
- Number of peers with dual-stack support
- Migration progress (percentage of peers with iroh addresses)
- Cache file size and cleanup frequency

This ensures the bootstrap cache remains functional throughout the migration while supporting the gradual transition from libp2p to iroh.
