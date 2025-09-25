# Copyright 2025 MaidSafe.net limited.
#
# This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
# Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
# under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
# KIND, either express or implied. Please review the Licences for the specific language governing
# permissions and limitations relating to use of the SAFE Network Software.

#!/usr/bin/env python3
import asyncio
import random
from autonomi_client import Client, SecretKey, PaymentOption, Wallet, EVMNetwork, Scratchpad

# Configuration constants
INITIAL_SETUP_DELAY = 5  # seconds to wait after creating scratchpad
FORK_CHECK_DELAY = 60    # seconds to wait for the network to settle down before checking for fork
CONCURRENT_UPDATES_COUNT = 10  # number of concurrent update attempts
WORD_SAMPLE_SIZE = 5     # number of words to sample for test data generation

def generate_test_data():
    words = [
        "apple", "banana", "cherry", "dragon", "forest", "galaxy", "harbor", 
        "island", "kitchen", "mountain", "ocean", "penguin", "rainbow", 
        "sunset", "treasure", "village", "butterfly", "castle", "garden"
    ]
    return " ".join(random.sample(words, WORD_SAMPLE_SIZE))

def print_fork_analysis(fork_error, owner_key):
    if not hasattr(fork_error, 'conflicting_scratchpads'):
        print("No fork data found in exception")
        return
    
    scratchpads = sorted(fork_error.conflicting_scratchpads, key=lambda s: str(s.signature()))
    print(f"\nFORK ANALYSIS:")
    print("=" * 80)
    print(f"Found {len(scratchpads)} conflicting scratchpads:")
    
    for i, scratchpad in enumerate(scratchpads):
        print(f"\n#{i + 1} OF {len(scratchpads)}:")
        print(f"  Counter: {scratchpad.counter()}")
        print(f"  Data type encoding: {scratchpad.data_encoding()}")
        print(f"  PublicKey/Address: {scratchpad.owner().hex()}")
        print(f"  Signature: {str(scratchpad.signature())}")
        print(f"  Scratchpad hash: {scratchpad.scratchpad_hash()}")
        print(f"  Encrypted data hash: {scratchpad.encrypted_data_hash()}")
        
        try:
            data = scratchpad.decrypt_data(owner_key)
            content = data.decode('utf-8', errors='replace')
            print(f"  Decrypted data: \"{content}\"")
        except Exception as e:
            print(f"  Decryption failed: {e}")

def resolve_fork(fork_error, owner_key):
    scratchpads = sorted(fork_error.conflicting_scratchpads, key=lambda s: str(s.signature()))
    
    print("\nFORK RESOLUTION:")
    print("=" * 80)
    print("Choose which scratchpad to keep:")
    
    for i, scratchpad in enumerate(scratchpads):
        try:
            data = scratchpad.decrypt_data(owner_key).decode('utf-8', errors='replace')
            print(f"  #{i+1} | Counter: {scratchpad.counter()} | Data: \"{data}\"")
        except:
            print(f"  #{i+1} | Counter: {scratchpad.counter()} | [decrypt failed]")
    
    try:
        choice = int(input(f"Enter choice (1-{len(scratchpads)}): ")) - 1
        if 0 <= choice < len(scratchpads):
            return scratchpads[choice]
        print("Invalid choice")
        return None
    except ValueError:
        print("Invalid input")
        return None

async def check_for_fork(client, addr, owner_key):
    try:
        scratchpad = await client.scratchpad_get(addr)
        data = scratchpad.decrypt_data(owner_key).decode('utf-8', errors='replace')
        print(f"Success: counter={scratchpad.counter()}, data=\"{data}\"")
        return None
    except RuntimeError as e:
        if hasattr(e, 'conflicting_scratchpads'):
            return e
        print(f"Non-fork error: {e}")
        return None

async def run_concurrent_updates(client, base_scratchpad, owner_key):
    async def update_attempt(i):
        try:
            data = generate_test_data()
            await client.scratchpad_update_from(base_scratchpad, owner_key, 0, data.encode())
            return f"Update {i}: Success with \"{data}\""
        except Exception as e:
            return f"Update {i}: {type(e).__name__}"
    
    print("Running concurrent updates...")
    tasks = [update_attempt(i + 1) for i in range(CONCURRENT_UPDATES_COUNT)]
    results = await asyncio.gather(*tasks, return_exceptions=True)
    
    for result in results:
        print(f"  {result}")

async def test_scratchpad_fork():
    print("Setting up client and wallet...")
    client = await Client.init_local()
    network = EVMNetwork(local=True)
    wallet = Wallet.new_from_private_key(network, "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
    payment = PaymentOption.wallet(wallet)
    
    print("Creating test scratchpad...")
    owner_key = SecretKey()
    initial_data = generate_test_data()
    print(f"Initial data: \"{initial_data}\"")
    print(f"Secret key: {owner_key.hex()}")
    
    cost, addr = await client.scratchpad_create(owner_key, 0, initial_data.encode(), payment)
    print(f"Created scratchpad: {addr}")
    
    await asyncio.sleep(INITIAL_SETUP_DELAY)
    base_scratchpad = await client.scratchpad_get(addr)
    print(f"Base counter: {base_scratchpad.counter()}")
    
    await run_concurrent_updates(client, base_scratchpad, owner_key)
    
    await asyncio.sleep(FORK_CHECK_DELAY)
    print("\nChecking for fork...")
    fork_error = await check_for_fork(client, addr, owner_key)
    
    if not fork_error:
        print("No fork detected")
        return
    
    print_fork_analysis(fork_error, owner_key)
    chosen_scratchpad = resolve_fork(fork_error, owner_key)
    
    if not chosen_scratchpad:
        print("Fork resolution cancelled")
        return
    
    print("\nResolving fork...")
    chosen_data = chosen_scratchpad.decrypt_data(owner_key)
    
    updated_scratchpad = await client.scratchpad_update_from(
        chosen_scratchpad, owner_key, 0, chosen_data
    )

    content = chosen_data.decode('utf-8', errors='replace')
    print(f"Fork resolved with counter {updated_scratchpad.counter()}")
    print(f"Final data: \"{content}\"")

async def main():
    try:
        await test_scratchpad_fork()
    except Exception as e:
        print(f"Test failed: {e}")

if __name__ == "__main__":
    asyncio.run(main())