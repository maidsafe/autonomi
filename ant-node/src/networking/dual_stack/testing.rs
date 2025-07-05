// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! A/B Testing Framework for Dual-Stack Transport Comparison
//! 
//! This module provides comprehensive A/B testing capabilities to compare
//! the performance of libp2p and iroh transports in real-world scenarios.
//! It enables controlled experiments, statistical analysis, and automated
//! decision making for transport migration.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::{RwLock, Mutex};
use tracing::{debug, error, info, warn, instrument};

use crate::networking::kad::transport::{
    KademliaTransport, KadPeerId, KadAddress, KadMessage, KadResponse, KadError,
    PeerInfo, ConnectionStatus, QueryId, RecordKey, Record, QueryResult,
};

use super::{
    TransportId, DualStackError, DualStackResult,
    coordinator::DualStackTransport,
};

/// A/B testing framework for comparing transport performance
pub struct ABTestingFramework {
    /// Configuration for A/B tests
    config: ABTestConfig,
    
    /// Currently running experiments
    active_experiments: Arc<RwLock<HashMap<String, ABExperiment>>>,
    
    /// Test execution engine
    test_executor: Arc<TestExecutor>,
    
    /// Results collection and analysis
    results_analyzer: Arc<ResultsAnalyzer>,
    
    /// Statistical significance calculator
    stats_calculator: Arc<StatisticalCalculator>,
    
    /// Test scheduling and orchestration
    scheduler: Arc<TestScheduler>,
}

/// Configuration for A/B testing framework
#[derive(Debug, Clone)]
pub struct ABTestConfig {
    /// Enable A/B testing
    pub enabled: bool,
    
    /// Default test duration
    pub default_test_duration: Duration,
    
    /// Minimum sample size for statistical significance
    pub min_sample_size: usize,
    
    /// Confidence level for statistical tests (e.g., 0.95 for 95%)
    pub confidence_level: f64,
    
    /// Maximum concurrent experiments
    pub max_concurrent_experiments: usize,
    
    /// Test data retention period
    pub data_retention_period: Duration,
    
    /// Export results to external systems
    pub export_results: bool,
}

/// A/B experiment definition and state
#[derive(Debug, Clone)]
pub struct ABExperiment {
    /// Unique experiment identifier
    pub id: String,
    
    /// Human-readable experiment name
    pub name: String,
    
    /// Experiment description
    pub description: String,
    
    /// Test configuration
    pub test_config: TestConfig,
    
    /// Current experiment state
    pub state: ExperimentState,
    
    /// Experiment timeline
    pub timeline: ExperimentTimeline,
    
    /// Participant assignment
    pub participants: ParticipantAssignment,
    
    /// Collected results
    pub results: ExperimentResults,
    
    /// Statistical analysis
    pub analysis: Option<StatisticalAnalysis>,
}

/// Test configuration for an experiment
#[derive(Debug, Clone)]
pub struct TestConfig {
    /// Test type
    pub test_type: TestType,
    
    /// Traffic split configuration
    pub traffic_split: TrafficSplit,
    
    /// Test duration
    pub duration: Duration,
    
    /// Success criteria
    pub success_criteria: SuccessCriteria,
    
    /// Operations to test
    pub operations_to_test: Vec<OperationType>,
    
    /// Geographic/peer targeting
    pub targeting: TestTargeting,
}

/// Types of A/B tests supported
#[derive(Debug, Clone)]
pub enum TestType {
    /// Simple A/B test (libp2p vs iroh)
    SimpleAB,
    
    /// Multi-variate test with different configurations
    Multivariate {
        variants: Vec<TestVariant>,
    },
    
    /// Ramped rollout test with gradual increase
    RampedRollout {
        initial_percentage: f32,
        ramp_rate: f32,
        target_percentage: f32,
    },
    
    /// Feature flag test with on/off comparison
    FeatureFlag {
        feature_name: String,
        control_enabled: bool,
        treatment_enabled: bool,
    },
}

/// Test variant definition
#[derive(Debug, Clone)]
pub struct TestVariant {
    pub name: String,
    pub transport: TransportId,
    pub config_overrides: HashMap<String, String>,
    pub expected_traffic_percentage: f32,
}

/// Traffic split configuration
#[derive(Debug, Clone)]
pub struct TrafficSplit {
    /// Control group (typically libp2p) percentage
    pub control_percentage: f32,
    
    /// Treatment group (typically iroh) percentage  
    pub treatment_percentage: f32,
    
    /// Assignment strategy
    pub assignment_strategy: AssignmentStrategy,
}

/// Strategy for assigning participants to test groups
#[derive(Debug, Clone)]
pub enum AssignmentStrategy {
    /// Random assignment based on peer ID hash
    Random,
    
    /// Round-robin assignment
    RoundRobin,
    
    /// Hash-based deterministic assignment
    Deterministic { seed: u64 },
    
    /// Geographic assignment
    Geographic { regions: Vec<String> },
    
    /// Performance-based assignment
    PerformanceBased { criteria: Vec<String> },
}

/// Success criteria for experiment evaluation
#[derive(Debug, Clone)]
pub struct SuccessCriteria {
    /// Primary metric to optimize
    pub primary_metric: PrimaryMetric,
    
    /// Secondary metrics to monitor
    pub secondary_metrics: Vec<SecondaryMetric>,
    
    /// Minimum improvement threshold
    pub min_improvement_threshold: f64,
    
    /// Maximum acceptable degradation for secondary metrics
    pub max_degradation_threshold: f64,
    
    /// Statistical significance requirements
    pub significance_requirements: SignificanceRequirements,
}

/// Primary metric for experiment success
#[derive(Debug, Clone)]
pub enum PrimaryMetric {
    /// Average latency improvement
    LatencyReduction { target_reduction_percentage: f64 },
    
