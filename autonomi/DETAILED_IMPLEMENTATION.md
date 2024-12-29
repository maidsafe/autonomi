# Detailed Implementation Plan

## Pre-Implementation Analysis

### Current Files Structure

```
autonomi/
├── src/
│   ├── client/
│   │   ├── mod.rs         # Main client implementation
│   │   ├── address.rs     # Network addressing
│   │   ├── payment.rs     # Payment functionality
│   │   ├── quote.rs       # Quoting system
│   │   ├── data.rs        # Data operations
│   │   ├── files.rs       # File handling
│   │   └── ...
├── tests/
└── examples/
```

### Required Changes

1. **Client Module (`src/client/mod.rs`)**
   - Remove direct network handling from public API
   - Add local network support with automatic MDNS discovery
   - Simplify client initialization
   - Add streaming file operations
   - Ensure proper integration with local ant-node
   - Enable MDNS automatically when local mode is selected

2. **Network Layer**
   - Move network complexity behind abstraction
   - Enable MDNS automatically for local testing
   - Implement bootstrap cache properly
   - Use local ant-node with --local flag for testing
   - Configure MDNS with faster discovery for local mode

3. **Data Operations**
   - Implement streaming file operations
   - Use self_encryption for chunking
   - Add proper error handling

## Day 1 Morning: Core Implementation

### Hour 0-1: Project Setup and Analysis

1. **Dependencies Review**

   ```toml
   [dependencies]
   tokio = { version = "1.0", features = ["full"] }
   libp2p = "0.54"
   self_encryption = "0.31"
   ant-bootstrap = { path = "../ant-bootstrap" }
   ant-networking = { path = "../ant-networking" }
   ant-node = { path = "../ant-node" }  # Local ant-node crate
   ```

2. **Initial Test Setup**

   ```rust
   // tests/common/mod.rs
   pub async fn setup_local_network(node_count: usize) -> Result<(Client, LocalNetwork)> {
       // Ensure we're using the local ant-node crate
       let network = LocalNetwork::new_with_local_nodes(node_count).await?;
       let client = Client::new_local().await?;
       Ok((client, network))
   }
   ```

### Hour 1-2: Network Layer Implementation

1. **Local Network Support**

   ```rust
   // src/network/local.rs
   pub struct LocalNetwork {
       nodes: Vec<LocalNode>,
       temp_dir: TempDir,  // Store node data
   }

   impl LocalNetwork {
       pub async fn new_with_local_nodes(node_count: usize) -> Result<Self> {
           let temp_dir = tempfile::tempdir()?;
           let mut nodes = Vec::with_capacity(node_count);
           
           // Start first node with --local flag
           let first = LocalNode::start_local(temp_dir.path(), None).await?;
           nodes.push(first);
           
           // Start additional nodes, all with --local flag
           for i in 1..node_count {
               let node = LocalNode::start_local(
                   temp_dir.path(),
                   Some(nodes[0].multiaddr())
               ).await?;
               nodes.push(node);
           }
           
           Ok(Self { nodes, temp_dir })
       }
   }
   ```

2. **Node Management**

   ```rust
   // src/network/node.rs
   pub struct LocalNode {
       process: Child,
       rpc_port: u16,
       peer_id: PeerId,
       multiaddr: Multiaddr,
   }

   impl LocalNode {
       pub async fn start_local(
           data_dir: &Path,
           bootstrap: Option<Multiaddr>
       ) -> Result<Self, NodeError> {
           // Find available port
           let rpc_port = get_available_port()?;
           
           // Start ant-node with local flag which enables MDNS discovery
           let process = Command::new("ant-node")
               .arg("--local")  // This enables MDNS for local discovery
               .arg("--rpc-port")
               .arg(rpc_port.to_string())
               .arg("--log-level")
               .arg("debug")  // Helpful for seeing MDNS activity
               .spawn()?;
           
           // Wait for node to start and get peer info
           let peer_info = wait_for_node_ready(rpc_port).await?;
           
           Ok(Self {
               process,
               rpc_port,
               peer_id: peer_info.peer_id,
               multiaddr: peer_info.multiaddr,
           })
       }

       pub fn is_local(&self) -> bool {
           true  // All nodes started with --local flag
       }
   }
   ```

3. **Quick Test**

   ```rust
   #[tokio::test]
   async fn test_local_node_startup() {
       let temp_dir = tempfile::tempdir().unwrap();
       let node = LocalNode::start_local(temp_dir.path(), None).await.unwrap();
       assert!(node.is_running());
       assert!(node.is_local());
   }
   ```

