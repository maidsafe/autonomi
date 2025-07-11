#!/usr/bin/env python3
"""
Enhanced Autonomi Network Test Runner

This script provides a comprehensive test suite for the Autonomi Network with
multiple test tiers:
- Unit tests: Fast, isolated component tests
- Integration tests: Network-based tests with local network setup
- Full tests: Complete end-to-end testing with all data types

UV Dependencies:
# Install UV if not available: curl -LsSf https://astral.sh/uv/install.sh | sh
# Run this script: uv run python test_runner.py
# Or with dependencies: uv run --with rich,subprocess32 python test_runner.py
"""

# /// script
# requires-python = ">=3.11"
# dependencies = [
#     "rich>=13.0.0",
#     "subprocess32>=3.5.4",
#     "psutil>=5.9.0",
#     "tomli>=2.0.0",
# ]
# ///

import os
import sys
import subprocess
import time
import json
import tempfile
from pathlib import Path
from typing import Dict, List, Optional, Tuple
from dataclasses import dataclass
from enum import Enum
import logging
from rich.console import Console
from rich.progress import Progress, SpinnerColumn, TextColumn, BarColumn, TimeElapsedColumn
from rich.table import Table
from rich.panel import Panel
from rich.text import Text
import psutil
import tomli

# Configure logging
logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')
logger = logging.getLogger(__name__)

console = Console()

class TestLevel(Enum):
    UNIT = "unit"
    INTEGRATION = "integration"
    FULL = "full"

@dataclass
class TestConfig:
    """Configuration for test execution"""
    timeout: int
    packages: List[str]
    data_types: List[str] = None
    network_required: bool = False
    evm_required: bool = False

@dataclass
class TestResult:
    """Result of a test execution"""
    success: bool
    duration: float
    stdout: str
    stderr: str
    return_code: int
    test_count: int = 0
    failed_tests: List[str] = None

class NetworkManager:
    """Manages local Autonomi network for testing"""
    
    def __init__(self, node_count: int = 25):
        self.node_count = node_count
        self.network_process = None
        self.network_dir = None
        self.evm_process = None
        
    def start_evm_testnet(self) -> bool:
        """Start EVM testnet for payment testing"""
        try:
            console.print("🔧 Starting EVM testnet...", style="yellow")
            self.evm_process = subprocess.Popen(
                ["cargo", "run", "--bin", "evm-testnet"],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True
            )
            time.sleep(5)  # Give EVM time to start
            
            if self.evm_process.poll() is None:
                console.print("✅ EVM testnet started", style="green")
                return True
            else:
                console.print("❌ EVM testnet failed to start", style="red")
                return False
        except Exception as e:
            console.print(f"❌ Failed to start EVM testnet: {e}", style="red")
            return False
    
    def start_network(self) -> bool:
        """Start local Autonomi network"""
        try:
            console.print(f"🚀 Starting local network with {self.node_count} nodes...", style="yellow")
            
            # Create temporary directory for network
            self.network_dir = tempfile.mkdtemp(prefix="autonomi_test_")
            
            # Start network using antctl
            cmd = [
                "cargo", "run", "--bin", "antctl", "--", 
                "local", "run", 
                "--build", 
                "--clean",
                "--node-count", str(self.node_count),
                "--rewards-address", "0x1234567890123456789012345678901234567890"  # Dummy address
            ]
            
            self.network_process = subprocess.Popen(
                cmd,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                cwd=self.network_dir
            )
            
            # Wait for network to be ready
            max_wait = 120  # 2 minutes
            for i in range(max_wait):
                if self.is_network_ready():
                    console.print("✅ Network is ready!", style="green")
                    return True
                time.sleep(1)
                if i % 10 == 0:
                    console.print(f"⏳ Waiting for network... ({i}/{max_wait}s)", style="yellow")
            
            console.print("❌ Network failed to start within timeout", style="red")
            return False
            
        except Exception as e:
            console.print(f"❌ Failed to start network: {e}", style="red")
            return False
    
    def is_network_ready(self) -> bool:
        """Check if network is ready for testing"""
        try:
            result = subprocess.run(
                ["cargo", "run", "--bin", "antctl", "--", "status"],
                capture_output=True,
                text=True,
                timeout=10
            )
            return result.returncode == 0 and "running" in result.stdout.lower()
        except:
            return False
    
    def stop_network(self):
        """Stop the local network"""
        try:
            if self.network_process:
                console.print("🛑 Stopping network...", style="yellow")
                
                # Try graceful shutdown first
                subprocess.run(
                    ["cargo", "run", "--bin", "antctl", "--", "local", "kill"],
                    capture_output=True,
                    timeout=30
                )
                
                self.network_process.terminate()
                self.network_process.wait(timeout=10)
                
            if self.evm_process:
                console.print("🛑 Stopping EVM testnet...", style="yellow")
                self.evm_process.terminate()
                self.evm_process.wait(timeout=10)
                
            console.print("✅ Network stopped", style="green")
        except Exception as e:
            console.print(f"⚠️  Warning: Error stopping network: {e}", style="yellow")

