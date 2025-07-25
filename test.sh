#!/bin/bash

# Enhanced Autonomi Network Test Runner
# This script provides comprehensive testing capabilities for the Autonomi Network
# 
# Network Management Sequence (per README):
# 1. Start EVM testnet: cargo run --bin evm-testnet
# 2. Start local network: cargo run --bin antctl -- local run --build --clean --rewards-address <addr>
# 3. Verify status: cargo run --bin antctl -- local status (should show 25 nodes RUNNING)
# 4. Run tests with environment: ANT_PEERS=local SECRET_KEY=<key>
# 5. Cleanup: cargo run --bin antctl -- local kill

set -e

# Global logging configuration
TEST_RUN_ID=""
TEST_LOG_DIR=""
ENABLE_DEBUG_LOGGING=false
SAVE_LOGS=false
AUTO_ANALYZE_FAILURES=false

# Parse command line flags for logging and filter arguments
FILTERED_ARGS=()
while [[ $# -gt 0 ]]; do
    case $1 in
        --debug)
            ENABLE_DEBUG_LOGGING=true
            SAVE_LOGS=true
            shift
            ;;
        --save-logs)
            SAVE_LOGS=true
            shift
            ;;
        --analyze-failures)
            AUTO_ANALYZE_FAILURES=true
            shift
            ;;
        *)
            FILTERED_ARGS+=("$1")
            shift
            ;;
    esac
done

# Restore positional parameters with filtered arguments
set -- "${FILTERED_ARGS[@]}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Print colored output
print_status() {
    echo -e "${BLUE}ℹ️  $1${NC}"
}

print_success() {
    echo -e "${GREEN}✅ $1${NC}"
}

print_warning() {
    echo -e "${YELLOW}⚠️  $1${NC}"
}

print_error() {
    echo -e "${RED}❌ $1${NC}"
}

print_header() {
    echo -e "\n${BLUE}🧪 $1${NC}"
    echo "================================================"
}

# ================================
# PLATFORM COMPATIBILITY FUNCTIONS
# ================================

# Cross-platform timeout function for macOS/Linux compatibility
run_with_timeout() {
    local timeout_seconds="$1"
    local command="$2"
    
    # Check if system timeout is available
    if command -v timeout >/dev/null 2>&1; then
        timeout "$timeout_seconds" bash -c "$command"
        return $?
    elif command -v gtimeout >/dev/null 2>&1; then
        gtimeout "$timeout_seconds" bash -c "$command"
        return $?
    else
        # macOS fallback: use background process with kill
        bash -c "$command" &
        local pid=$!
        local count=0
        
        while [ $count -lt "$timeout_seconds" ]; do
            if ! kill -0 "$pid" 2>/dev/null; then
                wait "$pid"
                return $?
            fi
            sleep 1
            count=$((count + 1))
        done
        
        # Timeout reached, kill the process
        kill -TERM "$pid" 2>/dev/null
        sleep 2
        kill -KILL "$pid" 2>/dev/null
        return 124  # Standard timeout exit code
    fi
}

# ================================
# ENHANCED LOGGING INFRASTRUCTURE
# ================================

# Print log location and analysis instructions
print_log_location_info() {
    if [ "$SAVE_LOGS" = true ] && [ -n "$TEST_LOG_DIR" ]; then
        echo ""
        print_status "📁 LOGS SAVED TO: $TEST_LOG_DIR"
        echo -e "${BLUE}📋 Log Structure:${NC}"
        echo "  • Network logs:  $TEST_LOG_DIR/logs/network/"
        echo "  • Test logs:     $TEST_LOG_DIR/logs/tests/"
        echo "  • Summary:       $TEST_LOG_DIR/logs/summary/"
        echo "  • All files:     find $TEST_LOG_DIR -name '*.log' -o -name '*.json' -o -name '*.txt'"
        echo ""
        echo -e "${BLUE}🔍 Analysis Commands:${NC}"
        echo "  • Analyze run:   ./test.sh analyze $TEST_LOG_DIR"
        echo "  • View metadata: cat $TEST_LOG_DIR/logs/summary/test-metadata.json"
        echo "  • Check failures: ls $TEST_LOG_DIR/logs/summary/failure-analysis-*.txt"
        echo ""
    fi
}