### Hour 2-4: Core Client & Data Operations

1. **Client Implementation**

   ```rust
   // src/client/mod.rs
   impl Client {
       pub async fn new_local() -> Result<Self> {
           let config = ClientConfig {
               network_type: NetworkType::Local,  // This enables MDNS in client
               ..Default::default()
           };
           Self::new(config).await
       }
       
       pub async fn store_file(&self, path: PathBuf) -> Result<FileMap> {
           let store = |name, data| self.network.store_chunk(name, data);
           streaming_encrypt_from_file(&path, store)
       }
       
       pub async fn get_file(&self, map: FileMap, output: PathBuf) -> Result<()> {
           let get = |name| self.network.get_chunk(name);
           streaming_decrypt_from_storage(&map.inner, &output, get)
       }
   }
   ```

2. **Quick Test**

   ```rust
   #[tokio::test]
   async fn test_file_operations() {
       let (client, _network) = setup_local_network(3).await?;
       
       // Create test file
       let mut temp_file = NamedTempFile::new()?;
       temp_file.write_all(b"test data")?;
       
       // Test store and retrieve
       let file_map = client.store_file(temp_file.path().to_path_buf()).await?;
       let output = NamedTempFile::new()?;
       client.get_file(file_map, output.path().to_path_buf()).await?;
       
       // Verify contents
       assert_eq!(
           fs::read(temp_file.path())?,
           fs::read(output.path())?
       );
   }
   ```

## Day 1 Afternoon: Integration

### Hour 4-6: Local Network Testing

1. **Network Test Utilities**

   ```rust
   // tests/common/network.rs
   pub struct TestNetwork {
       network: LocalNetwork,
       clients: Vec<Client>,
   }

   impl TestNetwork {
       pub async fn new(node_count: usize, client_count: usize) -> Result<Self> {
           let network = LocalNetwork::new(node_count).await?;
           let mut clients = Vec::new();
           
           for _ in 0..client_count {
               clients.push(Client::new_local().await?);
           }
           
           Ok(Self { network, clients })
       }
   }
   ```

2. **Integration Tests**

   ```rust
   #[tokio::test]
   async fn test_multi_client_operations() {
       let test_net = TestNetwork::new(3, 2).await?;
       let [client1, client2] = &test_net.clients[..2] else {
           panic!("Need 2 clients");
       };
       
       // Client 1 stores data
       let data = b"test data";
       let addr = client1.store_bytes(data.to_vec()).await?;
       
       // Client 2 retrieves it
       let retrieved = client2.get_bytes(addr).await?;
       assert_eq!(data, &retrieved[..]);
   }
   ```

### Hour 6-8: Wallet Integration

1. **Basic Wallet Implementation**

   ```rust
   // src/wallet/mod.rs
   pub struct Wallet {
       keypair: Keypair,
       balance: Arc<RwLock<Amount>>,
   }

   impl Wallet {
       pub async fn new() -> Result<Self> {
           let keypair = Keypair::generate_ed25519();
           Ok(Self {
               keypair,
               balance: Arc::new(RwLock::new(Amount::zero())),
           })
       }
   }
   ```

2. **Client Integration**

   ```rust
   impl Client {
       pub async fn with_wallet(
           config: ClientConfig,
           wallet: Wallet
       ) -> Result<Self> {
           let mut client = Self::new(config).await?;
           client.wallet = Some(wallet);
           Ok(client)
       }
   }
   ```

3. **Quick Test**

   ```rust
   #[tokio::test]
   async fn test_wallet_operations() {
       let wallet = Wallet::new().await?;
       let client = Client::with_wallet(
           ClientConfig::default(),
           wallet
       ).await?;
       
       // Test paid storage
       let data = b"paid storage";
       let addr = client.store_bytes(data.to_vec()).await?;
       assert!(addr.is_valid());
   }
   ```

## Day 2 Morning: Python Integration

### Hour 0-2: Python Bindings

1. **Core Types**

   ```python
   # python/autonomi/types.py
   from dataclasses import dataclass
   from typing import Optional, List

   @dataclass
   class FileMap:
       """Represents a stored file's metadata"""
       chunks: List[str]
       size: int
       original_path: str
   ```

2. **Client Implementation**

   ```python
   # python/autonomi/client.py
   class Client:
       @classmethod
       async def new_local(cls) -> 'Client':
           """Create a client for local testing"""
           return cls._create_local()
       
       async def store_file(self, path: str) -> FileMap:
           """Store a file using streaming encryption"""
           return await self._store_file(path)
   ```