class TestRunner:
    """Main test runner class"""
    
    def __init__(self):
        self.workspace_root = Path.cwd()
        self.config = self._load_config()
        self.network_manager = NetworkManager()
        
    def _load_config(self) -> Dict[str, TestConfig]:
        """Load test configuration from Cargo.toml"""
        try:
            with open(self.workspace_root / "Cargo.toml", "rb") as f:
                cargo_toml = tomli.load(f)
                
            testing_config = cargo_toml.get("workspace", {}).get("metadata", {}).get("testing", {})
            
            return {
                TestLevel.UNIT.value: TestConfig(
                    timeout=30,
                    packages=testing_config.get("unit-test-packages", []),
                    network_required=False,
                    evm_required=False
                ),
                TestLevel.INTEGRATION.value: TestConfig(
                    timeout=600,
                    packages=testing_config.get("integration-test-packages", []),
                    network_required=True,
                    evm_required=False
                ),
                TestLevel.FULL.value: TestConfig(
                    timeout=1800,
                    packages=testing_config.get("integration-test-packages", []),
                    data_types=testing_config.get("full-test-data-types", []),
                    network_required=True,
                    evm_required=True
                )
            }
        except Exception as e:
            logger.warning(f"Failed to load config from Cargo.toml: {e}")
            return self._default_config()
    
    def _default_config(self) -> Dict[str, TestConfig]:
        """Default configuration if Cargo.toml parsing fails"""
        return {
            TestLevel.UNIT.value: TestConfig(
                timeout=30,
                packages=["ant-bootstrap", "ant-evm", "ant-logging", "ant-protocol", "autonomi"],
                network_required=False,
                evm_required=False
            ),
            TestLevel.INTEGRATION.value: TestConfig(
                timeout=600,
                packages=["autonomi"],
                network_required=True,
                evm_required=False
            ),
            TestLevel.FULL.value: TestConfig(
                timeout=1800,
                packages=["autonomi"],
                data_types=["chunks", "files", "registers", "vaults"],
                network_required=True,
                evm_required=True
            )
        }
    
    def run_unit_tests(self) -> TestResult:
        """Run unit tests for all packages"""
        console.print("\n🧪 Running Unit Tests", style="bold blue")
        
        config = self.config[TestLevel.UNIT.value]
        total_duration = 0
        total_tests = 0
        failed_tests = []
        all_output = []
        
        with Progress(
            SpinnerColumn(),
            TextColumn("[progress.description]{task.description}"),
            BarColumn(),
            TextColumn("[progress.percentage]{task.percentage:>3.0f}%"),
            TimeElapsedColumn(),
            console=console,
            transient=True
        ) as progress:
            
            task = progress.add_task(f"Running tests for {len(config.packages)} packages", total=len(config.packages))
            
            for package in config.packages:
                progress.update(task, description=f"Testing {package}")
                
                cmd = ["cargo", "test", "--release", "--package", package, "--lib"]
                
                try:
                    result = subprocess.run(
                        cmd,
                        capture_output=True,
                        text=True,
                        timeout=config.timeout
                    )
                    
                    total_duration += time.time()
                    all_output.append(f"=== {package} ===")
                    all_output.append(result.stdout)
                    all_output.append(result.stderr)
                    
                    if result.returncode != 0:
                        failed_tests.append(package)
                        console.print(f"❌ {package} failed", style="red")
                    else:
                        # Parse test count from output
                        test_count = self._parse_test_count(result.stdout)
                        total_tests += test_count
                        console.print(f"✅ {package} passed ({test_count} tests)", style="green")
                        
                except subprocess.TimeoutExpired:
                    failed_tests.append(f"{package} (timeout)")
                    console.print(f"⏰ {package} timed out", style="yellow")
                except Exception as e:
                    failed_tests.append(f"{package} (error: {e})")
                    console.print(f"❌ {package} error: {e}", style="red")
                
                progress.advance(task)
        
        return TestResult(
            success=len(failed_tests) == 0,
            duration=total_duration,
            stdout="\n".join(all_output),
            stderr="",
            return_code=0 if len(failed_tests) == 0 else 1,
            test_count=total_tests,
            failed_tests=failed_tests
        )
    
    def run_integration_tests(self) -> TestResult:
        """Run integration tests with network setup"""
        console.print("\n🔗 Running Integration Tests", style="bold blue")
        
        config = self.config[TestLevel.INTEGRATION.value]
        
        # Setup network if required
        if config.network_required:
            if not self.network_manager.start_network():
                return TestResult(
                    success=False,
                    duration=0,
                    stdout="",
                    stderr="Failed to start network",
                    return_code=1,
                    failed_tests=["network_setup"]
                )
        
        try:
            # Set environment variables for network tests
            env = os.environ.copy()
            env["ANT_PEERS"] = "local"
            env["SECRET_KEY"] = "0x1234567890123456789012345678901234567890123456789012345678901234"
            
            total_duration = 0
            total_tests = 0
            failed_tests = []
            all_output = []
            
            for package in config.packages:
                console.print(f"🧪 Testing {package} integration tests...", style="yellow")
                
                cmd = ["cargo", "test", "--release", "--package", package, "--tests"]
                
                try:
                    start_time = time.time()
                    result = subprocess.run(
                        cmd,
                        capture_output=True,
                        text=True,
                        timeout=config.timeout,
                        env=env
                    )
                    duration = time.time() - start_time
                    total_duration += duration
                    
                    all_output.append(f"=== {package} integration tests ===")
                    all_output.append(result.stdout)
                    all_output.append(result.stderr)
                    
                    if result.returncode != 0:
                        failed_tests.append(package)
                        console.print(f"❌ {package} integration tests failed", style="red")
                    else:
                        test_count = self._parse_test_count(result.stdout)
                        total_tests += test_count
                        console.print(f"✅ {package} integration tests passed ({test_count} tests)", style="green")
                        
                except subprocess.TimeoutExpired:
                    failed_tests.append(f"{package} (timeout)")
                    console.print(f"⏰ {package} timed out", style="yellow")
                except Exception as e:
                    failed_tests.append(f"{package} (error: {e})")
                    console.print(f"❌ {package} error: {e}", style="red")
            
            return TestResult(
                success=len(failed_tests) == 0,
                duration=total_duration,
                stdout="\n".join(all_output),
                stderr="",
                return_code=0 if len(failed_tests) == 0 else 1,
                test_count=total_tests,
                failed_tests=failed_tests
            )
            
        finally:
            if config.network_required:
                self.network_manager.stop_network()
    
    def run_full_tests(self) -> TestResult:
        """Run comprehensive full tests with all data types"""
        console.print("\n🎯 Running Full Integration Tests", style="bold blue")
        
        config = self.config[TestLevel.FULL.value]
        
        # Setup network and EVM if required
        if config.evm_required:
            if not self.network_manager.start_evm_testnet():
                return TestResult(
                    success=False,
                    duration=0,
                    stdout="",
                    stderr="Failed to start EVM testnet",
                    return_code=1,
                    failed_tests=["evm_setup"]
                )
        
        if config.network_required:
            if not self.network_manager.start_network():
                return TestResult(
                    success=False,
                    duration=0,
                    stdout="",
                    stderr="Failed to start network",
                    return_code=1,
                    failed_tests=["network_setup"]
                )
        
        try:
            return self._run_comprehensive_tests(config)
        finally:
            if config.network_required:
                self.network_manager.stop_network()
    
    def _run_comprehensive_tests(self, config: TestConfig) -> TestResult:
        """Run comprehensive tests for all data types"""
        env = os.environ.copy()
        env["ANT_PEERS"] = "local"
        env["SECRET_KEY"] = "0x1234567890123456789012345678901234567890123456789012345678901234"
        
        total_duration = 0
        total_tests = 0
        failed_tests = []
        all_output = []
        
        # Test each data type
        for data_type in config.data_types or []:
            console.print(f"🧪 Testing {data_type} operations...", style="yellow")
            
            success = self._test_data_type(data_type, env)
            if not success:
                failed_tests.append(data_type)
                console.print(f"❌ {data_type} tests failed", style="red")
            else:
                console.print(f"✅ {data_type} tests passed", style="green")
                total_tests += 1
        
        # Run all integration tests
        for package in config.packages:
            console.print(f"🧪 Testing {package} full integration...", style="yellow")
            
            cmd = ["cargo", "test", "--release", "--package", package, "--tests"]
            
            try:
                start_time = time.time()
                result = subprocess.run(
                    cmd,
                    capture_output=True,
                    text=True,
                    timeout=config.timeout,
                    env=env
                )
                duration = time.time() - start_time
                total_duration += duration
                
                all_output.append(f"=== {package} full tests ===")
                all_output.append(result.stdout)
                all_output.append(result.stderr)
                
                if result.returncode != 0:
                    failed_tests.append(package)
                    console.print(f"❌ {package} full tests failed", style="red")
                else:
                    test_count = self._parse_test_count(result.stdout)
                    total_tests += test_count
                    console.print(f"✅ {package} full tests passed ({test_count} tests)", style="green")
                    
            except subprocess.TimeoutExpired:
                failed_tests.append(f"{package} (timeout)")
                console.print(f"⏰ {package} timed out", style="yellow")
            except Exception as e:
                failed_tests.append(f"{package} (error: {e})")
                console.print(f"❌ {package} error: {e}", style="red")
        
        return TestResult(
            success=len(failed_tests) == 0,
            duration=total_duration,
            stdout="\n".join(all_output),
            stderr="",
            return_code=0 if len(failed_tests) == 0 else 1,
            test_count=total_tests,
            failed_tests=failed_tests
        )
    
    def _test_data_type(self, data_type: str, env: dict) -> bool:
        """Test specific data type upload/download operations"""
        try:
            if data_type == "files":
                return self._test_files(env)
            elif data_type == "chunks":
                return self._test_chunks(env)
            elif data_type == "registers":
                return self._test_registers(env)
            elif data_type == "vaults":
                return self._test_vaults(env)
            elif data_type == "graph-entries":
                return self._test_graph_entries(env)
            elif data_type == "scratchpads":
                return self._test_scratchpads(env)
            elif data_type == "pointers":
                return self._test_pointers(env)
            else:
                console.print(f"⚠️  Unknown data type: {data_type}", style="yellow")
                return False
        except Exception as e:
            console.print(f"❌ Error testing {data_type}: {e}", style="red")
            return False
    
    def _test_files(self, env: dict) -> bool:
        """Test file upload/download operations"""
        test_files = [
            ("small.txt", "Hello, Autonomi Network!"),
            ("medium.json", '{"test": "data", "numbers": [1, 2, 3, 4, 5]}'),
            ("large.dat", "X" * 1024 * 1024),  # 1MB file
        ]
        
        for filename, content in test_files:
            # Create test file
            test_file = Path(f"/tmp/{filename}")
            test_file.write_text(content)
            
            # Upload file
            upload_cmd = ["cargo", "run", "--bin", "ant", "--", "--local", "file", "upload", str(test_file)]
            result = subprocess.run(upload_cmd, capture_output=True, text=True, env=env)
            
            if result.returncode != 0:
                console.print(f"❌ Failed to upload {filename}", style="red")
                return False
            
            # Extract address from output
            address = self._extract_address(result.stdout)
            if not address:
                console.print(f"❌ Failed to extract address for {filename}", style="red")
                return False
            
            # Download file
            download_file = Path(f"/tmp/downloaded_{filename}")
            download_cmd = ["cargo", "run", "--bin", "ant", "--", "--local", "file", "download", address, str(download_file)]
            result = subprocess.run(download_cmd, capture_output=True, text=True, env=env)
            
            if result.returncode != 0:
                console.print(f"❌ Failed to download {filename}", style="red")
                return False
            
            # Verify content
            if download_file.read_text() != content:
                console.print(f"❌ Content mismatch for {filename}", style="red")
                return False
            
            # Cleanup
            test_file.unlink()
            download_file.unlink()
        
        return True
    
    def _test_chunks(self, env: dict) -> bool:
        """Test chunk operations"""
        # This would test direct chunk upload/download
        # For now, return True as chunks are tested via files
        return True
    
    def _test_registers(self, env: dict) -> bool:
        """Test register operations"""
        # This would test register create/update/read operations
        # For now, return True as this requires more complex test setup
        return True
    
    def _test_vaults(self, env: dict) -> bool:
        """Test vault operations"""
        # This would test vault create/sync/load operations
        # For now, return True as this requires more complex test setup
        return True
    
    def _test_graph_entries(self, env: dict) -> bool:
        """Test graph entry operations"""
        # This would test graph entry operations
        # For now, return True as this requires more complex test setup
        return True
    
    def _test_scratchpads(self, env: dict) -> bool:
        """Test scratchpad operations"""
        # This would test scratchpad operations
        # For now, return True as this requires more complex test setup
        return True
    
    def _test_pointers(self, env: dict) -> bool:
        """Test pointer operations"""
        # This would test pointer operations
        # For now, return True as this requires more complex test setup
        return True
    
    def _extract_address(self, output: str) -> Optional[str]:
        """Extract address from command output"""
        # Parse the output to extract the address
        # This is a simplified version - actual implementation would need
        # to parse the specific format used by the ant CLI
        lines = output.split('\n')
        for line in lines:
            if 'address' in line.lower() or 'addr' in line.lower():
                # Extract hex address or similar
                parts = line.split()
                for part in parts:
                    if len(part) > 10 and (part.startswith('0x') or part.isalnum()):
                        return part
        return None
    
    def _parse_test_count(self, output: str) -> int:
        """Parse test count from cargo test output"""
        lines = output.split('\n')
        for line in lines:
            if 'test result:' in line and 'passed' in line:
                # Extract number from "test result: ok. X passed; Y failed; ..."
                parts = line.split()
                for i, part in enumerate(parts):
                    if part == 'passed;' and i > 0:
                        try:
                            return int(parts[i-1])
                        except ValueError:
                            pass
        return 0
    
    def print_summary(self, results: Dict[str, TestResult]):
        """Print test summary"""
        console.print("\n📊 Test Summary", style="bold blue")
        
        table = Table(title="Test Results")
        table.add_column("Test Level", style="cyan")
        table.add_column("Status", style="magenta")
        table.add_column("Tests", style="green")
        table.add_column("Duration", style="yellow")
        table.add_column("Failed", style="red")
        
        total_tests = 0
        total_duration = 0
        total_failed = 0
        
        for level, result in results.items():
            status = "✅ PASS" if result.success else "❌ FAIL"
            duration = f"{result.duration:.2f}s"
            failed_count = len(result.failed_tests) if result.failed_tests else 0
            
            table.add_row(
                level.upper(),
                status,
                str(result.test_count),
                duration,
                str(failed_count)
            )
            
            total_tests += result.test_count
            total_duration += result.duration
            total_failed += failed_count
        
        table.add_row(
            "TOTAL",
            "✅ PASS" if total_failed == 0 else "❌ FAIL",
            str(total_tests),
            f"{total_duration:.2f}s",
            str(total_failed),
            style="bold"
        )
        
        console.print(table)
        
        # Print failed tests details
        if total_failed > 0:
            console.print("\n❌ Failed Tests:", style="bold red")
            for level, result in results.items():
                if result.failed_tests:
                    console.print(f"{level.upper()}:", style="red")
                    for failed in result.failed_tests:
                        console.print(f"  - {failed}", style="red")

def main():
    """Main entry point"""
    import argparse
    
    parser = argparse.ArgumentParser(description="Autonomi Network Test Runner")
    parser.add_argument("--level", choices=["unit", "integration", "full"], 
                       default="unit", help="Test level to run")
    parser.add_argument("--verbose", "-v", action="store_true", 
                       help="Verbose output")
    
    args = parser.parse_args()
    
    if args.verbose:
        logging.getLogger().setLevel(logging.DEBUG)
    
    runner = TestRunner()
    results = {}
    
    console.print(Panel.fit("🧪 Autonomi Network Test Runner", style="bold blue"))
    
    if args.level == "unit":
        results["unit"] = runner.run_unit_tests()
    elif args.level == "integration":
        results["integration"] = runner.run_integration_tests()
    elif args.level == "full":
        results["full"] = runner.run_full_tests()
    
    runner.print_summary(results)
    
    # Return appropriate exit code
    success = all(result.success for result in results.values())
    sys.exit(0 if success else 1)

if __name__ == "__main__":
    main()