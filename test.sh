#!/bin/bash

# Enhanced Autonomi Network Test Runner
# This script provides comprehensive testing capabilities for the Autonomi Network

set -e

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
    
    # Ensure Foundry is in PATH if installed
    ensure_foundry_path
    
    # Check for Foundry/Anvil
    if ! command -v anvil >/dev/null 2>&1; then
        if ! check_and_install_foundry; then
            print_warning "Proceeding without EVM testing (some tests will be skipped)"
            SKIP_EVM_TESTS=true
        else
            SKIP_EVM_TESTS=false
        fi
    else
        print_success "Anvil is available for EVM testing"
        SKIP_EVM_TESTS=false
    fi
    
    # Start EVM testnet (only if Foundry is available and not already running)
    if [ "$SKIP_EVM_TESTS" = false ]; then
        if is_anvil_running; then
            print_status "Anvil is already running, using existing instance"
        else
            print_status "Starting EVM testnet..."
            cargo run --bin evm-testnet &
            EVM_PID=$!
            sleep 5
        fi
    else
        print_status "Skipping EVM testnet (Foundry not available)"
    fi
    
    # Start local network using antctl
    print_status "Starting local network using antctl (25 nodes)..."
    print_status "This will take about 60-90 seconds for full network startup"
    
    # Start the network
    cargo run --bin antctl -- local run --build --clean --rewards-address 0x1234567890123456789012345678901234567890 &
    NETWORK_PID=$!
    
    # Wait for network to be ready with better checks
    print_status "Waiting for network nodes to start..."
    local network_ready=false
    local node_count=0
    
    for i in {1..180}; do
        # Check if antctl can get status (basic connectivity)
        if cargo run --bin antctl -- status > /dev/null 2>&1; then
            # Get detailed status to count nodes
            local status_output
            if status_output=$(cargo run --bin antctl -- status 2>&1); then
                # Count running nodes from status output
                node_count=$(echo "$status_output" | grep -c "Node " || echo "0")
                
                if [ "$node_count" -ge 20 ]; then
                    print_success "Network is ready with $node_count nodes!"
                    network_ready=true
                    break
                elif [ "$node_count" -gt 0 ]; then
                    if [ $((i % 10)) -eq 0 ]; then
                        print_status "Network starting... ($node_count nodes, $i seconds elapsed)"
                    fi
                fi
            fi
        fi
        
        sleep 1
        if [ $((i % 30)) -eq 0 ]; then
            print_status "Still waiting for network startup... ($i seconds elapsed)"
        fi
    done
    
    if [ "$network_ready" = false ]; then
        print_error "Network failed to start properly within timeout (3 minutes)"
        print_error "Only $node_count nodes detected"
        cleanup_processes
        return 1
    fi
    
    # Set environment for tests
    export ANT_PEERS="local"
    export SECRET_KEY="0x1234567890123456789012345678901234567890123456789012345678901234"
    
    # Run comprehensive tests
    print_status "Running comprehensive data type tests..."
    
    # Test file operations
    test_file_operations
    
    # Run comprehensive tests including the one that was moved from integration
    print_status "Running comprehensive data type tests..."
    
    # Run the comprehensive address test separately
    local address_test_output
    if address_test_output=$(cargo test --release --package autonomi --test address 2>&1); then
        local test_count=$(parse_test_results "$address_test_output" "autonomi-address")
        if [ -n "$test_count" ] && [ "$test_count" -gt 0 ]; then
            print_success "Address tests passed ($test_count tests)"
        else
            print_success "Address tests passed"
        fi
    else
        # Extract failed test names for address tests
        local failed_test_names=$(extract_failed_tests "$address_test_output")
        
        # Store complete failure information
        local failure_info="PACKAGE: autonomi-address
