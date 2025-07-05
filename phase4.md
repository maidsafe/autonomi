# Phase 4: Production Deployment and Monitoring

## Objective
Deploy the dual-stack network to production with comprehensive monitoring, gradual rollout strategies, and the ability to safely migrate the entire network from libp2p to iroh while maintaining service availability.

## Prerequisites
- Phase 3 completed: Dual-stack networking implemented and tested
- Both transports proven stable in test environments
- Metrics collection infrastructure in place
- Team familiar with dual-stack operation

## Tasks

### 1. Production Monitoring Infrastructure

Create `ant-node/src/networking/monitoring/mod.rs`:
```rust
pub mod dashboard;
pub mod alerts;
pub mod health;
pub mod migration_tracker;

use prometheus::{Registry, Encoder, TextEncoder};

/// Production monitoring system for dual-stack network
pub struct NetworkMonitor {
    /// Prometheus registry
    registry: Registry,
    /// Real-time dashboard data
    dashboard: DashboardCollector,
    /// Alert manager
    alerts: AlertManager,
    /// Health check system
    health: HealthChecker,
    /// Migration progress tracker
    migration: MigrationTracker,
    /// Historical data store
    time_series: TimeSeriesStore,
}

impl NetworkMonitor {
    pub async fn new(config: MonitorConfig) -> Result<Self> {
        let registry = Registry::new();
        
        // Register all metrics
        let dashboard = DashboardCollector::new(&registry)?;
        let alerts = AlertManager::new(config.alert_config)?;
        let health = HealthChecker::new(config.health_config)?;
        let migration = MigrationTracker::new(&registry)?;
        let time_series = TimeSeriesStore::new(config.storage_config)?;
        
        Ok(Self {
            registry,
            dashboard,
            alerts,
            health,
            migration,
            time_series,
        })
    }
    
    /// Start monitoring loops
    pub async fn start(&mut self) -> Result<()> {
        // Start metric collection
        tokio::spawn(self.collect_metrics_loop());
        
        // Start health checks
        tokio::spawn(self.health_check_loop());
        
        // Start alert evaluation
        tokio::spawn(self.alert_evaluation_loop());
        
        // Start dashboard updates
        tokio::spawn(self.dashboard_update_loop());
        
        Ok(())
    }
}
```

### 2. Real-time Dashboard

Create `ant-node/src/networking/monitoring/dashboard.rs`:
```rust
/// Real-time metrics for operations dashboard
pub struct DashboardCollector {
    // Transport health
    libp2p_health_score: Gauge,
    iroh_health_score: Gauge,
    
    // Active connections
    libp2p_active_connections: Gauge,
    iroh_active_connections: Gauge,
    
    // Request rates (per second)
    libp2p_request_rate: Gauge,
    iroh_request_rate: Gauge,
    
    // Error rates
    libp2p_error_rate: Gauge,
    iroh_error_rate: Gauge,
    
    // P50, P95, P99 latencies
    libp2p_latency_p50: Gauge,
    libp2p_latency_p95: Gauge,
    libp2p_latency_p99: Gauge,
    iroh_latency_p50: Gauge,
    iroh_latency_p95: Gauge,
    iroh_latency_p99: Gauge,
    
    // Network partition detection
    partition_risk_score: Gauge,
    
    // Migration progress
    migration_progress_percent: Gauge,
    migration_estimated_completion: Gauge,
}

impl DashboardCollector {
    pub fn calculate_health_score(&self, transport: Transport) -> f64 {
        match transport {
            Transport::LibP2p => {
                let success_rate = self.get_success_rate(Transport::LibP2p);
                let latency_score = self.get_latency_score(Transport::LibP2p);
                let connection_score = self.get_connection_score(Transport::LibP2p);
                
                // Weighted average
                (success_rate * 0.5 + latency_score * 0.3 + connection_score * 0.2)
            }
            Transport::Iroh => {
                // Similar calculation for iroh
            }
        }
    }
    
    pub fn detect_network_partition(&self) -> f64 {
        // Analyze peer connectivity patterns
        // Return risk score 0.0 (healthy) to 1.0 (likely partitioned)
    }
}

/// Dashboard API endpoints
pub struct DashboardServer {
    collector: Arc<DashboardCollector>,
}

impl DashboardServer {
    pub async fn serve(self, addr: SocketAddr) -> Result<()> {
        let app = Router::new()
            .route("/metrics", get(self.metrics_handler))
            .route("/health", get(self.health_handler))
            .route("/status", get(self.status_handler))
            .route("/ws", get(self.websocket_handler));
        
        axum::Server::bind(&addr)
            .serve(app.into_make_service())
            .await?;
        
        Ok(())
    }
    
    async fn websocket_handler(
        ws: WebSocketUpgrade,
        State(collector): State<Arc<DashboardCollector>>,
    ) -> Response {
        ws.on_upgrade(|socket| handle_socket(socket, collector))
    }
}
```