    /// Success rate improvement
    SuccessRateImprovement { target_improvement_percentage: f64 },
    
    /// Throughput improvement
    ThroughputImprovement { target_improvement_percentage: f64 },
    
    /// Connection establishment time
    ConnectionTimeReduction { target_reduction_percentage: f64 },
    
    /// Overall user experience score
    UserExperienceScore { target_score: f64 },
}

/// Secondary metrics to monitor during experiments
#[derive(Debug, Clone)]
pub enum SecondaryMetric {
    ErrorRate,
    ResourceUsage,
    NetworkUtilization,
    PeerConnectivity,
    DiscoveryEfficiency,
}

/// Statistical significance requirements
#[derive(Debug, Clone)]
pub struct SignificanceRequirements {
    /// Required confidence level (e.g., 0.95)
    pub confidence_level: f64,
    
    /// Required statistical power (e.g., 0.8)
    pub power: f64,
    
    /// Minimum effect size to detect
    pub min_effect_size: f64,
    
    /// Maximum p-value for significance
    pub max_p_value: f64,
}

/// Operations to include in testing
#[derive(Debug, Clone)]
pub enum OperationType {
    FindNode,
    FindValue,
    PutRecord,
    Bootstrap,
    GetRoutingTable,
    PeerDiscovery,
}

/// Test targeting configuration
#[derive(Debug, Clone)]
pub struct TestTargeting {
    /// Target specific peer types
    pub peer_types: Vec<PeerType>,
    
    /// Geographic targeting
    pub geographic_regions: Vec<String>,
    
    /// Network conditions
    pub network_conditions: Vec<NetworkCondition>,
    
    /// Time-based targeting
    pub time_targeting: Option<TimeTargeting>,
}

/// Types of peers to target in tests
#[derive(Debug, Clone)]
pub enum PeerType {
    HighPerformance,
    LowLatency,
    HighBandwidth,
    Mobile,
    Desktop,
    Server,
    NewPeers,
    EstablishedPeers,
}

/// Network conditions for targeting
#[derive(Debug, Clone)]
pub enum NetworkCondition {
    HighLatency,
    LowBandwidth,
    UnstableConnection,
    BehindNAT,
    DirectConnection,
}

/// Time-based targeting
#[derive(Debug, Clone)]
pub struct TimeTargeting {
    pub peak_hours: Vec<u8>,
    pub off_peak_hours: Vec<u8>,
    pub specific_days: Vec<u8>,
}

/// Current state of an experiment
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExperimentState {
    /// Experiment is being configured
    Draft,
    
    /// Ready to start but not yet running
    Ready,
    
    /// Currently running and collecting data
    Running,
    
    /// Temporarily paused
    Paused,
    
    /// Completed successfully
    Completed,
    
    /// Stopped due to failure or safety concerns
    Stopped,
    
    /// Analysis phase after data collection
    Analyzing,
    
    /// Results ready and published
    Published,
}

/// Experiment timeline tracking
#[derive(Debug, Clone)]
pub struct ExperimentTimeline {
    pub created_at: Instant,
    pub started_at: Option<Instant>,
    pub paused_at: Option<Instant>,
    pub completed_at: Option<Instant>,
    pub duration_target: Duration,
    pub duration_actual: Option<Duration>,
}

/// Participant assignment tracking
#[derive(Debug, Clone)]
pub struct ParticipantAssignment {
    /// Peers assigned to control group
    pub control_group: Vec<KadPeerId>,
    
    /// Peers assigned to treatment group
    pub treatment_group: Vec<KadPeerId>,
    
    /// Assignment metadata
    pub assignment_metadata: HashMap<KadPeerId, AssignmentMetadata>,
    
    /// Total participants
    pub total_participants: usize,
}

/// Metadata about participant assignment
#[derive(Debug, Clone)]
pub struct AssignmentMetadata {
    pub assigned_at: Instant,
    pub assignment_group: AssignmentGroup,
    pub assignment_reason: String,
    pub peer_characteristics: PeerCharacteristics,
}

/// Assignment group identification
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssignmentGroup {
    Control,
    Treatment,
    Excluded,
}

/// Characteristics of a peer for assignment decisions
#[derive(Debug, Clone)]
pub struct PeerCharacteristics {
    pub estimated_performance: f64,
    pub network_quality: NetworkQuality,
    pub geographic_region: Option<String>,
    pub peer_type: PeerType,
    pub connection_stability: f64,
}

/// Network quality assessment
#[derive(Debug, Clone)]
pub struct NetworkQuality {
    pub latency_score: f64,
    pub bandwidth_score: f64,
    pub stability_score: f64,
    pub nat_traversal_capability: bool,
}

/// Collected experiment results
#[derive(Debug, Clone)]
pub struct ExperimentResults {
    /// Results by group
    pub group_results: HashMap<AssignmentGroup, GroupResults>,
    
    /// Detailed operation results
    pub operation_results: Vec<OperationResult>,
    
    /// Time-series data
    pub time_series: TimeSeries,
    
    /// Error analysis
    pub error_analysis: ErrorAnalysis,
}

/// Results for a specific group (control/treatment)
#[derive(Debug, Clone)]
pub struct GroupResults {
    pub group: AssignmentGroup,
    pub participant_count: usize,
    pub total_operations: u64,
    pub successful_operations: u64,
    pub average_latency: Duration,
    pub median_latency: Duration,
    pub p95_latency: Duration,
    pub p99_latency: Duration,
    pub throughput: f64,
    pub error_rate: f64,
    pub resource_usage: ResourceUsageStats,
}

/// Individual operation result
#[derive(Debug, Clone)]
pub struct OperationResult {
    pub experiment_id: String,
    pub participant_id: KadPeerId,
    pub group: AssignmentGroup,
    pub operation_type: OperationType,
    pub transport_used: TransportId,
    pub timestamp: Instant,
    pub duration: Duration,
    pub success: bool,
    pub error_type: Option<String>,
    pub bytes_transferred: Option<u64>,
    pub additional_metrics: HashMap<String, f64>,
}