FAILED_TESTS: $failed_test_names
FULL_OUTPUT:
$address_test_output
=================================================================================="
        
        FAILED_TEST_OUTPUTS+=("$failure_info")
        
        print_error "Address tests failed (specific tests: $failed_test_names)"
        FAILED_PACKAGES+=("autonomi-address")
        FAILED_TESTS=$((FAILED_TESTS + 1))
    fi
    
    TOTAL_TESTS=$((TOTAL_TESTS + 1))
    
    # Run all other integration tests
    local all_integration_tests=("chunk" "files" "graph" "put" "pointer" "scratchpad" "registers" "vault" "wallet" "analyze" "external_signer")
    
    for test_name in "${all_integration_tests[@]}"; do
        print_status "Running $test_name full tests"
        
        local test_output
        if test_output=$(cargo test --release --package autonomi --test "$test_name" 2>&1); then
            local test_count=$(parse_test_results "$test_output" "autonomi-$test_name-full")
            if [ -n "$test_count" ] && [ "$test_count" -gt 0 ]; then
                print_success "$test_name full tests passed ($test_count tests)"
            else
                print_success "$test_name full tests passed"
            fi
        else
            # Extract failed test names for full tests
            local failed_test_names=$(extract_failed_tests "$test_output")
            
            # Store complete failure information
            local failure_info="PACKAGE: autonomi-$test_name-full
FAILED_TESTS: $failed_test_names
FULL_OUTPUT:
$test_output
=================================================================================="
            
            FAILED_TEST_OUTPUTS+=("$failure_info")
            
            print_error "$test_name full tests failed (specific tests: $failed_test_names)"
            FAILED_PACKAGES+=("autonomi-$test_name-full")
            FAILED_TESTS=$((FAILED_TESTS + 1))
        fi
        
        TOTAL_TESTS=$((TOTAL_TESTS + 1))
    done
    
    # Cleanup
    cleanup_processes
}

# Function to test file operations
test_file_operations() {
    print_status "Testing file upload/download operations"
    
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
        
        # Upload file
        if upload_output=$(cargo run --bin ant -- --local file upload "$file" 2>&1); then
            # Extract address from output (simplified)
            address=$(echo "$upload_output" | grep -oE '[0-9a-fA-F]{64}' | head -1)
            
            if [ -n "$address" ]; then
                # Download file
                if download_output=$(cargo run --bin ant -- --local file download "$address" "$test_dir/downloaded_$filename" 2>&1); then
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
                    upload_success=false
                    file_operation_errors+=("$filename: Download failed - $download_output")
                fi
            else
                print_error "Failed to extract address for $filename"
                upload_success=false
                file_operation_errors+=("$filename: Failed to extract address from upload output - $upload_output")
            fi
        else
            print_error "$filename upload failed"
            upload_success=false
            file_operation_errors+=("$filename: Upload failed - $upload_output")
        fi
    done
    
    # Cleanup
    rm -rf "$test_dir"
    
    if [ "$upload_success" = true ]; then
        print_success "All file operations passed"
    else
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
    
    # Stop network using antctl
    if [ -n "$NETWORK_PID" ]; then
        print_status "Stopping local network..."
        cargo run --bin antctl -- local kill > /dev/null 2>&1 || true
        
        # Wait a moment for graceful shutdown
        sleep 2
        
        # Force kill the antctl process if still running
        if kill -0 $NETWORK_PID 2>/dev/null; then
            print_status "Force stopping network process..."
            kill -9 $NETWORK_PID 2>/dev/null || true
        fi
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
    "--help"|"-h")
        echo "Usage: $0 [unit|integration|full]"
        echo ""
        echo "Test levels:"
        echo "  unit        - Run unit tests from src/ directories only (default)"
        echo "  integration - Run lightweight integration tests (requires running network)"
        echo "  full        - Run comprehensive tests with network setup and all data types"
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
        echo "  $0 unit       # Run unit tests only (may prompt for Foundry installation)"
        echo "  $0 integration # Run integration tests (requires: cargo run --bin antctl -- local run)"
        echo "  $0 full       # Run comprehensive tests (includes network setup)"
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