### 3. Alert System

Create `ant-node/src/networking/monitoring/alerts.rs`:
```rust
#[derive(Debug, Clone)]
pub struct Alert {
    pub id: String,
    pub severity: AlertSeverity,
    pub transport: Option<Transport>,
    pub title: String,
    pub description: String,
    pub triggered_at: Instant,
    pub resolved_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertSeverity {
    Critical, // Page immediately
    Warning,  // Notify on-call
    Info,     // Log only
}

pub struct AlertManager {
    rules: Vec<AlertRule>,
    active_alerts: Arc<Mutex<HashMap<String, Alert>>>,
    notification_channels: Vec<Box<dyn NotificationChannel>>,
}

pub struct AlertRule {
    pub id: String,
    pub name: String,
    pub condition: Box<dyn AlertCondition>,
    pub severity: AlertSeverity,
    pub cooldown: Duration,
}

/// Alert conditions
pub trait AlertCondition: Send + Sync {
    fn evaluate(&self, metrics: &NetworkMetrics) -> bool;
    fn description(&self) -> String;
}

/// Example alert conditions
pub struct HighErrorRate {
    pub transport: Transport,
    pub threshold: f64,
    pub duration: Duration,
}

impl AlertCondition for HighErrorRate {
    fn evaluate(&self, metrics: &NetworkMetrics) -> bool {
        let error_rate = metrics.get_error_rate(self.transport);
        let duration_exceeded = metrics
            .get_error_rate_duration(self.transport, self.threshold)
            .map(|d| d > self.duration)
            .unwrap_or(false);
        
        error_rate > self.threshold && duration_exceeded
    }
    
    fn description(&self) -> String {
        format!("{:?} error rate > {}% for {:?}", 
            self.transport, self.threshold * 100.0, self.duration)
    }
}

impl AlertManager {
    pub fn default_rules() -> Vec<AlertRule> {
        vec![
            AlertRule {
                id: "high_error_rate_libp2p".into(),
                name: "High libp2p Error Rate".into(),
                condition: Box::new(HighErrorRate {
                    transport: Transport::LibP2p,
                    threshold: 0.05, // 5%
                    duration: Duration::from_secs(300), // 5 minutes
                }),
                severity: AlertSeverity::Critical,
                cooldown: Duration::from_secs(3600), // 1 hour
            },
            AlertRule {
                id: "iroh_latency_spike".into(),
                name: "iroh Latency Spike".into(),
                condition: Box::new(LatencySpike {
                    transport: Transport::Iroh,
                    threshold_ms: 500,
                    percentile: 95,
                }),
                severity: AlertSeverity::Warning,
                cooldown: Duration::from_secs(1800), // 30 minutes
            },
            AlertRule {
                id: "network_partition_risk".into(),
                name: "Network Partition Risk".into(),
                condition: Box::new(PartitionRisk {
                    threshold: 0.7,
                }),
                severity: AlertSeverity::Critical,
                cooldown: Duration::from_secs(900), // 15 minutes
            },
        ]
    }
}
```

### 4. Health Check System