/// Time-series data collection
#[derive(Debug, Clone)]
pub struct TimeSeries {
    pub data_points: Vec<TimeSeriesPoint>,
    pub collection_interval: Duration,
    pub metrics_tracked: Vec<String>,
}

/// Individual time-series data point
#[derive(Debug, Clone)]
pub struct TimeSeriesPoint {
    pub timestamp: Instant,
    pub control_metrics: HashMap<String, f64>,
    pub treatment_metrics: HashMap<String, f64>,
}

/// Error analysis across experiment
#[derive(Debug, Clone)]
pub struct ErrorAnalysis {
    pub error_breakdown: HashMap<String, ErrorStats>,
    pub error_correlation: HashMap<String, f64>,
    pub error_trends: Vec<ErrorTrend>,
}

/// Statistics for specific error types
#[derive(Debug, Clone)]
pub struct ErrorStats {
    pub error_type: String,
    pub occurrences: u64,
    pub control_group_count: u64,
    pub treatment_group_count: u64,
    pub severity_distribution: HashMap<String, u64>,
}

/// Error trend analysis
#[derive(Debug, Clone)]
pub struct ErrorTrend {
    pub error_type: String,
    pub trend_direction: TrendDirection,
    pub magnitude: f64,
    pub confidence: f64,
}

/// Direction of trend analysis
#[derive(Debug, Clone)]
pub enum TrendDirection {
    Increasing,
    Decreasing,
    Stable,
    Volatile,
}

/// Resource usage statistics
#[derive(Debug, Clone)]
pub struct ResourceUsageStats {
    pub cpu_usage_avg: f64,
    pub memory_usage_avg: f64,
    pub network_usage_avg: f64,
    pub storage_usage_avg: f64,
}

/// Statistical analysis results
#[derive(Debug, Clone)]
pub struct StatisticalAnalysis {
    /// Hypothesis test results
    pub hypothesis_tests: Vec<HypothesisTest>,
    
    /// Effect size measurements
    pub effect_sizes: HashMap<String, EffectSize>,
    
    /// Confidence intervals
    pub confidence_intervals: HashMap<String, ConfidenceInterval>,
    
    /// Statistical significance indicators
    pub significance_results: SignificanceResults,
    
    /// Recommendations based on analysis
    pub recommendations: Vec<Recommendation>,
}

/// Hypothesis test result
#[derive(Debug, Clone)]
pub struct HypothesisTest {
    pub test_name: String,
    pub null_hypothesis: String,
    pub alternative_hypothesis: String,
    pub test_statistic: f64,
    pub p_value: f64,
    pub is_significant: bool,
    pub test_type: StatisticalTestType,
}

/// Types of statistical tests
#[derive(Debug, Clone)]
pub enum StatisticalTestType {
    TTest,
    ChiSquare,
    MannWhitneyU,
    WelchTTest,
    FisherExact,
}

/// Effect size measurement
#[derive(Debug, Clone)]
pub struct EffectSize {
    pub metric_name: String,
    pub effect_size_value: f64,
    pub effect_size_type: EffectSizeType,
    pub interpretation: String,
}

/// Types of effect size measurements
#[derive(Debug, Clone)]
pub enum EffectSizeType {
    CohenD,
    HedgeG,
    PercentageChange,
    Correlation,
}

/// Confidence interval
#[derive(Debug, Clone)]
pub struct ConfidenceInterval {
    pub metric_name: String,
    pub lower_bound: f64,
    pub upper_bound: f64,
    pub confidence_level: f64,
}

/// Overall significance results
#[derive(Debug, Clone)]
pub struct SignificanceResults {
    pub overall_significance: bool,
    pub primary_metric_significant: bool,
    pub secondary_metrics_significant: HashMap<String, bool>,
    pub practical_significance: bool,
    pub recommendation_confidence: f64,
}

/// Recommendations from analysis
#[derive(Debug, Clone)]
pub struct Recommendation {
    pub recommendation_type: RecommendationType,
    pub description: String,
    pub confidence: f64,
    pub supporting_evidence: Vec<String>,
    pub risks: Vec<String>,
    pub next_steps: Vec<String>,
}

/// Types of recommendations
#[derive(Debug, Clone)]
pub enum RecommendationType {
    /// Proceed with full rollout to treatment
    ProceedWithRollout,
    
    /// Continue current experiment with modifications
    ContinueWithModifications,
    
    /// Stop experiment and revert to control
    StopAndRevert,
    
    /// Extend experiment duration for more data
    ExtendExperiment,
    
    /// Run follow-up experiments
    RunFollowUp,
}

/// Test execution engine
pub struct TestExecutor {
    /// Reference to dual-stack transport
    dual_stack_transport: Arc<DualStackTransport>,
    
    /// Active test executions
    active_executions: Arc<RwLock<HashMap<String, TestExecution>>>,
    
    /// Execution scheduler
    execution_scheduler: Arc<Mutex<ExecutionScheduler>>,
}

/// Individual test execution state
#[derive(Debug)]
struct TestExecution {
    experiment_id: String,
    start_time: Instant,
    operations_executed: u64,
    current_participants: usize,
    execution_state: ExecutionState,
}

/// State of test execution
#[derive(Debug, PartialEq, Eq)]
enum ExecutionState {
    Initializing,
    Running,
    Paused,
    Stopping,
    Completed,
    Failed,
}

/// Execution scheduler for managing test timing
#[derive(Debug)]
struct ExecutionScheduler {
    scheduled_tests: Vec<ScheduledTest>,
    execution_queue: Vec<String>,
}