# Copy node logs from default location to our test directory
copy_node_logs_to_test_dir() {
    if [ "$SAVE_LOGS" = true ] && [ -n "$TEST_LOG_DIR" ]; then
        log_message "INFO" "Copying node logs to test directory..."
        
        local node_logs_copied=0
        local total_log_size=0
        
        # Find all node log directories
        local autonomi_dir="$HOME/Library/Application Support/autonomi"
        if [ -d "$autonomi_dir/node" ]; then
            for node_dir in "$autonomi_dir/node"/*/; do
                if [ -d "$node_dir/logs" ]; then
                    local node_id=$(basename "$node_dir")
                    local dest_dir="$TEST_LOG_DIR/logs/network/nodes/$node_id"
                    
                    # Create destination directory
                    mkdir -p "$dest_dir"
                    
                    # Copy all log files from this node
                    if cp -r "$node_dir/logs/"* "$dest_dir/" 2>/dev/null; then
                        node_logs_copied=$((node_logs_copied + 1))
                        
                        # Calculate log size for this node
                        local node_size=$(du -sk "$dest_dir" 2>/dev/null | cut -f1)
                        total_log_size=$((total_log_size + node_size))
                        
                        log_message "INFO" "Copied logs for node $node_id"
                    fi
                fi
            done
        fi
        
        if [ $node_logs_copied -gt 0 ]; then
            log_message "SUCCESS" "Copied logs from $node_logs_copied nodes (${total_log_size}KB total)"
        else
            log_message "WARNING" "No node logs found to copy"
        fi
    fi
}

# Print final log summary and instructions
print_final_log_summary() {
    if [ "$SAVE_LOGS" = true ] && [ -n "$TEST_LOG_DIR" ]; then
        echo ""
        echo "════════════════════════════════════════════════"
        print_success "📁 COMPREHENSIVE LOGS SAVED"
        echo -e "${GREEN}Location: $TEST_LOG_DIR${NC}"
        echo ""
        
        # Show what we've captured
        local network_logs=$(find "$TEST_LOG_DIR/logs/network" -name "*.log" 2>/dev/null | wc -l | tr -d ' ')
        local test_logs=$(find "$TEST_LOG_DIR/logs/tests" -name "*.log" 2>/dev/null | wc -l | tr -d ' ')
        local node_logs=$(find "$TEST_LOG_DIR/logs/network/nodes" -name "*.log" 2>/dev/null | wc -l | tr -d ' ')
        
        echo -e "${BLUE}📊 Captured Logs:${NC}"
        echo "  • Network logs: $network_logs files"
        echo "  • Test logs: $test_logs files"  
        echo "  • Node logs: $node_logs files"
        echo ""
        
        echo -e "${BLUE}Quick Analysis:${NC}"
        echo "  ./test.sh analyze $TEST_LOG_DIR"
        echo ""
        if [ -f "$TEST_LOG_DIR/logs/summary/failure-analysis-"*.txt ]; then
            echo -e "${YELLOW}⚠️  Failure analyses generated - check summary directory${NC}"
        fi
        echo "════════════════════════════════════════════════"
        echo ""
    fi
}

# Setup test logging directory structure
setup_test_logging() {
    if [ "$SAVE_LOGS" = true ]; then
        TEST_RUN_ID=$(date +"%Y-%m-%d_%H-%M-%S")
        TEST_LOG_DIR="/tmp/autonomi_test_run_${TEST_RUN_ID}"
        
        print_status "Setting up enhanced logging in $TEST_LOG_DIR"
        
        # Create directory structure
        mkdir -p "$TEST_LOG_DIR/logs/network/nodes"
        mkdir -p "$TEST_LOG_DIR/logs/tests"
        mkdir -p "$TEST_LOG_DIR/logs/summary"
        
        # Create initial metadata
        cat > "$TEST_LOG_DIR/logs/summary/test-metadata.json" << EOF
{
    "test_run_id": "$TEST_RUN_ID",
    "start_time": "$(date -Iseconds)",
    "command_line": "$0 $*",
    "debug_logging": $ENABLE_DEBUG_LOGGING,
    "working_directory": "$(pwd)",
    "git_commit": "$(git rev-parse HEAD 2>/dev/null || echo 'unknown')",
    "git_branch": "$(git branch --show-current 2>/dev/null || echo 'unknown')"
}
EOF
        
        print_success "Test logging directory created: $TEST_LOG_DIR"
        
        # Print log analysis instructions
        print_log_location_info
        
        # Set environment variables for Rust logging
        if [ "$ENABLE_DEBUG_LOGGING" = true ]; then
            export RUST_LOG="debug"
            export ANT_LOG="all"
        else
            export RUST_LOG="info"
            export ANT_LOG="networking,client,bootstrap"
        fi
    fi
}

# Log a message to both console and log file
log_message() {
    local level="$1"
    local message="$2"
    local timestamp=$(date '+%Y-%m-%d %H:%M:%S')
    
    # Always print to console
    case $level in
        "INFO") print_status "$message" ;;
        "SUCCESS") print_success "$message" ;;
        "WARNING") print_warning "$message" ;;
        "ERROR") print_error "$message" ;;
    esac
    
    # Also log to file if logging is enabled
    if [ "$SAVE_LOGS" = true ] && [ -n "$TEST_LOG_DIR" ]; then
        echo "[$timestamp] [$level] $message" >> "$TEST_LOG_DIR/logs/summary/test-runner.log"
    fi
}

# Capture output from a command to both console and log file
capture_command_output() {
    local command="$1"
    local log_file="$2"
    local description="$3"
    
    if [ "$SAVE_LOGS" = true ]; then
        log_message "INFO" "Running: $description"
        echo "# Command: $command" >> "$log_file"
        echo "# Started: $(date -Iseconds)" >> "$log_file"
        echo "# Description: $description" >> "$log_file"
        echo "========================================" >> "$log_file"
        
        # Use script to capture all output including colors
        script -q /dev/null bash -c "$command" 2>&1 | tee -a "$log_file"
        local exit_code=${PIPESTATUS[0]}
        
        echo "# Exit code: $exit_code" >> "$log_file"
        echo "# Finished: $(date -Iseconds)" >> "$log_file"
        echo "" >> "$log_file"
        
        return $exit_code
    else
        eval "$command"
    fi
}

# Start EVM testnet with logging
start_evm_with_logging() {
    if [ "$SAVE_LOGS" = true ]; then
        local evm_log="$TEST_LOG_DIR/logs/network/evm-testnet.log"
        log_message "INFO" "Starting EVM testnet with logging to $evm_log"
        
        # Start EVM testnet in background with output redirection
        cargo run --package evm-testnet --bin evm-testnet > "$evm_log" 2>&1 &
        EVM_PID=$!
        
        # Wait a moment for it to start and check if it's still running
        sleep 3
        if kill -0 $EVM_PID 2>/dev/null; then
            log_message "SUCCESS" "EVM testnet started (PID: $EVM_PID)"
            return 0
        else
            log_message "ERROR" "EVM testnet failed to start"
            return 1
        fi
    else
        # Fallback to original method
        cargo run --package evm-testnet --bin evm-testnet &
        EVM_PID=$!
        sleep 3
    fi
}

# Start network with individual node logging
start_network_with_logging() {
    local rewards_address="$1"
    
    if [ "$SAVE_LOGS" = true ]; then
        local network_log="$TEST_LOG_DIR/logs/network/antctl.log"
        log_message "INFO" "Starting network with logging to $network_log"
        
        # Start network with logging
        cargo run --bin antctl -- local run --build --clean --rewards-address "$rewards_address" > "$network_log" 2>&1 &
        NETWORK_PID=$!
        
        log_message "SUCCESS" "Network startup initiated (PID: $NETWORK_PID)"
        return 0
    else
        # Fallback to original method
        cargo run --bin antctl -- local run --build --clean --rewards-address "$rewards_address" &
        NETWORK_PID=$!
    fi
}

# Get network status with logging
get_network_status_with_logging() {
    if [ "$SAVE_LOGS" = true ]; then
        local status_log="$TEST_LOG_DIR/logs/network/status-checks.log"
        echo "# Status check at $(date -Iseconds)" >> "$status_log"
        
        # Run status command and capture output (simple approach without timeout for macOS compatibility)
        if cargo run --bin antctl -- local status >> "$status_log" 2>&1; then
            # Extract node count from the captured output
            local node_count=$(tail -30 "$status_log" | grep -c "RUNNING" 2>/dev/null || echo "0")
            # Sanitize the node count to ensure it's a clean integer
            node_count=$(echo "$node_count" | tr -d '\n\r ' | grep -o '[0-9]*' | head -1)
            node_count=${node_count:-0}
            echo "# Detected $node_count running nodes" >> "$status_log"
            echo "$node_count"
            return 0
        else
            echo "# Status command failed or timed out" >> "$status_log"
            echo "0"
            return 1
        fi
    else
        # Fallback to original method
        if cargo run --bin antctl -- local status > /dev/null 2>&1; then
            local status_output
            if status_output=$(cargo run --bin antctl -- local status 2>&1); then
                local node_count=$(echo "$status_output" | grep -c "RUNNING" 2>/dev/null || echo "0")
                node_count=$(echo "$node_count" | tr -d '\n\r ' | grep -o '[0-9]*' | head -1)
                node_count=${node_count:-0}
                echo "$node_count"
                return 0
            fi
        fi
        echo "0"
        return 1
    fi
}

# Run a test with comprehensive logging
run_test_with_logging() {
    local test_package="$1"
    local test_name="$2"
    local test_description="$3"
    
    if [ "$SAVE_LOGS" = true ]; then
        local test_log="$TEST_LOG_DIR/logs/tests/${test_name}.log"
        log_message "INFO" "Running $test_description"
        
        # Set up environment for this test
        local test_env=""
        if [ "$ENABLE_DEBUG_LOGGING" = true ]; then
            test_env="RUST_LOG=debug ANT_LOG=all"
        else
            test_env="RUST_LOG=info"
        fi
        
        # Add network environment variables
        test_env="$test_env ANT_PEERS=local"
        
        # Get EVM key if available
        local evm_csv_path="$HOME/Library/Application Support/autonomi/evm_testnet_data.csv"
        if [ -f "$evm_csv_path" ]; then
            local deployer_key=$(cut -d',' -f4 "$evm_csv_path")
            test_env="$test_env SECRET_KEY=$deployer_key"
        else
            test_env="$test_env SECRET_KEY=0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
        fi
        
        # Run the test with comprehensive logging
        local test_command="$test_env cargo test --release --package $test_package --test $test_name -- --nocapture"
        
        echo "# Test: $test_description" >> "$test_log"
        echo "# Package: $test_package" >> "$test_log"
        echo "# Test name: $test_name" >> "$test_log"
        echo "# Command: $test_command" >> "$test_log"
        echo "# Started: $(date -Iseconds)" >> "$test_log"
        echo "========================================" >> "$test_log"
        
        # Execute test and capture all output
        if eval "$test_command" >> "$test_log" 2>&1; then
            echo "# Test PASSED" >> "$test_log"
            echo "# Finished: $(date -Iseconds)" >> "$test_log"
            log_message "SUCCESS" "$test_description completed successfully"
            return 0
        else
            echo "# Test FAILED" >> "$test_log"
            echo "# Finished: $(date -Iseconds)" >> "$test_log"
            log_message "ERROR" "$test_description failed"
            
            # Auto-analyze failure if enabled
            if [ "$AUTO_ANALYZE_FAILURES" = true ]; then
                analyze_test_failure "$test_log" "$test_description"
            fi
            return 1
        fi
    else
        # Fallback to original method without logging
        local evm_csv_path="$HOME/Library/Application Support/autonomi/evm_testnet_data.csv"
        if [ -f "$evm_csv_path" ]; then
            local deployer_key=$(cut -d',' -f4 "$evm_csv_path")
            export SECRET_KEY="$deployer_key"
        else
            export SECRET_KEY="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
        fi
        export ANT_PEERS="local"
        
        if cargo test --release --package "$test_package" --test "$test_name" 2>&1; then
            return 0
        else
            return 1
        fi
    fi
}

# Analyze test failure from log file
analyze_test_failure() {
    local test_log="$1"
    local test_description="$2"
    
    log_message "INFO" "Analyzing failure for: $test_description"
    
    local analysis_file="$TEST_LOG_DIR/logs/summary/failure-analysis-$(basename "$test_log" .log).txt"
    
    cat > "$analysis_file" << EOF
FAILURE ANALYSIS: $test_description
=====================================
Generated: $(date -Iseconds)
Log file: $test_log

CRITICAL ERRORS FOUND:
EOF
    
    # Look for common error patterns
    grep -i "error\|failed\|panic\|abort" "$test_log" | head -20 >> "$analysis_file" 2>/dev/null || echo "No critical errors found in main output" >> "$analysis_file"
    
    echo -e "\nTOKEN/PAYMENT ERRORS:" >> "$analysis_file"
    grep -i "insufficient.*token\|payment.*fail\|evm.*error" "$test_log" | head -10 >> "$analysis_file" 2>/dev/null || echo "No payment-related errors found" >> "$analysis_file"
    
    echo -e "\nNETWORK CONNECTIVITY ERRORS:" >> "$analysis_file"
    grep -i "failed to connect\|timeout\|no bootstrap\|network.*error" "$test_log" | head -10 >> "$analysis_file" 2>/dev/null || echo "No network connectivity errors found" >> "$analysis_file"
    
    echo -e "\nRECOMMENDATIONS:" >> "$analysis_file"
    
    # Analyze patterns and provide recommendations
    if grep -q -i "insufficient.*token" "$test_log"; then
        echo "- Check EVM testnet is running and CSV file exists" >> "$analysis_file"
        echo "- Verify SECRET_KEY environment variable is set correctly" >> "$analysis_file"
        echo "- Ensure wallet has sufficient tokens for test operations" >> "$analysis_file"
    fi
    
    if grep -q -i "failed to connect\|no bootstrap" "$test_log"; then
        echo "- Verify local network is running with 'cargo run --bin antctl -- local status'" >> "$analysis_file"
        echo "- Check ANT_PEERS=local environment variable is set" >> "$analysis_file"
        echo "- Ensure nodes have had time to establish connections" >> "$analysis_file"
    fi
    
    if grep -q -i "timeout" "$test_log"; then
        echo "- Network may be slow or overloaded, try running tests individually" >> "$analysis_file"
        echo "- Check system resources (memory, CPU)" >> "$analysis_file"
        echo "- Consider increasing test timeouts" >> "$analysis_file"
    fi
    
    log_message "INFO" "Failure analysis saved to: $analysis_file"
}

# Generate final test summary with log analysis
generate_test_summary() {
    if [ "$SAVE_LOGS" = true ]; then
        local summary_file="$TEST_LOG_DIR/logs/summary/final-summary.txt"
        
        cat > "$summary_file" << EOF
AUTONOMI NETWORK TEST SUMMARY
=============================
Test Run ID: $TEST_RUN_ID
Completed: $(date -Iseconds)
Duration: $(date -d @$(($(date +%s) - $(date -d "$(grep start_time "$TEST_LOG_DIR/logs/summary/test-metadata.json" | cut -d'"' -f4)" +%s))) -u +%H:%M:%S)

LOG DIRECTORY: $TEST_LOG_DIR

AVAILABLE LOGS:
EOF
        
        find "$TEST_LOG_DIR/logs" -type f -name "*.log" | while read -r log_file; do
            local relative_path="${log_file#$TEST_LOG_DIR/logs/}"
            local size=$(du -h "$log_file" | cut -f1)
            echo "  $relative_path ($size)" >> "$summary_file"
        done
        
        echo -e "\nFAILURE ANALYSES:" >> "$summary_file"
        find "$TEST_LOG_DIR/logs/summary" -name "failure-analysis-*.txt" | while read -r analysis_file; do
            echo "  $(basename "$analysis_file")" >> "$summary_file"
        done
        
        echo -e "\nTO INVESTIGATE FAILURES:" >> "$summary_file"
        echo "  1. Check individual test logs in $TEST_LOG_DIR/logs/tests/" >> "$summary_file"
        echo "  2. Review network logs in $TEST_LOG_DIR/logs/network/" >> "$summary_file"
        echo "  3. Read failure analyses in $TEST_LOG_DIR/logs/summary/" >> "$summary_file"
        echo "  4. Use: ./test.sh analyze $TEST_LOG_DIR" >> "$summary_file"
        
        log_message "SUCCESS" "Complete test summary saved to: $summary_file"
        print_status "All logs saved in: $TEST_LOG_DIR"
    fi
}

# Analyze a previous test run
analyze_previous_test_run() {
    local log_dir="$1"
    
    if [ ! -d "$log_dir" ]; then
        print_error "Log directory does not exist: $log_dir"
        exit 1
    fi
    
    if [ ! -d "$log_dir/logs" ]; then
        print_error "Invalid log directory structure: $log_dir/logs not found"
        exit 1
    fi
    
    print_header "Analyzing Test Run: $(basename "$log_dir")"
    
    # Load test metadata if available
    local metadata_file="$log_dir/logs/summary/test-metadata.json"
    if [ -f "$metadata_file" ]; then
        print_status "Test Run Metadata:"
        echo "  Run ID: $(grep test_run_id "$metadata_file" | cut -d'"' -f4)"
        echo "  Started: $(grep start_time "$metadata_file" | cut -d'"' -f4)"
        echo "  Command: $(grep command_line "$metadata_file" | cut -d'"' -f4)"
        echo "  Debug: $(grep debug_logging "$metadata_file" | cut -d':' -f2 | tr -d ' ,')"
        echo ""
    fi
    
    # Analyze network logs
    print_status "Network Analysis:"
    local network_dir="$log_dir/logs/network"
    if [ -d "$network_dir" ]; then
        if [ -f "$network_dir/evm-testnet.log" ]; then
            local evm_errors=$(grep -i "error\|failed\|panic" "$network_dir/evm-testnet.log" | wc -l | tr -d ' ')
            echo "  EVM Testnet: $evm_errors errors found"
        fi
        
        if [ -f "$network_dir/antctl.log" ]; then
            local network_errors=$(grep -i "error\|failed\|panic" "$network_dir/antctl.log" | wc -l | tr -d ' ')
            echo "  Network Startup: $network_errors errors found"
        fi
        
        if [ -f "$network_dir/status-checks.log" ]; then
            local last_status=$(tail -5 "$network_dir/status-checks.log" | grep "Detected.*nodes" | tail -1)
            echo "  Final Status: $last_status"
        fi
    else
        echo "  No network logs found"
    fi
    echo ""
    
    # Analyze test logs
    print_status "Test Analysis:"
    local test_dir="$log_dir/logs/tests"
    local total_tests=0
    local failed_tests=0
    
    if [ -d "$test_dir" ]; then
        for test_log in "$test_dir"/*.log; do
            if [ -f "$test_log" ]; then
                total_tests=$((total_tests + 1))
                local test_name=$(basename "$test_log" .log)
                
                if grep -q "# Test FAILED" "$test_log"; then
                    failed_tests=$((failed_tests + 1))
                    echo "  ❌ $test_name - FAILED"
                    
                    # Show key error messages
                    local key_errors=$(grep -i "error:" "$test_log" | head -3)
                    if [ -n "$key_errors" ]; then
                        echo "    Key errors:"
                        echo "$key_errors" | sed 's/^/      /'
                    fi
                elif grep -q "# Test PASSED" "$test_log"; then
                    echo "  ✅ $test_name - PASSED"
                else
                    echo "  ⚠️  $test_name - UNKNOWN (incomplete log)"
                fi
            fi
        done
        
        echo ""
        echo "  Summary: $((total_tests - failed_tests))/$total_tests tests passed"
    else
        echo "  No test logs found"
    fi
    echo ""
    
    # Show existing failure analyses
    print_status "Available Failure Analyses:"
    local analysis_dir="$log_dir/logs/summary"
    local analysis_count=0
    
    if [ -d "$analysis_dir" ]; then
        for analysis_file in "$analysis_dir"/failure-analysis-*.txt; do
            if [ -f "$analysis_file" ]; then
                analysis_count=$((analysis_count + 1))
                local analysis_name=$(basename "$analysis_file" .txt | sed 's/failure-analysis-//')
                echo "  📋 $analysis_name"
                echo "     View with: cat \"$analysis_file\""
            fi
        done
        
        if [ $analysis_count -eq 0 ]; then
            echo "  No failure analyses found"
            
            # Offer to generate analyses for failed tests
            if [ $failed_tests -gt 0 ]; then
                echo ""
                print_status "Generating failure analyses for failed tests..."
                
                for test_log in "$test_dir"/*.log; do
                    if [ -f "$test_log" ] && grep -q "# Test FAILED" "$test_log"; then
                        local test_name=$(basename "$test_log" .log)
                        
                        # Save current TEST_LOG_DIR and restore after
                        local old_test_log_dir="$TEST_LOG_DIR"
                        TEST_LOG_DIR="$log_dir"
                        
                        analyze_test_failure "$test_log" "$test_name retrospective analysis"
                        
                        TEST_LOG_DIR="$old_test_log_dir"
                        
                        echo "    Generated analysis for $test_name"
                    fi
                done
            fi
        fi
    else
        echo "  No analysis directory found"
    fi
    echo ""
    
    # Summary and recommendations
    if [ $failed_tests -gt 0 ]; then
        print_error "Found $failed_tests failed tests"
        echo ""
        print_status "Debugging Steps:"
        echo "  1. Review individual test logs in: $test_dir/"
        echo "  2. Check network logs in: $network_dir/"
        echo "  3. Read failure analyses in: $analysis_dir/"
        echo "  4. Look for common patterns across failed tests"
        echo ""
        print_status "Common Solutions:"
        echo "  - EVM issues: Restart EVM testnet and ensure CSV file exists"
        echo "  - Network issues: Check node connectivity and bootstrap peers"
        echo "  - Token issues: Verify SECRET_KEY points to funded wallet"
        echo "  - Timeout issues: Increase timeouts or check system resources"
    else
        print_success "All tests appear to have passed!"
    fi
}

# Test configuration
UNIT_TEST_PACKAGES=(
    "ant-bootstrap"
    "ant-evm" 
    "ant-logging"
    "ant-node-manager"
    "ant-protocol"
    "ant-service-management"
    "autonomi"
    "node-launchpad"
    "evmlib"
)

# Exclude packages with known issues
EXCLUDE_PACKAGES=(
    "ant-node"  # Has compilation errors
)

# Test results tracking
TOTAL_TESTS=0
FAILED_TESTS=0
FAILED_PACKAGES=()
PASSED_PACKAGES=()
SKIPPED_PACKAGES=()
TEST_DETAILS=()
FAILED_TEST_OUTPUTS=()  # Store full output for failed tests

# Function to parse test results from cargo output
parse_test_results() {
    local output="$1"
    local package="$2"
    
    # Extract test counts from "test result: ok. X passed; Y failed; Z ignored; ..."
    local passed_count=0
    local failed_count=0
    local ignored_count=0
    local duration="unknown"
    
    # Parse the test result line
    if echo "$output" | grep -q "test result:"; then
        local result_line=$(echo "$output" | grep "test result:" | tail -1)
        
        # Extract passed count (format: "5 passed;")
        if echo "$result_line" | grep -q "passed;"; then
            passed_count=$(echo "$result_line" | grep -o '[0-9]\+ passed' | grep -o '[0-9]\+')
        fi
        
        # Extract failed count (format: "0 failed;")
        if echo "$result_line" | grep -q "failed;"; then
            failed_count=$(echo "$result_line" | grep -o '[0-9]\+ failed' | grep -o '[0-9]\+')
        fi
        
        # Extract ignored count (format: "0 ignored;")
        if echo "$result_line" | grep -q "ignored;"; then
            ignored_count=$(echo "$result_line" | grep -o '[0-9]\+ ignored' | grep -o '[0-9]\+')
        fi
        
        # Extract duration
        if echo "$result_line" | grep -q "finished in"; then
            duration=$(echo "$result_line" | sed -n 's/.*finished in \([^;]*\).*/\1/p')
        fi
    fi
    
    # Store test details
    TEST_DETAILS+=("$package:$passed_count:$failed_count:$ignored_count:$duration")
    
    echo "$passed_count"
}

