#!/usr/bin/env python3
"""
Test script to trigger and verify fork error handling in scratchpad operations.
"""

import asyncio
import random
from autonomi_client import Client, SecretKey, PaymentOption, Wallet, EVMNetwork, Scratchpad

def generate_test_data():
    """Generate random test data."""
    words = [
        "apple", "banana", "cherry", "dragon", "elephant", "forest", "galaxy", "harbor", 
        "island", "jungle", "kitchen", "lighthouse", "mountain", "notebook", "ocean",
        "penguin", "question", "rainbow", "sunset", "treasure", "umbrella", "village",
        "whisper", "xylophone", "yellow", "zebra", "adventure", "butterfly", "castle",
        "diamond", "energy", "freedom", "garden", "happiness", "imagination", "journey"
    ]
    return " ".join(random.sample(words, 5))

def analyze_fork_from_exception(fork_error, owner_key):
    """Analyze fork from RuntimeError with conflicting_scratchpads attribute."""
    print("\nFORK ANALYSIS:")
    print("=" * 60)
    
    try:
        # Check if the exception has conflicting scratchpads
        if hasattr(fork_error, 'conflicting_scratchpads'):
            conflicting_scratchpads = fork_error.conflicting_scratchpads
            
            # Sort by signature to ensure consistent ordering with CLI
            sorted_scratchpads = sorted(conflicting_scratchpads, key=lambda s: str(s.signature()))
            
            print(f"Retrieved {len(sorted_scratchpads)} conflicting scratchpad(s):")
            
            for i, scratchpad in enumerate(sorted_scratchpads):
                print(f"\nCONFLICTING SCRATCHPAD #{i + 1} OF {len(sorted_scratchpads)}:")
                print(f"  Counter: {scratchpad.counter()}")
                print(f"  Data type encoding: {scratchpad.data_encoding()}")
                print(f"  PublicKey/Address: {scratchpad.owner().hex()}")
                print(f"  Signature: {str(scratchpad.signature())}")
                print(f"  Scratchpad hash: {scratchpad.scratchpad_hash()}")
                print(f"  Encrypted data hash: {scratchpad.encrypted_data_hash()}")
                print(f"  Encrypted data size: {len(scratchpad.encrypted_data())} bytes")
                
                # Decrypt the data
                try:
                    decrypted_data = scratchpad.decrypt_data(owner_key)
                    decrypted_str = decrypted_data.decode('utf-8', errors='replace')
                    print(f"  Decrypted data: \"{decrypted_str}\"")
                    print(f"  Decrypted data size: {len(decrypted_data)} bytes")
                except Exception as decrypt_err:
                    print(f"  Decryption failed: {decrypt_err}")
        else:
            print("No conflicting scratchpads data found in exception")
                
    except Exception as e:
        print(f"Fork analysis failed: {e}")