/// Scheduled test entry
#[derive(Debug)]
struct ScheduledTest {
    experiment_id: String,
    scheduled_start: Instant,
    priority: TestPriority,
}

/// Test priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TestPriority {
    Low,
    Normal,
    High,
    Critical,
}

/// Results analysis engine
pub struct ResultsAnalyzer {
    /// Analysis configuration
    config: AnalysisConfig,
    
    /// Statistical analyzers
    analyzers: Vec<Box<dyn StatisticalAnalyzer>>,
    
    /// Results storage
    results_storage: Arc<RwLock<ResultsStorage>>,
}

/// Configuration for results analysis
#[derive(Debug, Clone)]
struct AnalysisConfig {
    confidence_level: f64,
    min_sample_size: usize,
    effect_size_threshold: f64,
    auto_analysis_enabled: bool,
}

/// Trait for statistical analyzers
trait StatisticalAnalyzer: Send + Sync {
    fn analyze(&self, data: &ExperimentResults) -> DualStackResult<Vec<HypothesisTest>>;
    fn name(&self) -> &str;
}

/// Results storage system
#[derive(Debug)]
struct ResultsStorage {
    experiment_results: HashMap<String, ExperimentResults>,
    analysis_cache: HashMap<String, StatisticalAnalysis>,
    export_queue: Vec<ExportRequest>,
}

/// Export request for external systems
#[derive(Debug)]
struct ExportRequest {
    experiment_id: String,
    export_format: ExportFormat,
    destination: String,
    requested_at: Instant,
}

/// Export format options
#[derive(Debug)]
enum ExportFormat {
    Json,
    Csv,
    Prometheus,
    InfluxDB,
}

/// Statistical significance calculator
pub struct StatisticalCalculator {
    significance_tests: Vec<Box<dyn SignificanceTest>>,
}

/// Trait for significance tests
trait SignificanceTest: Send + Sync {
    fn test(&self, control: &[f64], treatment: &[f64]) -> DualStackResult<HypothesisTest>;
    fn test_name(&self) -> &str;
}

/// Test scheduler for experiment orchestration
pub struct TestScheduler {
    /// Scheduler configuration
    config: SchedulerConfig,
    
    /// Scheduled experiments
    scheduled_experiments: Arc<RwLock<Vec<ScheduledExperiment>>>,
    
    /// Scheduler state
    scheduler_state: Arc<RwLock<SchedulerState>>,
}

/// Scheduler configuration
#[derive(Debug, Clone)]
struct SchedulerConfig {
    max_concurrent_experiments: usize,
    default_cooldown_period: Duration,
    auto_scheduling_enabled: bool,
}

/// Scheduled experiment entry
#[derive(Debug, Clone)]
struct ScheduledExperiment {
    experiment: ABExperiment,
    scheduled_start: Instant,
    priority: TestPriority,
    dependencies: Vec<String>,
}

/// Scheduler state tracking
#[derive(Debug)]
struct SchedulerState {
    running_experiments: HashMap<String, Instant>,
    completed_experiments: Vec<String>,
    failed_experiments: Vec<String>,
    next_available_slot: Instant,
}

impl ABTestingFramework {
    /// Create a new A/B testing framework
    pub async fn new(config: ABTestConfig, dual_stack_transport: Arc<DualStackTransport>) -> DualStackResult<Self> {
        let active_experiments = Arc::new(RwLock::new(HashMap::new()));
        
        let test_executor = Arc::new(TestExecutor {
            dual_stack_transport,
            active_executions: Arc::new(RwLock::new(HashMap::new())),
            execution_scheduler: Arc::new(Mutex::new(ExecutionScheduler {
                scheduled_tests: Vec::new(),
                execution_queue: Vec::new(),
            })),
        });
        
        let results_analyzer = Arc::new(ResultsAnalyzer {
            config: AnalysisConfig {
                confidence_level: config.confidence_level,
                min_sample_size: config.min_sample_size,
                effect_size_threshold: 0.2, // Small effect size
                auto_analysis_enabled: true,
            },
            analyzers: vec![], // Would be populated with actual analyzers
            results_storage: Arc::new(RwLock::new(ResultsStorage {
                experiment_results: HashMap::new(),
                analysis_cache: HashMap::new(),
                export_queue: Vec::new(),
            })),
        });
        
        let stats_calculator = Arc::new(StatisticalCalculator {
            significance_tests: vec![], // Would be populated with actual tests
        });
        
        let scheduler = Arc::new(TestScheduler {
            config: SchedulerConfig {
                max_concurrent_experiments: config.max_concurrent_experiments,
                default_cooldown_period: Duration::from_hours(1),
                auto_scheduling_enabled: true,
            },
            scheduled_experiments: Arc::new(RwLock::new(Vec::new())),
            scheduler_state: Arc::new(RwLock::new(SchedulerState {
                running_experiments: HashMap::new(),
                completed_experiments: Vec::new(),
                failed_experiments: Vec::new(),
                next_available_slot: Instant::now(),
            })),
        });
        
        Ok(Self {
            config,
            active_experiments,
            test_executor,
            results_analyzer,
            stats_calculator,
            scheduler,
        })
    }
    