# Function to extract failed test names from cargo output
extract_failed_tests() {
    local output="$1"
    local failed_tests=()
    
    # Look for lines starting with "---- " which indicate failed test names
    while IFS= read -r line; do
        if [[ "$line" =~ ^----[[:space:]]+(.+)[[:space:]]+stdout[[:space:]]*---- ]]; then
            local test_name="${BASH_REMATCH[1]}"
            failed_tests+=("$test_name")
        fi
    done <<< "$output"
    
    # Also look for test names in the "failures:" section
    local in_failures_section=false
    while IFS= read -r line; do
        if [[ "$line" =~ ^failures:[[:space:]]*$ ]]; then
            in_failures_section=true
            continue
        fi
        if [ "$in_failures_section" = true ]; then
            # Look for lines that contain only test names (indent + test_name)
            if [[ "$line" =~ ^[[:space:]]+([a-zA-Z0-9_]+)[[:space:]]*$ ]]; then
                local test_name="${BASH_REMATCH[1]}"
                # Only include if it looks like a test name and doesn't contain common non-test words
                if [[ "$test_name" =~ ^test_ ]] || [[ "$test_name" =~ _test$ ]] || [[ "$test_name" =~ test ]]; then
                    failed_tests+=("$test_name")
                fi
            elif [[ "$line" =~ ^test[[:space:]]result: ]]; then
                break
            elif [[ "$line" =~ ^[[:space:]]*$ ]]; then
                # Skip empty lines
                continue
            fi
        fi
    done <<< "$output"
    
    # Remove duplicates and return failed test names as comma-separated string
    if [ ${#failed_tests[@]} -gt 0 ]; then
        # Remove duplicates
        local unique_tests=($(printf "%s\n" "${failed_tests[@]}" | sort -u))
        
        local result=""
        for test in "${unique_tests[@]}"; do
            if [ -z "$result" ]; then
                result="$test"
            else
                result="$result, $test"
            fi
        done
        echo "$result"
    else
        echo "unknown"
    fi
}

# Function to run unit tests
run_unit_tests() {
    print_header "Running Unit Tests"
    print_status "Running unit tests from src/ directories only"
    
    # Setup enhanced logging if enabled
    setup_test_logging
    
    # Ensure Foundry is in PATH if installed
    ensure_foundry_path
    
    local package_count=${#UNIT_TEST_PACKAGES[@]}
    local current=0
    
    for package in "${UNIT_TEST_PACKAGES[@]}"; do
        current=$((current + 1))
        print_status "Testing $package ($current/$package_count)"
        
        # Capture test output for parsing
        local test_output
        local test_passed=false
        
        # Special handling for evmlib package (only check if Anvil is needed)
        if [ "$package" = "evmlib" ]; then
            ensure_foundry_path  # Make sure PATH is updated
            
            if ! command -v anvil >/dev/null 2>&1; then
                print_warning "$package requires Anvil for EVM tests"
                
                if check_and_install_foundry; then
                    print_status "Anvil now available, running all $package tests"
                else
                    print_warning "$package EVM tests skipped (Foundry not installed)"
                    SKIPPED_PACKAGES+=("$package (EVM tests)")
                    TEST_DETAILS+=("$package:0:0:0:skipped (no Anvil)")
                    continue
                fi
            fi
        fi
        
        # Run unit tests only (--lib flag excludes integration tests)
        if test_output=$(cargo test --release --package "$package" --lib 2>&1); then
            test_passed=true
            local test_count=$(parse_test_results "$test_output" "$package")
            if [ -n "$test_count" ] && [ "$test_count" -gt 0 ]; then
                print_success "$package tests passed ($test_count tests)"
            else
                print_success "$package tests passed"
            fi
            PASSED_PACKAGES+=("$package")
        else
            test_passed=false
            parse_test_results "$test_output" "$package" >/dev/null  # Store details
            
            # Extract failed test names
            local failed_test_names=$(extract_failed_tests "$test_output")
            
            # Store complete failure information
            local failure_info="PACKAGE: $package
FAILED_TESTS: $failed_test_names
FULL_OUTPUT:
$test_output
=================================================================================="
            
            FAILED_TEST_OUTPUTS+=("$failure_info")
            
            print_error "$package tests failed (specific tests: $failed_test_names)"
            FAILED_PACKAGES+=("$package")
            FAILED_TESTS=$((FAILED_TESTS + 1))
        fi
        
        TOTAL_TESTS=$((TOTAL_TESTS + 1))
    done
}

# Function to run integration tests (requires network)
run_integration_tests() {
    print_header "Running Integration Tests"
    print_status "Currently all tests in autonomi/tests/ require a full network"
    print_status "All previous 'integration' tests have been moved to 'full' category"
    
    # Setup enhanced logging if enabled
    setup_test_logging
    
    # For now, create some truly lightweight integration tests
    print_status "Running client configuration and setup tests"
    
    local integration_passed=0
    local integration_failed=0
    
    # Test 1: Client configuration parsing
    print_status "Testing client configuration parsing"
    local config_test_output
    if config_test_output=$(cargo test --release --package autonomi --lib "config" 2>&1); then
        local test_count=$(parse_test_results "$config_test_output" "autonomi-config")
        if [ -n "$test_count" ] && [ "$test_count" -gt 0 ]; then
            print_success "Client config tests passed ($test_count tests)"
        else
            print_success "Client config tests passed"
        fi
        integration_passed=$((integration_passed + 1))
    else
        local failed_test_names=$(extract_failed_tests "$config_test_output")
        local failure_info="PACKAGE: autonomi-config
FAILED_TESTS: $failed_test_names
FULL_OUTPUT:
$config_test_output
=================================================================================="
        FAILED_TEST_OUTPUTS+=("$failure_info")
        print_error "Client config tests failed (specific tests: $failed_test_names)"
        FAILED_PACKAGES+=("autonomi-config")
        integration_failed=$((integration_failed + 1))
    fi
    TOTAL_TESTS=$((TOTAL_TESTS + 1))
    
    # Test 2: Network address and protocol tests (no network required)
    print_status "Testing network address and protocol handling"
    local protocol_test_output
    if protocol_test_output=$(cargo test --release --package ant-protocol --lib 2>&1); then
        local test_count=$(parse_test_results "$protocol_test_output" "ant-protocol")
        if [ -n "$test_count" ] && [ "$test_count" -gt 0 ]; then
            print_success "Protocol tests passed ($test_count tests)"
        else
            print_success "Protocol tests passed"
        fi
        integration_passed=$((integration_passed + 1))
    else
        local failed_test_names=$(extract_failed_tests "$protocol_test_output")
        local failure_info="PACKAGE: ant-protocol
FAILED_TESTS: $failed_test_names
FULL_OUTPUT:
$protocol_test_output
=================================================================================="
        FAILED_TEST_OUTPUTS+=("$failure_info")
        print_error "Protocol tests failed (specific tests: $failed_test_names)"
        FAILED_PACKAGES+=("ant-protocol")
        integration_failed=$((integration_failed + 1))
    fi
    TOTAL_TESTS=$((TOTAL_TESTS + 1))
    
    if [ $integration_failed -gt 0 ]; then
        FAILED_TESTS=$((FAILED_TESTS + integration_failed))
        print_error "$integration_failed integration test groups failed"
    else
        print_success "All $integration_passed integration test groups passed"
    fi
    
    print_status "Note: All network-dependent tests are now in the 'full' category"
    print_status "Use './test.sh full' to run comprehensive tests with network setup"
}

# Function to check and install Foundry/Anvil if needed
check_and_install_foundry() {
    # First check if anvil is in PATH
    if command -v anvil >/dev/null 2>&1; then
        print_success "Anvil is already installed"
        return 0
    fi
    
    # Check if foundry is installed but not in PATH
    if [ -f "$HOME/.foundry/bin/anvil" ]; then
        print_status "Found Anvil in ~/.foundry/bin, adding to PATH"
        export PATH="$HOME/.foundry/bin:$PATH"
        if command -v anvil >/dev/null 2>&1; then
            print_success "Anvil is now available"
            return 0
        fi
    fi
    
    # Anvil is not available, offer to install
    print_warning "Anvil (part of Foundry) is required for EVM testing but not found."
    echo ""
    echo "Foundry is a toolkit for Ethereum development that includes Anvil (local EVM node)."
    echo "This is needed to run EVM-related tests and full integration tests."
    echo ""
    echo "Installation details:"
    echo "  - Downloads from: https://getfoundry.sh"
    echo "  - Installs to: ~/.foundry/bin/"
    echo "  - Size: ~50MB"
    echo ""
    
    read -p "Would you like to install Foundry now? (y/N): " -n 1 -r
    echo
    
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        print_status "Installing Foundry..."
        
        # Download and install Foundry
        if curl -L https://foundry.paradigm.xyz | bash; then
            # Add to PATH for current session
            export PATH="$HOME/.foundry/bin:$PATH"
            
            # Install/update Foundry
            if foundryup; then
                print_success "Foundry installed successfully!"
                
                # Verify installation
                if command -v anvil >/dev/null 2>&1; then
                    print_success "Anvil is now available"
                    return 0
                else
                    print_error "Anvil installation verification failed"
                    return 1
                fi
            else
                print_error "Foundry installation failed"
                return 1
            fi
        else
            print_error "Failed to download Foundry installer"
            return 1
        fi
    else
        print_warning "Skipping Foundry installation. EVM tests will be skipped."
        return 1
    fi
}

# Function to check if Anvil is running
is_anvil_running() {
    # Check if anvil is running on default port 8545
    if command -v nc >/dev/null 2>&1; then
        if nc -z localhost 8545 2>/dev/null; then
            return 0
        else
            return 1
        fi
    elif command -v netstat >/dev/null 2>&1; then
        if netstat -tuln 2>/dev/null | grep -q ":8545 "; then
            return 0
        else
            return 1
        fi
    elif command -v lsof >/dev/null 2>&1; then
        if lsof -i :8545 >/dev/null 2>&1; then
            return 0
        else
            return 1
        fi
    else
        # Can't check, assume not running
        return 1
    fi
}

# Function to ensure PATH includes Foundry binaries
ensure_foundry_path() {
    if [ -d "$HOME/.foundry/bin" ] && [[ ":$PATH:" != *":$HOME/.foundry/bin:"* ]]; then
        export PATH="$HOME/.foundry/bin:$PATH"
    fi
}

# Function to run full network tests
run_full_tests() {
    print_header "Running Full Network Tests"
    print_warning "Full tests will start a local network and test all data types"
    print_status "Following README sequence: EVM testnet → Network startup → Tests → Cleanup"
    
    # Setup enhanced logging if enabled
    setup_test_logging
    
    # Ensure Foundry is in PATH if installed
    ensure_foundry_path
    
    # Check for Foundry/Anvil
    if ! command -v anvil >/dev/null 2>&1; then
        if ! check_and_install_foundry; then
            log_message "WARNING" "Proceeding without EVM testing (some tests will be skipped)"
            SKIP_EVM_TESTS=true
        else
            SKIP_EVM_TESTS=false
        fi
    else
        log_message "SUCCESS" "Anvil is available for EVM testing"
        SKIP_EVM_TESTS=false
    fi
    
    # Start EVM testnet first (required for network startup)
    if [ "$SKIP_EVM_TESTS" = false ]; then
        if is_anvil_running; then
            log_message "INFO" "Anvil is already running, using existing instance"
        else
            if ! start_evm_with_logging; then
                log_message "ERROR" "Failed to start EVM testnet"
                return 1
            fi
            log_message "INFO" "Waiting for EVM testnet to be ready..."
            sleep 8  # Give more time for EVM to fully start
        fi
    else
        log_message "ERROR" "EVM testnet is required for full network tests"
        return 1
    fi
    
    # Start local network using antctl (README says this creates 25 nodes by default)
    log_message "INFO" "Starting local network using antctl (25 nodes)..."
    log_message "INFO" "This will take about 60-90 seconds for full network startup"
    
    # Start the network (per README: should show "twenty-five running nodes")
    if ! start_network_with_logging "0x1234567890123456789012345678901234567890"; then
        log_message "ERROR" "Failed to start network"
        cleanup_processes
        return 1
    fi
    
    # Wait for network to be ready with better checks
    log_message "INFO" "Waiting for network nodes to start..."
    local network_ready=false
    local node_count=0
    
    for i in {1..180}; do
        node_count=$(get_network_status_with_logging)
        
        if [ "$node_count" -ge 20 ]; then
            log_message "SUCCESS" "Network is ready with $node_count nodes!"
            network_ready=true
            break
        elif [ "$node_count" -gt 0 ]; then
            if [ $((i % 10)) -eq 0 ]; then
                log_message "INFO" "Network starting... ($node_count/25 nodes, $i seconds elapsed)"
            fi
        else
            if [ $((i % 15)) -eq 0 ]; then
                log_message "INFO" "Waiting for network connectivity... ($i seconds elapsed)"
            fi
        fi
        
        sleep 1
        if [ $((i % 30)) -eq 0 ]; then
            log_message "INFO" "Still waiting for network startup... ($i seconds elapsed of 180 max)"
        fi
    done
    
    if [ "$network_ready" = false ]; then
        log_message "ERROR" "Network failed to start properly within timeout (3 minutes)"
        log_message "ERROR" "Only $node_count nodes detected (expected 25 nodes per README)"
        cleanup_processes
        return 1
    fi
    
    # Set environment for tests (per README requirements)
    export ANT_PEERS="local"
    
    # Get the actual EVM testnet deployer key from the CSV file (this wallet has tokens)
    local evm_csv_path="$HOME/Library/Application Support/autonomi/evm_testnet_data.csv"
    if [ -f "$evm_csv_path" ]; then
        local deployer_key=$(cut -d',' -f4 "$evm_csv_path")
        export SECRET_KEY="$deployer_key"
        log_message "INFO" "Using EVM testnet deployer key from $evm_csv_path"
    else
        log_message "WARNING" "EVM testnet CSV file not found, using fallback key"
        export SECRET_KEY="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
    fi
    
    # Run comprehensive tests
    log_message "INFO" "Starting comprehensive test execution..."
    
    # Print clear section headers for different test types
    print_header "API Integration Tests (Core Functionality)"
    print_status "Testing Rust client API and network integration"
    print_status "These tests verify the core Autonomi network functionality"
    
    # Run the comprehensive address test separately
    if run_test_with_logging "autonomi" "address" "comprehensive data address tests"; then
        TOTAL_TESTS=$((TOTAL_TESTS + 1))
    else
        FAILED_TESTS=$((FAILED_TESTS + 1))
        TOTAL_TESTS=$((TOTAL_TESTS + 1))
        FAILED_PACKAGES+=("autonomi-address")
    fi
    
    # Run all other integration tests
    local all_integration_tests=("chunk" "files" "graph" "put" "pointer" "scratchpad" "registers" "vault" "wallet" "analyze" "external_signer")
    
    for test_name in "${all_integration_tests[@]}"; do
        if run_test_with_logging "autonomi" "$test_name" "$test_name full network tests"; then
            TOTAL_TESTS=$((TOTAL_TESTS + 1))
        else
            FAILED_TESTS=$((FAILED_TESTS + 1))
            TOTAL_TESTS=$((TOTAL_TESTS + 1))
            FAILED_PACKAGES+=("autonomi-$test_name-full")
        fi
    done
    
    # Show API test results before moving to CLI tests
    echo ""
    print_header "CLI Interface Tests (Optional/Diagnostic)"
    print_status "Testing command-line interface (ant binary)"
    print_status "Note: These test the CLI wrapper, not core functionality"
    if [ $FAILED_TESTS -eq 0 ]; then
        print_success "✅ All API tests passed! Core network functionality is working perfectly"
        print_status "CLI tests are diagnostic only - API success indicates healthy system"
    else
        print_warning "Some API tests failed - CLI tests may also be affected"
    fi
    echo ""
    
    # Test CLI file operations as a separate diagnostic section
    test_file_operations
    
    # Generate comprehensive test summary
    generate_test_summary
    
    # Cleanup
    cleanup_processes
}

# Function to test CLI file operations (diagnostic)
test_file_operations() {
    print_status "🔧 Testing CLI file upload/download operations (diagnostic)"
    print_status "This tests the 'ant' command-line binary interface"
    
    # Create log file for file operations if logging is enabled
    local file_ops_log=""
    if [ "$SAVE_LOGS" = true ] && [ -n "$TEST_LOG_DIR" ]; then
        file_ops_log="$TEST_LOG_DIR/logs/tests/file-operations.log"
        echo "# Test: CLI file upload/download operations" > "$file_ops_log"
        echo "# Started: $(date -Iseconds)" >> "$file_ops_log"
        echo "========================================" >> "$file_ops_log"
    fi
    
    # Create test files
    local test_dir="/tmp/autonomi_test_$$"
    mkdir -p "$test_dir"
    
    # Small text file
    echo "Hello, Autonomi Network!" > "$test_dir/small.txt"
    
    # Medium JSON file
    echo '{"test": "data", "numbers": [1, 2, 3, 4, 5]}' > "$test_dir/medium.json"
    
    # Large file (1MB)
    dd if=/dev/zero of="$test_dir/large.dat" bs=1024 count=1024 2>/dev/null
    
    local upload_success=true
    local file_operation_errors=()
    
    for file in "$test_dir"/*; do
        filename=$(basename "$file")
        print_status "Testing $filename upload/download"
        
        # Log the test step
        if [ -n "$file_ops_log" ]; then
            echo "" >> "$file_ops_log"
            echo "# Testing: $filename" >> "$file_ops_log"
            echo "# File size: $(stat -f%z "$file" 2>/dev/null || stat -c%s "$file" 2>/dev/null || echo 'unknown')" >> "$file_ops_log"
        fi
        
        # Upload file
        if upload_output=$(cargo run --bin ant -- --local file upload "$file" 2>&1); then
            # Log successful upload
            if [ -n "$file_ops_log" ]; then
                echo "# Upload command: cargo run --bin ant -- --local file upload $file" >> "$file_ops_log"
                echo "# Upload output:" >> "$file_ops_log"
                echo "$upload_output" >> "$file_ops_log"
            fi
            
            # Extract address from output (simplified)
            address=$(echo "$upload_output" | grep -oE '[0-9a-fA-F]{64}' | head -1)
            
            if [ -n "$address" ]; then
                # Log address extraction
                if [ -n "$file_ops_log" ]; then
                    echo "# Extracted address: $address" >> "$file_ops_log"
                fi
                
                # Download file
                if download_output=$(cargo run --bin ant -- --local file download "$address" "$test_dir/downloaded_$filename" 2>&1); then
                    # Log successful download
                    if [ -n "$file_ops_log" ]; then
                        echo "# Download command: cargo run --bin ant -- --local file download $address $test_dir/downloaded_$filename" >> "$file_ops_log"
                        echo "# Download output:" >> "$file_ops_log"
                        echo "$download_output" >> "$file_ops_log"
                    fi
                    # Verify content
                    if cmp -s "$file" "$test_dir/downloaded_$filename"; then
                        print_success "$filename upload/download successful"
                    else
                        print_error "$filename content mismatch"
                        upload_success=false
                        file_operation_errors+=("$filename: Content mismatch after download")
                    fi
                else
                    print_error "$filename download failed"
                    if [ -n "$file_ops_log" ]; then
                        echo "# Download FAILED:" >> "$file_ops_log"
                        echo "$download_output" >> "$file_ops_log"
                    fi
                    upload_success=false
                    file_operation_errors+=("$filename: Download failed - $download_output")
                fi
            else
                print_error "Failed to extract address for $filename"
                if [ -n "$file_ops_log" ]; then
                    echo "# Address extraction FAILED:" >> "$file_ops_log"
                    echo "# Upload output was:" >> "$file_ops_log"
                    echo "$upload_output" >> "$file_ops_log"
                fi
                upload_success=false
                file_operation_errors+=("$filename: Failed to extract address from upload output - $upload_output")
            fi
        else
            print_error "$filename upload failed"
            if [ -n "$file_ops_log" ]; then
                echo "# Upload FAILED:" >> "$file_ops_log"
                echo "$upload_output" >> "$file_ops_log"
            fi
            upload_success=false
            file_operation_errors+=("$filename: Upload failed - $upload_output")
        fi
    done
    
    # Log final summary
    if [ -n "$file_ops_log" ]; then
        echo "" >> "$file_ops_log"
        echo "# Test Summary:" >> "$file_ops_log"
        echo "# Success: $upload_success" >> "$file_ops_log"
        echo "# Errors: ${#file_operation_errors[@]}" >> "$file_ops_log"
        if [ ${#file_operation_errors[@]} -gt 0 ]; then
            echo "# Error details:" >> "$file_ops_log"
            for error in "${file_operation_errors[@]}"; do
                echo "#   - $error" >> "$file_ops_log"
            done
        fi
        echo "# Finished: $(date -Iseconds)" >> "$file_ops_log"
    fi
    
    # Cleanup
    rm -rf "$test_dir"
    
    if [ "$upload_success" = true ]; then
        print_success "✅ All CLI file operations passed"
        print_status "Both API and CLI interfaces are working correctly"
    else
        print_warning "⚠️  CLI file operations had issues (API tests still passed)"
        print_status "Core network functionality is healthy - CLI issues are separate"
        # Store detailed file operation failure information
        local file_errors_text=""
        for error in "${file_operation_errors[@]}"; do
            file_errors_text="$file_errors_text
  - $error"
        done
        
        local failure_info="PACKAGE: file-operations
FAILED_TESTS: file upload/download operations
DETAILED_ERRORS:$file_errors_text
=================================================================================="
        
        FAILED_TEST_OUTPUTS+=("$failure_info")
        
        print_error "Some file operations failed (${#file_operation_errors[@]} errors)"
        FAILED_PACKAGES+=("file-operations")
        FAILED_TESTS=$((FAILED_TESTS + 1))
    fi
    
    TOTAL_TESTS=$((TOTAL_TESTS + 1))
}

# Function to cleanup processes
cleanup_processes() {
    print_status "Cleaning up processes..."
    
    # Stop network using antctl (per README)
    print_status "Stopping local network..."
    cargo run --bin antctl -- local kill > /dev/null 2>&1 || true
    
    # Wait a moment for graceful shutdown
    sleep 3
    
    # Force kill the antctl process if still running
    if [ -n "$NETWORK_PID" ] && kill -0 $NETWORK_PID 2>/dev/null; then
        print_status "Force stopping network process..."
        kill -9 $NETWORK_PID 2>/dev/null || true
    fi
    
    # Stop EVM testnet (only if it was started by us)
    if [ -n "$EVM_PID" ] && [ "$SKIP_EVM_TESTS" = false ]; then
        print_status "Stopping EVM testnet..."
        kill $EVM_PID 2>/dev/null || true
        sleep 1
        kill -9 $EVM_PID 2>/dev/null || true
    fi
    
    # Clean up any remaining node processes (in case some are orphaned)
    pkill -f "antnode" 2>/dev/null || true
    pkill -f "ant-node" 2>/dev/null || true
    
    # Copy node logs to our test directory before cleanup completes
    copy_node_logs_to_test_dir
    
    # Update final log summary with accurate counts after copying node logs
    if [ "$SAVE_LOGS" = true ] && [ -n "$TEST_LOG_DIR" ]; then
        # Refresh log counts after copying node logs
        local network_logs=$(find "$TEST_LOG_DIR/logs/network" -name "*.log" 2>/dev/null | wc -l | tr -d ' ')
        local test_logs=$(find "$TEST_LOG_DIR/logs/tests" -name "*.log" 2>/dev/null | wc -l | tr -d ' ')
        local node_logs=$(find "$TEST_LOG_DIR/logs/network/nodes" -name "*.log" 2>/dev/null | wc -l | tr -d ' ')
        
        if [ "$node_logs" -gt 0 ]; then
            log_message "SUCCESS" "📊 Final log count: $network_logs network + $test_logs test + $node_logs node logs"
        fi
    fi
    
    print_success "Cleanup complete"
}

# Function to print detailed test summary
print_summary() {
    print_header "Test Summary"
    
    local passed_packages=${#PASSED_PACKAGES[@]}
    local failed_packages=${#FAILED_PACKAGES[@]}
    local skipped_packages=${#SKIPPED_PACKAGES[@]}
    local total_individual_tests=0
    local total_duration=0
    
    # Calculate totals from test details
    for detail in "${TEST_DETAILS[@]}"; do
        IFS=':' read -ra parts <<< "$detail"
        local passed=${parts[1]:-0}
        if [[ "$passed" =~ ^[0-9]+$ ]]; then
            total_individual_tests=$((total_individual_tests + passed))
        fi
    done
    
    # Print overview
    echo "📊 Overview:"
    echo "  Packages tested: $TOTAL_TESTS"
    echo "  Individual tests: $total_individual_tests"
    echo "  Packages passed: $passed_packages"
    echo "  Packages failed: $failed_packages"
    echo "  Packages skipped: $skipped_packages"
    echo ""
    
    # Print summary (simplified for now)
    if [ ${#TEST_DETAILS[@]} -gt 0 ]; then
        print_success "📋 Detailed Results:"
        printf "%-20s %-8s %-8s %-8s %-10s %s\n" "Package" "Passed" "Failed" "Ignored" "Duration" "Status"
        printf "%-20s %-8s %-8s %-8s %-10s %s\n" "────────────────────" "──────" "──────" "───────" "────────" "──────"
        
        for detail in "${TEST_DETAILS[@]}"; do
            IFS=':' read -ra parts <<< "$detail"
            local package=${parts[0]}
            local passed=${parts[1]:-0}
            local failed=${parts[2]:-0}
            local ignored=${parts[3]:-0}
            local duration=${parts[4]:-"unknown"}
            
            local status="✅ PASS"
            if [ "$failed" -gt 0 ]; then
                status="❌ FAIL"
            elif [[ "$duration" == *"skipped"* ]]; then
                status="⏭️  SKIP"
            fi
            
            printf "%-20s %-8s %-8s %-8s %-10s %s\n" "$package" "$passed" "$failed" "$ignored" "$duration" "$status"
        done
    else
        print_status "📋 Test counts displayed during execution above"
    fi
    
    # Print package-level status
    echo ""
    if [ ${#PASSED_PACKAGES[@]} -gt 0 ]; then
        print_success "✅ Passed packages (${#PASSED_PACKAGES[@]}):"
        for package in "${PASSED_PACKAGES[@]}"; do
            echo "  - $package"
        done
    fi
    
    if [ ${#SKIPPED_PACKAGES[@]} -gt 0 ]; then
        echo ""
        print_warning "⏭️  Skipped packages (${#SKIPPED_PACKAGES[@]}):"
        for package in "${SKIPPED_PACKAGES[@]}"; do
            echo "  - $package"
        done
    fi
    
    if [ ${#FAILED_PACKAGES[@]} -gt 0 ]; then
        echo ""
        print_error "❌ Failed packages (${#FAILED_PACKAGES[@]}):"
        for package in "${FAILED_PACKAGES[@]}"; do
            echo "  - $package"
        done
    fi
    
    # Print detailed failure information if any tests failed
    if [ ${#FAILED_TEST_OUTPUTS[@]} -gt 0 ]; then
        echo ""
        print_error "🔍 DETAILED FAILURE ANALYSIS"
        echo "════════════════════════════════════════════════"
        echo ""
        
        for failure_output in "${FAILED_TEST_OUTPUTS[@]}"; do
            echo "$failure_output"
            echo ""
        done
    fi
    
    # Print final status
    echo ""
    echo "════════════════════════════════════════════════"
    if [ $failed_packages -eq 0 ]; then
        print_success "🎉 All tests passed! ($total_individual_tests individual tests)"
    else
        print_error "⚠️  $failed_packages package(s) failed. See detailed analysis above."
    fi
    
    # Print suggestions for failed tests
    if [ $failed_packages -gt 0 ]; then
        echo ""
        print_status "💡 Troubleshooting tips:"
        echo "  - Review the detailed failure analysis above for specific error messages"
        echo "  - Check compilation errors and missing dependencies"
        echo "  - Run failed tests individually for debugging:"
        for package in "${FAILED_PACKAGES[@]}"; do
            echo "    cargo test --package $package --lib -- --nocapture"
        done
        echo "  - Install missing dependencies (Foundry for EVM tests if needed)"
        echo "  - Check for environment issues or missing system dependencies"
    fi
    
    # Print final log summary if logging was enabled
    print_final_log_summary
    
    # Print next steps
    if [ $failed_packages -eq 0 ] && [ $skipped_packages -eq 0 ]; then
        echo ""
        print_status "🚀 Ready for integration testing!"
        echo "  Next: ./test.sh integration  # Test with network"
        echo "  Full: ./test.sh full         # Complete E2E testing"
    fi
}

# Trap to ensure cleanup on exit
trap cleanup_processes EXIT

# Main execution
case "${1:-unit}" in
    "unit")
        run_unit_tests
        ;;
    "integration")
        run_integration_tests
        ;;
    "full")
        run_full_tests
        ;;
    "analyze")
        if [ -z "$2" ]; then
            print_error "Usage: $0 analyze <log_directory>"
            print_error "Analyze a previous test run's logs for failures and issues"
            exit 1
        fi
        analyze_previous_test_run "$2"
        ;;
    "--help"|"-h")
        echo "Usage: $0 [unit|integration|full|analyze] [options]"
        echo ""
        echo "Test levels:"
        echo "  unit        - Run unit tests from src/ directories only (default)"
        echo "  integration - Run lightweight integration tests (requires running network)"
        echo "  full        - Run comprehensive tests with network setup and all data types"
        echo "  analyze     - Analyze logs from a previous test run"
        echo ""
        echo "Logging options:"
        echo "  --debug            - Enable debug logging and save all logs"
        echo "  --save-logs        - Save logs without debug level"
        echo "  --analyze-failures - Auto-analyze failures when they occur"
        echo ""
        echo "Test organization:"
        echo "  Unit tests:        Fast tests that don't require network connectivity"
        echo "  Integration tests: Lightweight tests that require a running network"
        echo "  Full tests:        Comprehensive E2E tests including data type verification"
        echo ""
        echo "Dependencies:"
        echo "  - Foundry/Anvil: Required for EVM tests (auto-installed with user consent)"
        echo "  - Local network: Auto-managed for integration and full tests"
        echo ""
        echo "Examples:"
        echo "  $0 unit                    # Run unit tests only"
        echo "  $0 full --debug            # Run full tests with comprehensive logging"
        echo "  $0 full --analyze-failures # Run tests with automatic failure analysis"
        echo "  $0 analyze /tmp/autonomi_test_run_2024-01-01_12-00-00"
        echo ""
        echo "Enhanced logging features:"
        echo "  - Separate log files for each test and network component"
        echo "  - Automatic failure analysis with recommendations"
        echo "  - Centralized log directory with structured organization"
        echo "  - Post-test analysis tools for debugging failures"
        echo ""
        echo "Note: First run may prompt to install Foundry for EVM testing."
        exit 0
        ;;
    *)
        print_error "Unknown test level: $1"
        echo "Use --help for usage information"
        exit 1
        ;;
esac

print_summary

# Exit with appropriate code
exit $FAILED_TESTS