Create `ant-node/src/networking/monitoring/health.rs`:
```rust
pub struct HealthChecker {
    checks: Vec<Box<dyn HealthCheck>>,
    status: Arc<Mutex<HealthStatus>>,
}

#[derive(Debug, Clone)]
pub struct HealthStatus {
    pub overall: HealthState,
    pub libp2p: HealthState,
    pub iroh: HealthState,
    pub checks: HashMap<String, CheckResult>,
    pub last_updated: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthState {
    Healthy,
    Degraded,
    Unhealthy,
}

pub trait HealthCheck: Send + Sync {
    fn name(&self) -> &str;
    async fn check(&self, network: &DualStackNetwork) -> CheckResult;
}

/// Example health checks
pub struct ConnectivityCheck {
    pub min_peers: usize,
    pub test_peers: Vec<KadPeerId>,
}

impl HealthCheck for ConnectivityCheck {
    fn name(&self) -> &str {
        "connectivity"
    }
    
    async fn check(&self, network: &DualStackNetwork) -> CheckResult {
        let connected_peers = network.connected_peers().await;
        
        if connected_peers.len() < self.min_peers {
            return CheckResult::Failed {
                reason: format!("Only {} peers connected, minimum {}", 
                    connected_peers.len(), self.min_peers),
            };
        }
        
        // Test connectivity to specific important peers
        for peer in &self.test_peers {
            if !network.can_reach(peer).await {
                return CheckResult::Degraded {
                    reason: format!("Cannot reach important peer: {:?}", peer),
                };
            }
        }
        
        CheckResult::Healthy
    }
}

pub struct KademliaRoutingCheck;

impl HealthCheck for KademliaRoutingCheck {
    fn name(&self) -> &str {
        "kademlia_routing"
    }
    
    async fn check(&self, network: &DualStackNetwork) -> CheckResult {
        // Test Kademlia operations
        let test_key = random_key();
        
        // Try to find closest peers
        match network.find_closest_peers(&test_key, 20).await {
            Ok(peers) if peers.len() >= 10 => CheckResult::Healthy,
            Ok(peers) => CheckResult::Degraded {
                reason: format!("Only found {} peers for routing test", peers.len()),
            },
            Err(e) => CheckResult::Failed {
                reason: format!("Routing test failed: {}", e),
            },
        }
    }
}
```

### 5. Migration Tracker

Create `ant-node/src/networking/monitoring/migration_tracker.rs`:
```rust
pub struct MigrationTracker {
    /// Migration start time
    start_time: Option<Instant>,
    /// Target completion time
    target_duration: Duration,
    /// Node migration status
    node_status: Arc<Mutex<HashMap<NodeId, NodeMigrationStatus>>>,
    /// Metrics
    nodes_total: Gauge,
    nodes_migrated: Gauge,
    nodes_dual_stack: Gauge,
    nodes_rollback: Gauge,
    migration_velocity: Gauge, // nodes per hour
}

#[derive(Debug, Clone)]
pub struct NodeMigrationStatus {
    pub node_id: NodeId,
    pub current_state: MigrationState,
    pub libp2p_version: String,
    pub iroh_version: Option<String>,
    pub migration_started: Option<Instant>,
    pub migration_completed: Option<Instant>,
    pub metrics: NodeMetrics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationState {
    LibP2pOnly,
    DualStackTesting,
    DualStackStable,
    IrohPrimary,
    IrohOnly,
    RolledBack,
}

impl MigrationTracker {
    pub async fn start_migration(&mut self, plan: MigrationPlan) -> Result<()> {
        self.start_time = Some(Instant::now());
        self.target_duration = plan.duration;
        
        info!("Starting network migration with plan: {:?}", plan);
        
        // Initialize tracking for all nodes
        for node in plan.nodes {
            self.node_status.lock().unwrap().insert(
                node.id.clone(),
                NodeMigrationStatus {
                    node_id: node.id,
                    current_state: MigrationState::LibP2pOnly,
                    libp2p_version: node.version,
                    iroh_version: None,
                    migration_started: None,
                    migration_completed: None,
                    metrics: NodeMetrics::default(),
                },
            );
        }
        
        Ok(())
    }
    
    pub fn get_migration_progress(&self) -> MigrationProgress {
        let statuses = self.node_status.lock().unwrap();
        
        let total = statuses.len();
        let migrated = statuses.values()
            .filter(|s| matches!(s.current_state, 
                MigrationState::IrohPrimary | MigrationState::IrohOnly))
            .count();
        
        let progress_percent = if total > 0 {
            (migrated as f64 / total as f64) * 100.0
        } else {
            0.0
        };
        
        let estimated_completion = self.estimate_completion_time();
        
        MigrationProgress {
            started_at: self.start_time,
            progress_percent,
            nodes_total: total,
            nodes_migrated: migrated,
            estimated_completion,
            current_phase: self.determine_phase(progress_percent),
        }
    }
    
    fn determine_phase(&self, progress: f64) -> MigrationPhase {
        match progress {
            p if p < 5.0 => MigrationPhase::Initialization,
            p if p < 25.0 => MigrationPhase::EarlyAdopters,
            p if p < 75.0 => MigrationPhase::MajorityMigration,
            p if p < 95.0 => MigrationPhase::Finalization,
            _ => MigrationPhase::Completed,
        }
    }
}
```

### 6. Deployment Strategy