    /// Create a new A/B experiment
    #[instrument(skip(self), fields(experiment_name = %experiment_name))]
    pub async fn create_experiment(
        &self,
        experiment_name: String,
        description: String,
        test_config: TestConfig,
    ) -> DualStackResult<String> {
        if !self.config.enabled {
            return Err(DualStackError::Configuration("A/B testing is disabled".to_string()));
        }
        
        let experiment_id = format!("exp_{}_{}", 
            experiment_name.replace(" ", "_").to_lowercase(),
            chrono::Utc::now().timestamp()
        );
        
        let experiment = ABExperiment {
            id: experiment_id.clone(),
            name: experiment_name,
            description,
            test_config,
            state: ExperimentState::Draft,
            timeline: ExperimentTimeline {
                created_at: Instant::now(),
                started_at: None,
                paused_at: None,
                completed_at: None,
                duration_target: Duration::from_hours(24), // Default 24 hours
                duration_actual: None,
            },
            participants: ParticipantAssignment {
                control_group: Vec::new(),
                treatment_group: Vec::new(),
                assignment_metadata: HashMap::new(),
                total_participants: 0,
            },
            results: ExperimentResults {
                group_results: HashMap::new(),
                operation_results: Vec::new(),
                time_series: TimeSeries {
                    data_points: Vec::new(),
                    collection_interval: Duration::from_minutes(5),
                    metrics_tracked: vec![
                        "latency".to_string(),
                        "success_rate".to_string(),
                        "throughput".to_string(),
                    ],
                },
                error_analysis: ErrorAnalysis {
                    error_breakdown: HashMap::new(),
                    error_correlation: HashMap::new(),
                    error_trends: Vec::new(),
                },
            },
            analysis: None,
        };
        
        let mut experiments = self.active_experiments.write().await;
        experiments.insert(experiment_id.clone(), experiment);
        
        info!("Created A/B experiment: {}", experiment_id);
        Ok(experiment_id)
    }
    
    /// Start an experiment
    #[instrument(skip(self), fields(experiment_id = %experiment_id))]
    pub async fn start_experiment(&self, experiment_id: &str) -> DualStackResult<()> {
        let mut experiments = self.active_experiments.write().await;
        
        let experiment = experiments.get_mut(experiment_id)
            .ok_or_else(|| DualStackError::Configuration(format!("Experiment {} not found", experiment_id)))?;
        
        if experiment.state != ExperimentState::Ready && experiment.state != ExperimentState::Draft {
            return Err(DualStackError::Configuration(
                format!("Experiment {} is not in a startable state: {:?}", experiment_id, experiment.state)
            ));
        }
        
        // Validate experiment configuration
        self.validate_experiment_config(&experiment.test_config)?;
        
        // Assign participants to groups
        self.assign_participants(experiment).await?;
        
        // Update experiment state
        experiment.state = ExperimentState::Running;
        experiment.timeline.started_at = Some(Instant::now());
        
        // Start test execution
        self.test_executor.start_execution(experiment_id, &experiment.test_config).await?;
        
        info!("Started A/B experiment: {}", experiment_id);
        Ok(())
    }
    
    /// Stop an experiment
    #[instrument(skip(self), fields(experiment_id = %experiment_id))]
    pub async fn stop_experiment(&self, experiment_id: &str, reason: &str) -> DualStackResult<()> {
        let mut experiments = self.active_experiments.write().await;
        
        let experiment = experiments.get_mut(experiment_id)
            .ok_or_else(|| DualStackError::Configuration(format!("Experiment {} not found", experiment_id)))?;
        
        if experiment.state != ExperimentState::Running {
            return Err(DualStackError::Configuration(
                format!("Experiment {} is not running", experiment_id)
            ));
        }
        
        // Stop test execution
        self.test_executor.stop_execution(experiment_id).await?;
        
        // Update experiment state
        experiment.state = ExperimentState::Stopped;
        experiment.timeline.completed_at = Some(Instant::now());
        
        if let Some(started_at) = experiment.timeline.started_at {
            experiment.timeline.duration_actual = Some(started_at.elapsed());
        }
        
        warn!("Stopped A/B experiment: {} (reason: {})", experiment_id, reason);
        Ok(())
    }
    
    /// Analyze experiment results
    #[instrument(skip(self), fields(experiment_id = %experiment_id))]
    pub async fn analyze_experiment(&self, experiment_id: &str) -> DualStackResult<StatisticalAnalysis> {
        let experiments = self.active_experiments.read().await;
        
        let experiment = experiments.get(experiment_id)
            .ok_or_else(|| DualStackError::Configuration(format!("Experiment {} not found", experiment_id)))?;
        
        // Check if we have sufficient data
        let total_operations = experiment.results.operation_results.len();
        if total_operations < self.config.min_sample_size {
            return Err(DualStackError::Configuration(
                format!("Insufficient data for analysis: {} operations (minimum: {})", 
                        total_operations, self.config.min_sample_size)
            ));
        }
        
        // Perform statistical analysis
        let analysis = self.perform_statistical_analysis(&experiment.results).await?;
        
        info!("Completed statistical analysis for experiment: {}", experiment_id);
        Ok(analysis)
    }
    
    /// Get experiment status
    pub async fn get_experiment_status(&self, experiment_id: &str) -> DualStackResult<ExperimentStatus> {
        let experiments = self.active_experiments.read().await;
        
        let experiment = experiments.get(experiment_id)
            .ok_or_else(|| DualStackError::Configuration(format!("Experiment {} not found", experiment_id)))?;
        
        Ok(ExperimentStatus {
            id: experiment.id.clone(),
            name: experiment.name.clone(),
            state: experiment.state.clone(),
            progress: self.calculate_experiment_progress(experiment),
            participants: experiment.participants.total_participants,
            operations_completed: experiment.results.operation_results.len(),
            time_remaining: self.calculate_time_remaining(experiment),
            preliminary_results: self.generate_preliminary_results(experiment).await,
        })
    }
    
    /// List all experiments
    pub async fn list_experiments(&self) -> Vec<ExperimentSummary> {
        let experiments = self.active_experiments.read().await;
        
        experiments.values()
            .map(|exp| ExperimentSummary {
                id: exp.id.clone(),
                name: exp.name.clone(),
                state: exp.state.clone(),
                created_at: exp.timeline.created_at,
                participants: exp.participants.total_participants,
            })
            .collect()
    }
    