async def test_scratchpad_fork():
    """Test scratchpad fork detection."""
    
    print("Testing scratchpad fork detection...")
    
    try:
        # Setup client and wallet
        client = await Client.init_local()
        network = EVMNetwork(local=True)
        wallet = Wallet.new_from_private_key(network, "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
        payment = PaymentOption.wallet(wallet)
        
        # Create test scratchpad
        owner_key = SecretKey()
        public_key = owner_key.public_key()
        print(f"Secret key: {owner_key.hex()}")
        
        initial_data = generate_test_data()
        print(f"Creating scratchpad with data: '{initial_data}'")
        
        cost, addr = await client.scratchpad_create(
            owner_key, 
            content_type=0,
            initial_data=initial_data.encode(), 
            payment_option=payment
        )
        print(f"Created scratchpad at: {addr}")
        
        # Wait for scratchpad creation to propagate
        await asyncio.sleep(5)
        
        # Get base scratchpad for concurrent updates
        try:
            base_scratchpad = await client.scratchpad_get(addr)
            print(f"Base scratchpad counter: {base_scratchpad.counter()}")
        except RuntimeError as e:
            if hasattr(e, 'conflicting_scratchpads'):
                print("Fork detected during base scratchpad retrieval")
                analyze_fork_from_exception(e, owner_key)
                return
            else:
                raise e
        
        # Attempt concurrent updates to trigger fork
        async def update_attempt(i):
            try:
                test_data = generate_test_data()
                new_scratchpad = await client.scratchpad_update_from(
                    base_scratchpad,
                    owner_key,
                    content_type=0,
                    data=test_data.encode()
                )
                return f"Update {i}: Success (counter {new_scratchpad.counter()}) - data: {test_data}"
            except RuntimeError as e:
                if hasattr(e, 'conflicting_scratchpads'):
                    return ("fork_detected", e)  # Return tuple to identify fork
                return f"Update {i}: Error - {str(e)[:50]}..."
            except Exception as e:
                if "Got multiple conflicting scratchpads" in str(e):
                    return f"Update {i}: Fork detected - {str(e)}"
                return f"Update {i}: Error - {str(e)[:50]}..."
        
        # Launch concurrent updates
        print("Launching concurrent updates...")
        tasks = [asyncio.create_task(update_attempt(i)) for i in range(10)]
        results = await asyncio.gather(*tasks, return_exceptions=True)
        
        # Show results
        for i, result in enumerate(results):
            print(f"  {result}")
        
        # Check for fork immediately after concurrent updates (silent check)
        try:
            immediate_scratchpad = await client.scratchpad_get(addr)
            # No fork - we're done
            print("✅ No fork detected - Relaunch test generate a fork error")
            print("Scratchpad retrieved successfully:")
            print(f"Counter: {immediate_scratchpad.counter()}")
            
            # Decrypt and show data
            try:
                decrypted_data = immediate_scratchpad.decrypt_data(owner_key)
                decrypted_str = decrypted_data.decode('utf-8', errors='replace')
                print(f"Data: {decrypted_str}")
            except Exception as e:
                print(f"Could not decrypt data: {e}")
                
        except RuntimeError as e:
            if hasattr(e, 'conflicting_scratchpads'):
                # Fork detected immediately - but don't print analysis yet
                print("Waiting for network to settle before checking...")
                await asyncio.sleep(25)
                try:
                    final_scratchpad = await client.scratchpad_get(addr)
                    print("✅ No fork detected - Relaunch test generate a fork error")
                    print("Scratchpad retrieved successfully:")
                    print(f"Counter: {final_scratchpad.counter()}")
                    
                    # Decrypt and show data
                    try:
                        decrypted_data = final_scratchpad.decrypt_data(owner_key)
                        decrypted_str = decrypted_data.decode('utf-8', errors='replace')
                        print(f"Data: {decrypted_str}")
                    except Exception as e:
                        print(f"Could not decrypt data: {e}")
                        
                except RuntimeError as e:
                    if hasattr(e, 'conflicting_scratchpads'):
                        analyze_fork_from_exception(e, owner_key)
                        
                        # Fork resolution
                        print("\n" + "="*60)
                        print("FORK RESOLUTION")
                        print("="*60)
                        
                        conflicting_scratchpads = e.conflicting_scratchpads
                        sorted_scratchpads = sorted(conflicting_scratchpads, key=lambda s: str(s.signature()))
                        
                        print("Which scratchpad do you want to keep? Choose a number")
                        try:
                            choice = int(input("Enter number (1-{}): ".format(len(sorted_scratchpads))))
                            if 1 <= choice <= len(sorted_scratchpads):
                                chosen_scratchpad = sorted_scratchpads[choice - 1]
                                print(f"Resolving fork by keeping scratchpad #{choice}...")
                                
                                # Get chosen data and create new scratchpad with higher counter
                                chosen_data = chosen_scratchpad.decrypt_data(owner_key)
                                max_counter = max(s.counter() for s in sorted_scratchpads)
                                new_counter = max_counter + 1
                                
                                # Create resolution scratchpad
                                new_scratchpad = Scratchpad(owner_key, 0, chosen_data, new_counter)
                                
                                # Apply resolution
                                network = EVMNetwork(local=True)
                                wallet = Wallet.new_from_private_key(network, "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
                                payment = PaymentOption.wallet(wallet)
                                
                                cost, resolved_addr = await client.scratchpad_put(new_scratchpad, payment)
                                print(f"✅ Fork resolved with counter {new_counter}")
                                print(f"Data: {chosen_data.decode('utf-8', errors='replace')}")
                            else:
                                print("❌ Invalid choice. Fork not resolved.")
                        except ValueError:
                            print("❌ Invalid input. Fork not resolved.")
                        except Exception as resolve_err:
                            print(f"❌ Failed to resolve fork: {resolve_err}")
                    else:
                        print(f"❌ Non-fork scratchpad error after settling:")
                        print(f"Error: {str(e)}")
                except Exception as e:
                    print(f"❌ Other error after settling: {type(e).__name__}: {e}")
            else:
                print(f"❌ Non-fork scratchpad error:")
                print(f"Error: {str(e)}")
        except Exception as e:
            print(f"❌ Other error: {type(e).__name__}: {e}")
        
    except Exception as e:
        print(f"Test failed: {e}")

async def main():
    await test_scratchpad_fork()

if __name__ == "__main__":
    asyncio.run(main())