Create `ant-node/src/networking/deployment/strategy.rs`:
```rust
pub struct DeploymentManager {
    /// Current deployment phase
    phase: DeploymentPhase,
    /// Rollout configuration
    config: RolloutConfig,
    /// Node selector
    selector: NodeSelector,
    /// Rollback controller
    rollback: RollbackController,
}

#[derive(Debug, Clone)]
pub struct RolloutConfig {
    /// Percentage of nodes to migrate in each wave
    pub wave_size: f64,
    /// Time between waves
    pub wave_interval: Duration,
    /// Success criteria before proceeding
    pub success_threshold: SuccessThreshold,
    /// Automatic rollback triggers
    pub rollback_triggers: Vec<RollbackTrigger>,
    /// Canary configuration
    pub canary_config: CanaryConfig,
}

#[derive(Debug, Clone)]
pub struct CanaryConfig {
    /// Number of canary nodes
    pub node_count: usize,
    /// Canary duration before proceeding
    pub duration: Duration,
    /// Specific nodes to use as canaries
    pub specific_nodes: Option<Vec<NodeId>>,
    /// Extra monitoring for canaries
    pub enhanced_monitoring: bool,
}

impl DeploymentManager {
    pub async fn execute_deployment(&mut self) -> Result<()> {
        // Phase 1: Deploy to canary nodes
        self.phase = DeploymentPhase::Canary;
        self.deploy_canary().await?;
        
        // Phase 2: Early adopters (5%)
        self.phase = DeploymentPhase::EarlyAdopters;
        self.deploy_wave(0.05).await?;
        
        // Phase 3: Gradual rollout (25%, 50%, 75%)
        self.phase = DeploymentPhase::GradualRollout;
        for percentage in [0.25, 0.50, 0.75] {
            self.deploy_wave(percentage).await?;
            
            // Check success criteria
            if !self.check_success_criteria().await? {
                warn!("Success criteria not met at {}%", percentage * 100.0);
                self.pause_deployment().await?;
            }
        }
        
        // Phase 4: Complete migration
        self.phase = DeploymentPhase::Completion;
        self.deploy_wave(1.0).await?;
        
        // Phase 5: Cleanup
        self.phase = DeploymentPhase::Cleanup;
        self.cleanup_old_infrastructure().await?;
        
        Ok(())
    }
    
    async fn deploy_canary(&mut self) -> Result<()> {
        let canary_nodes = self.selector.select_canary_nodes(&self.config.canary_config)?;
        
        info!("Deploying to {} canary nodes", canary_nodes.len());
        
        for node_id in canary_nodes {
            self.enable_dual_stack(&node_id).await?;
            
            // Enhanced monitoring for canaries
            if self.config.canary_config.enhanced_monitoring {
                self.enable_enhanced_monitoring(&node_id).await?;
            }
        }
        
        // Monitor canaries for specified duration
        tokio::time::sleep(self.config.canary_config.duration).await;
        
        // Validate canary health
        let canary_health = self.validate_canary_health().await?;
        if !canary_health.is_healthy() {
            error!("Canary validation failed: {:?}", canary_health);
            self.rollback_canaries().await?;
            return Err(anyhow!("Canary deployment failed"));
        }
        
        Ok(())
    }
}
```

### 7. Rollback System

Create `ant-node/src/networking/deployment/rollback.rs`:
```rust
pub struct RollbackController {
    /// Rollback history
    history: Vec<RollbackEvent>,
    /// Active rollback operations
    active_rollbacks: Arc<Mutex<HashMap<String, RollbackOperation>>>,
    /// Rollback strategies
    strategies: HashMap<RollbackTrigger, Box<dyn RollbackStrategy>>,
}

#[derive(Debug, Clone)]
pub enum RollbackTrigger {
    HighErrorRate { threshold: f64 },
    NetworkPartition { confidence: f64 },
    LatencyRegression { threshold_ms: u64 },
    ManualTrigger { reason: String },
    HealthCheckFailure { check_name: String },
}

pub trait RollbackStrategy: Send + Sync {
    async fn execute(&self, context: &RollbackContext) -> Result<()>;
    fn estimated_duration(&self) -> Duration;
}

pub struct FastRollback;

impl RollbackStrategy for FastRollback {
    async fn execute(&self, context: &RollbackContext) -> Result<()> {
        // Immediately switch all dual-stack nodes back to libp2p
        for node_id in &context.affected_nodes {
            context.network.set_mode(node_id, NetworkMode::LibP2pOnly).await?;
        }
        
        // Disable iroh on all nodes
        context.network.disable_iroh_globally().await?;
        
        Ok(())
    }
    
    fn estimated_duration(&self) -> Duration {
        Duration::from_secs(60) // 1 minute
    }
}

pub struct GradualRollback {
    pub wave_size: f64,
    pub wave_interval: Duration,
}

impl RollbackStrategy for GradualRollback {
    async fn execute(&self, context: &RollbackContext) -> Result<()> {
        let total_nodes = context.affected_nodes.len();
        let nodes_per_wave = (total_nodes as f64 * self.wave_size) as usize;
        
        for chunk in context.affected_nodes.chunks(nodes_per_wave) {
            for node_id in chunk {
                context.network.set_mode(node_id, NetworkMode::LibP2pOnly).await?;
            }
            
            // Wait between waves
            tokio::time::sleep(self.wave_interval).await;
            
            // Check if situation improved
            if context.is_issue_resolved().await? {
                info!("Issue resolved during gradual rollback");
                return Ok(());
            }
        }
        
        Ok(())
    }
    
    fn estimated_duration(&self) -> Duration {
        Duration::from_secs(1800) // 30 minutes
    }
}
```