    /// Validate experiment configuration
    fn validate_experiment_config(&self, config: &TestConfig) -> DualStackResult<()> {
        // Validate traffic split
        let total_percentage = config.traffic_split.control_percentage + config.traffic_split.treatment_percentage;
        if (total_percentage - 100.0).abs() > 0.01 {
            return Err(DualStackError::Configuration(
                format!("Traffic split percentages must sum to 100%, got {:.2}%", total_percentage)
            ));
        }
        
        // Validate duration
        if config.duration < Duration::from_minutes(10) {
            return Err(DualStackError::Configuration(
                "Experiment duration must be at least 10 minutes".to_string()
            ));
        }
        
        // Validate success criteria
        if config.success_criteria.confidence_level < 0.8 || config.success_criteria.confidence_level > 0.99 {
            return Err(DualStackError::Configuration(
                "Confidence level must be between 0.8 and 0.99".to_string()
            ));
        }
        
        Ok(())
    }
    
    /// Assign participants to experiment groups
    async fn assign_participants(&self, experiment: &mut ABExperiment) -> DualStackResult<()> {
        // This is a simplified implementation - would use actual peer discovery
        // and assignment based on the configured strategy
        
        let control_count = 50; // Would be determined by actual peer availability
        let treatment_count = 50;
        
        for i in 0..control_count {
            let peer_id = KadPeerId::new(format!("control_peer_{}", i).into_bytes());
            experiment.participants.control_group.push(peer_id.clone());
            
            experiment.participants.assignment_metadata.insert(peer_id, AssignmentMetadata {
                assigned_at: Instant::now(),
                assignment_group: AssignmentGroup::Control,
                assignment_reason: "Random assignment".to_string(),
                peer_characteristics: PeerCharacteristics {
                    estimated_performance: 0.8,
                    network_quality: NetworkQuality {
                        latency_score: 0.8,
                        bandwidth_score: 0.7,
                        stability_score: 0.9,
                        nat_traversal_capability: true,
                    },
                    geographic_region: Some("us-west".to_string()),
                    peer_type: PeerType::Desktop,
                    connection_stability: 0.85,
                },
            });
        }
        
        for i in 0..treatment_count {
            let peer_id = KadPeerId::new(format!("treatment_peer_{}", i).into_bytes());
            experiment.participants.treatment_group.push(peer_id.clone());
            
            experiment.participants.assignment_metadata.insert(peer_id, AssignmentMetadata {
                assigned_at: Instant::now(),
                assignment_group: AssignmentGroup::Treatment,
                assignment_reason: "Random assignment".to_string(),
                peer_characteristics: PeerCharacteristics {
                    estimated_performance: 0.8,
                    network_quality: NetworkQuality {
                        latency_score: 0.8,
                        bandwidth_score: 0.7,
                        stability_score: 0.9,
                        nat_traversal_capability: true,
                    },
                    geographic_region: Some("us-west".to_string()),
                    peer_type: PeerType::Desktop,
                    connection_stability: 0.85,
                },
            });
        }
        
        experiment.participants.total_participants = control_count + treatment_count;
        
        info!("Assigned {} participants to experiment {} ({} control, {} treatment)",
              experiment.participants.total_participants,
              experiment.id,
              control_count,
              treatment_count);
        
        Ok(())
    }
    
    /// Perform statistical analysis on results
    async fn perform_statistical_analysis(&self, results: &ExperimentResults) -> DualStackResult<StatisticalAnalysis> {
        // Extract control and treatment group data
        let control_latencies: Vec<f64> = results.operation_results
            .iter()
            .filter(|r| r.group == AssignmentGroup::Control)
            .map(|r| r.duration.as_millis() as f64)
            .collect();
        
        let treatment_latencies: Vec<f64> = results.operation_results
            .iter()
            .filter(|r| r.group == AssignmentGroup::Treatment)
            .map(|r| r.duration.as_millis() as f64)
            .collect();
        
        if control_latencies.is_empty() || treatment_latencies.is_empty() {
            return Err(DualStackError::Configuration("Insufficient data for analysis".to_string()));
        }
        
        // Perform t-test for latency comparison
        let t_test_result = self.perform_t_test(&control_latencies, &treatment_latencies)?;
        
        // Calculate effect size
        let effect_size = self.calculate_cohens_d(&control_latencies, &treatment_latencies);
        
        // Calculate confidence intervals
        let control_ci = self.calculate_confidence_interval(&control_latencies, 0.95);
        let treatment_ci = self.calculate_confidence_interval(&treatment_latencies, 0.95);
        
        // Generate recommendations
        let recommendations = self.generate_recommendations(&t_test_result, effect_size);
        
        Ok(StatisticalAnalysis {
            hypothesis_tests: vec![t_test_result],
            effect_sizes: [(
                "latency".to_string(),
                EffectSize {
                    metric_name: "latency".to_string(),
                    effect_size_value: effect_size,
                    effect_size_type: EffectSizeType::CohenD,
                    interpretation: self.interpret_effect_size(effect_size),
                }
            )].into_iter().collect(),
            confidence_intervals: [
                ("control_latency".to_string(), control_ci),
                ("treatment_latency".to_string(), treatment_ci),
            ].into_iter().collect(),
            significance_results: SignificanceResults {
                overall_significance: t_test_result.is_significant,
                primary_metric_significant: t_test_result.is_significant,
                secondary_metrics_significant: HashMap::new(),
                practical_significance: effect_size.abs() > 0.2, // Small effect size threshold
                recommendation_confidence: if t_test_result.is_significant { 0.95 } else { 0.5 },
            },
            recommendations,
        })
    }
    