### Hour 2-4: Testing & Documentation

1. **Python Tests**

   ```python
   # tests/test_python.py
   import pytest
   from autonomi import Client, FileMap

   async def test_file_operations():
       client = await Client.new_local()
       
       # Create test file
       with open("test.txt", "wb") as f:
           f.write(b"test data")
       
       # Test operations
       file_map = await client.store_file("test.txt")
       await client.get_file(file_map, "retrieved.txt")
       
       # Verify
       with open("retrieved.txt", "rb") as f:
           assert f.read() == b"test data"
   ```

## Required Documentation

1. **ant-node Local Testing**
   - Using the --local flag for testing
   - Local network setup with ant-node
   - MDNS discovery in local mode
   - Proper shutdown and cleanup

2. **libp2p MDNS**
   - Implementation details for local discovery
   - Best practices for testing setups

3. **self_encryption**
   - Streaming API usage
   - Chunk handling and verification

4. **ant-node**
   - Command line arguments
   - Local network setup

## Testing Strategy

1. **Unit Tests**
   - Test each component in isolation
   - Mock network operations where appropriate
   - Test error conditions
   - Verify local mode functionality

2. **Integration Tests**
   - Test complete workflows with local nodes
   - Test multiple clients in local mode
   - Test network failures
   - Verify MDNS discovery

3. **Python Tests**
   - Test Python API
   - Test error handling
   - Test resource cleanup

## Checkpoints

### Day 1 Morning

- [ ] Local ant-node builds and starts with --local flag
- [ ] Basic client operations work in local mode
- [ ] File streaming works
- [ ] MDNS discovery working between local nodes

### Day 1 Afternoon

- [ ] Multiple nodes connect via mdns
- [ ] Data transfer between clients works
- [ ] Basic wallet operations work

### Day 2 Morning

- [ ] Python bindings work
- [ ] All tests pass
- [ ] Documentation is clear

### Day 2 Afternoon

- [ ] Performance is acceptable
- [ ] Error handling is robust
- [ ] Examples work

### Local Network Setup

1. **Node Configuration with MDNS**

   ```rust
   pub struct LocalNode {
       process: Child,
       rpc_port: u16,
       peer_id: PeerId,
       multiaddr: Multiaddr,
   }

   impl LocalNode {
       pub async fn start_local() -> Result<Self, NodeError> {
           // Find available port
           let rpc_port = get_available_port()?;
           
           // Start ant-node with local flag which enables MDNS discovery
           let process = Command::new("ant-node")
               .arg("--local")  // This enables MDNS for local discovery
               .arg("--rpc-port")
               .arg(rpc_port.to_string())
               .arg("--log-level")
               .arg("debug")  // Helpful for seeing MDNS activity
               .spawn()?;
           
           // Wait for node to start and get peer info
           let peer_info = wait_for_node_ready(rpc_port).await?;
           
           Ok(Self {
               process,
               rpc_port,
               peer_id: peer_info.peer_id,
               multiaddr: peer_info.multiaddr,
           })
       }
   }
   ```

2. **Client Integration with Local Network**

   ```rust
   impl Client {
       // Create client connected to local network using MDNS
       pub async fn new_local() -> Result<Self> {
           let config = ClientConfig {
               network_type: NetworkType::Local,  // This enables MDNS in client
               ..Default::default()
           };
           Self::new(config).await
       }
   }
   ```

3. **Network Configuration**

   ```rust
   // In networking layer
   let mdns_config = if config.local {
       Some(mdns::Config {
           // Lower query interval to speed up peer discovery
           query_interval: Duration::from_secs(5),
           ..Default::default()
       })
   } else {
       None
   };
   ```

### Best Practices for Local Testing

1. **MDNS Configuration**
   - MDNS is automatically enabled when:
     - Client is initialized with `new_local()` or `local: true` in config
     - Node is started with `--local` flag
   - MDNS discovery is configured for faster peer discovery in local mode
   - Network stabilization wait times are adjusted for local testing

2. **Network Verification**
   - Verify MDNS discovery is working through debug logs
   - Check peer connections before testing
   - Monitor network stability
   - Handle node disconnections gracefully

3. **Development Workflow**
   - Always use `--local` flag for local development
   - Allow sufficient time for MDNS discovery (typically 5 seconds)
   - Monitor MDNS logs for connectivity issues
   - Test with different network sizes