### 8. Production CLI Tools

Create `ant-node/src/bin/network-migration-cli.rs`:
```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[clap(name = "ant-network-migration")]
#[clap(about = "Autonomi Network Migration Control")]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show migration status
    Status {
        #[clap(long)]
        detailed: bool,
    },
    
    /// Start migration
    Start {
        /// Migration plan file
        #[clap(long)]
        plan: PathBuf,
        
        /// Dry run mode
        #[clap(long)]
        dry_run: bool,
    },
    
    /// Pause migration
    Pause {
        /// Reason for pause
        #[clap(long)]
        reason: String,
    },
    
    /// Resume migration
    Resume,
    
    /// Trigger rollback
    Rollback {
        /// Rollback strategy
        #[clap(long, default_value = "fast")]
        strategy: String,
        
        /// Target state
        #[clap(long)]
        target: String,
    },
    
    /// Health check
    Health {
        /// Specific transport
        #[clap(long)]
        transport: Option<String>,
    },
    
    /// Show metrics
    Metrics {
        /// Output format
        #[clap(long, default_value = "table")]
        format: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    
    // Connect to network controller
    let controller = NetworkController::connect().await?;
    
    match cli.command {
        Commands::Status { detailed } => {
            let status = controller.get_migration_status().await?;
            if detailed {
                println!("{:#?}", status);
            } else {
                println!("Migration Progress: {:.1}%", status.progress_percent);
                println!("Phase: {:?}", status.current_phase);
                println!("Nodes migrated: {}/{}", status.nodes_migrated, status.nodes_total);
            }
        }
        
        Commands::Start { plan, dry_run } => {
            let plan_data = std::fs::read_to_string(plan)?;
            let migration_plan: MigrationPlan = serde_json::from_str(&plan_data)?;
            
            if dry_run {
                println!("Dry run mode - validating plan...");
                controller.validate_plan(&migration_plan).await?;
                println!("Plan is valid!");
            } else {
                controller.start_migration(migration_plan).await?;
                println!("Migration started");
            }
        }
        
        // ... implement other commands
    }
    
    Ok(())
}
```

### 9. Testing Production Scenarios

Create `ant-node/tests/production_scenarios.rs`:
```rust
#[cfg(test)]
mod production_tests {
    use super::*;
    
    #[tokio::test]
    async fn test_canary_deployment_failure() {
        // Test that canary failures prevent wider rollout
    }
    
    #[tokio::test]
    async fn test_network_partition_during_migration() {
        // Test behavior when network partitions during migration
    }
    
    #[tokio::test]
    async fn test_rapid_rollback() {
        // Test that rollback completes within SLA
    }
    
    #[tokio::test]
    async fn test_gradual_migration_with_load() {
        // Test migration under production-like load
    }
}
```

## Validation Criteria

1. Monitoring dashboard provides real-time visibility into both stacks
2. Alerts fire correctly for defined conditions
3. Health checks accurately reflect system state
4. Migration can be paused, resumed, and rolled back
5. Canary deployments catch issues before wider rollout
6. Performance metrics show iroh improvements
7. No service disruption during migration
8. Rollback completes within 5 minutes

## Production Checklist

- [ ] Monitoring infrastructure deployed
- [ ] Alert recipients configured
- [ ] Runbooks created for common issues
- [ ] Canary nodes identified
- [ ] Rollback procedures tested
- [ ] Load testing completed
- [ ] Communication plan ready
- [ ] Backup systems verified

## Notes

- Start with conservative thresholds and adjust based on experience
- Keep detailed logs of all migration activities
- Have on-call engineers during critical migration phases
- Plan for multiple rollback scenarios

## Next Phase
Phase 5 will complete the migration and remove libp2p dependencies.