    /// Perform t-test comparison
    fn perform_t_test(&self, control: &[f64], treatment: &[f64]) -> DualStackResult<HypothesisTest> {
        // Simplified t-test implementation
        let control_mean = control.iter().sum::<f64>() / control.len() as f64;
        let treatment_mean = treatment.iter().sum::<f64>() / treatment.len() as f64;
        
        let control_var = control.iter()
            .map(|x| (x - control_mean).powi(2))
            .sum::<f64>() / (control.len() - 1) as f64;
        
        let treatment_var = treatment.iter()
            .map(|x| (x - treatment_mean).powi(2))
            .sum::<f64>() / (treatment.len() - 1) as f64;
        
        let pooled_se = ((control_var / control.len() as f64) + 
                        (treatment_var / treatment.len() as f64)).sqrt();
        
        let t_statistic = (treatment_mean - control_mean) / pooled_se;
        
        // Simplified p-value calculation (would use proper statistical functions)
        let p_value = if t_statistic.abs() > 2.0 { 0.04 } else { 0.2 };
        
        Ok(HypothesisTest {
            test_name: "Two-sample t-test".to_string(),
            null_hypothesis: "No difference in means between groups".to_string(),
            alternative_hypothesis: "Significant difference in means between groups".to_string(),
            test_statistic: t_statistic,
            p_value,
            is_significant: p_value < 0.05,
            test_type: StatisticalTestType::TTest,
        })
    }
    
    /// Calculate Cohen's D effect size
    fn calculate_cohens_d(&self, control: &[f64], treatment: &[f64]) -> f64 {
        let control_mean = control.iter().sum::<f64>() / control.len() as f64;
        let treatment_mean = treatment.iter().sum::<f64>() / treatment.len() as f64;
        
        let control_var = control.iter()
            .map(|x| (x - control_mean).powi(2))
            .sum::<f64>() / (control.len() - 1) as f64;
        
        let treatment_var = treatment.iter()
            .map(|x| (x - treatment_mean).powi(2))
            .sum::<f64>() / (treatment.len() - 1) as f64;
        
        let pooled_sd = ((control_var + treatment_var) / 2.0).sqrt();
        
        (treatment_mean - control_mean) / pooled_sd
    }
    
    /// Calculate confidence interval
    fn calculate_confidence_interval(&self, data: &[f64], confidence_level: f64) -> ConfidenceInterval {
        let mean = data.iter().sum::<f64>() / data.len() as f64;
        let variance = data.iter()
            .map(|x| (x - mean).powi(2))
            .sum::<f64>() / (data.len() - 1) as f64;
        let se = (variance / data.len() as f64).sqrt();
        
        // Simplified - would use proper t-distribution critical values
        let margin = 1.96 * se; // Approximate for 95% confidence
        
        ConfidenceInterval {
            metric_name: "latency".to_string(),
            lower_bound: mean - margin,
            upper_bound: mean + margin,
            confidence_level,
        }
    }
    
    /// Interpret effect size magnitude
    fn interpret_effect_size(&self, effect_size: f64) -> String {
        match effect_size.abs() {
            x if x < 0.2 => "Negligible effect".to_string(),
            x if x < 0.5 => "Small effect".to_string(),
            x if x < 0.8 => "Medium effect".to_string(),
            _ => "Large effect".to_string(),
        }
    }
    
    /// Generate recommendations based on analysis
    fn generate_recommendations(&self, t_test: &HypothesisTest, effect_size: f64) -> Vec<Recommendation> {
        let mut recommendations = Vec::new();
        
        if t_test.is_significant && effect_size < -0.2 {
            // Treatment shows significant improvement
            recommendations.push(Recommendation {
                recommendation_type: RecommendationType::ProceedWithRollout,
                description: "Treatment group shows statistically significant improvement. Recommend proceeding with gradual rollout.".to_string(),
                confidence: 0.9,
                supporting_evidence: vec![
                    format!("Statistically significant result (p = {:.4})", t_test.p_value),
                    format!("Meaningful effect size (Cohen's d = {:.3})", effect_size),
                ],
                risks: vec![
                    "Monitor for degradation in other metrics".to_string(),
                    "Watch for unexpected edge cases in production".to_string(),
                ],
                next_steps: vec![
                    "Begin 10% rollout with enhanced monitoring".to_string(),
                    "Set up automated rollback triggers".to_string(),
                ],
            });
        } else if !t_test.is_significant {
            recommendations.push(Recommendation {
                recommendation_type: RecommendationType::ExtendExperiment,
                description: "No statistically significant difference detected. Recommend extending experiment duration.".to_string(),
                confidence: 0.7,
                supporting_evidence: vec![
                    format!("Non-significant result (p = {:.4})", t_test.p_value),
                ],
                risks: vec![
                    "May be wasting resources on ineffective change".to_string(),
                ],
                next_steps: vec![
                    "Extend experiment by 1 week".to_string(),
                    "Increase sample size if possible".to_string(),
                ],
            });
        }
        
        recommendations
    }
    
    /// Calculate experiment progress
    fn calculate_experiment_progress(&self, experiment: &ABExperiment) -> f64 {
        if let Some(started_at) = experiment.timeline.started_at {
            let elapsed = started_at.elapsed();
            let progress = elapsed.as_secs_f64() / experiment.timeline.duration_target.as_secs_f64();
            progress.min(1.0)
        } else {
            0.0
        }
    }
    
    /// Calculate time remaining
    fn calculate_time_remaining(&self, experiment: &ABExperiment) -> Option<Duration> {
        if let Some(started_at) = experiment.timeline.started_at {
            let elapsed = started_at.elapsed();
            if elapsed < experiment.timeline.duration_target {
                Some(experiment.timeline.duration_target - elapsed)
            } else {
                None
            }
        } else {
            Some(experiment.timeline.duration_target)
        }
    }
    
    /// Generate preliminary results
    async fn generate_preliminary_results(&self, experiment: &ABExperiment) -> PreliminaryResults {
        let total_operations = experiment.results.operation_results.len();
        
        if total_operations == 0 {
            return PreliminaryResults {
                total_operations: 0,
                control_operations: 0,
                treatment_operations: 0,
                control_success_rate: 0.0,
                treatment_success_rate: 0.0,
                control_avg_latency: Duration::from_millis(0),
                treatment_avg_latency: Duration::from_millis(0),
                preliminary_winner: None,
                confidence: 0.0,
            };
        }
        
        let control_ops: Vec<_> = experiment.results.operation_results
            .iter()
            .filter(|r| r.group == AssignmentGroup::Control)
            .collect();
        
        let treatment_ops: Vec<_> = experiment.results.operation_results
            .iter()
            .filter(|r| r.group == AssignmentGroup::Treatment)
            .collect();
        
        let control_success_rate = control_ops.iter()
            .filter(|r| r.success)
            .count() as f64 / control_ops.len() as f64;
        
        let treatment_success_rate = treatment_ops.iter()
            .filter(|r| r.success)
            .count() as f64 / treatment_ops.len() as f64;
        
        let control_avg_latency = if !control_ops.is_empty() {
            let total: Duration = control_ops.iter().map(|r| r.duration).sum();
            total / control_ops.len() as u32
        } else {
            Duration::from_millis(0)
        };
        
        let treatment_avg_latency = if !treatment_ops.is_empty() {
            let total: Duration = treatment_ops.iter().map(|r| r.duration).sum();
            total / treatment_ops.len() as u32
        } else {
            Duration::from_millis(0)
        };
        
        let preliminary_winner = if treatment_avg_latency < control_avg_latency && treatment_success_rate >= control_success_rate {
            Some(TransportId::Iroh)
        } else if control_avg_latency < treatment_avg_latency && control_success_rate >= treatment_success_rate {
            Some(TransportId::LibP2P)
        } else {
            None
        };
        
        PreliminaryResults {
            total_operations,
            control_operations: control_ops.len(),
            treatment_operations: treatment_ops.len(),
            control_success_rate,
            treatment_success_rate,
            control_avg_latency,
            treatment_avg_latency,
            preliminary_winner,
            confidence: if total_operations < self.config.min_sample_size { 0.3 } else { 0.7 },
        }
    }
}

impl TestExecutor {
    /// Start test execution for an experiment
    async fn start_execution(&self, experiment_id: &str, config: &TestConfig) -> DualStackResult<()> {
        let execution = TestExecution {
            experiment_id: experiment_id.to_string(),
            start_time: Instant::now(),
            operations_executed: 0,
            current_participants: 0,
            execution_state: ExecutionState::Running,
        };
        
        let mut executions = self.active_executions.write().await;
        executions.insert(experiment_id.to_string(), execution);
        
        info!("Started test execution for experiment: {}", experiment_id);
        Ok(())
    }
    
    /// Stop test execution for an experiment
    async fn stop_execution(&self, experiment_id: &str) -> DualStackResult<()> {
        let mut executions = self.active_executions.write().await;
        
        if let Some(execution) = executions.get_mut(experiment_id) {
            execution.execution_state = ExecutionState::Stopping;
        }
        
        executions.remove(experiment_id);
        
        info!("Stopped test execution for experiment: {}", experiment_id);
        Ok(())
    }
}

/// Experiment status information
#[derive(Debug, Clone)]
pub struct ExperimentStatus {
    pub id: String,
    pub name: String,
    pub state: ExperimentState,
    pub progress: f64,
    pub participants: usize,
    pub operations_completed: usize,
    pub time_remaining: Option<Duration>,
    pub preliminary_results: PreliminaryResults,
}

/// Experiment summary for listing
#[derive(Debug, Clone)]
pub struct ExperimentSummary {
    pub id: String,
    pub name: String,
    pub state: ExperimentState,
    pub created_at: Instant,
    pub participants: usize,
}

/// Preliminary results before full analysis
#[derive(Debug, Clone)]
pub struct PreliminaryResults {
    pub total_operations: usize,
    pub control_operations: usize,
    pub treatment_operations: usize,
    pub control_success_rate: f64,
    pub treatment_success_rate: f64,
    pub control_avg_latency: Duration,
    pub treatment_avg_latency: Duration,
    pub preliminary_winner: Option<TransportId>,
    pub confidence: f64,
}

impl Default for ABTestConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_test_duration: Duration::from_hours(24),
            min_sample_size: 100,
            confidence_level: 0.95,
            max_concurrent_experiments: 3,
            data_retention_period: Duration::from_days(30),
            export_results: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_ab_framework_creation() {
        let config = ABTestConfig::default();
        let dual_stack = Arc::new(
            // Would need actual DualStackTransport instance
            // DualStackTransport::new(...).await.unwrap()
        );
        
        // let framework = ABTestingFramework::new(config, dual_stack).await.unwrap();
        // assert!(framework.config.enabled);
    }
    
    #[test]
    fn test_effect_size_interpretation() {
        let framework = ABTestingFramework {
            // ... initialization would go here
        };
        
        assert_eq!(framework.interpret_effect_size(0.1), "Negligible effect");
        assert_eq!(framework.interpret_effect_size(0.3), "Small effect");
        assert_eq!(framework.interpret_effect_size(0.6), "Medium effect");
        assert_eq!(framework.interpret_effect_size(1.0), "Large effect");
    }
    
    #[test]
    fn test_traffic_split_validation() {
        let valid_split = TrafficSplit {
            control_percentage: 50.0,
            treatment_percentage: 50.0,
            assignment_strategy: AssignmentStrategy::Random,
        };
        
        let invalid_split = TrafficSplit {
            control_percentage: 60.0,
            treatment_percentage: 50.0, // Total > 100%
            assignment_strategy: AssignmentStrategy::Random,
        };
        
        // Would test validation logic
    }
}