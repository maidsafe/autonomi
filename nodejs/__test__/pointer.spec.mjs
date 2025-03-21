import test from 'ava'

import { Client, Wallet, Network, PaymentOption, PointerAddress, PublicKey, SecretKey, PointerTarget, ChunkAddress, XorName } from '../index.js'

test('pointer put and get', async (t) => {
  const client = await Client.initLocal();
  const wallet = Wallet.newFromPrivateKey(new Network(true), "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80");
  
  // Create a random secret key
  const secretKey = SecretKey.random();
  
  // Create a target (a random chunk address)
  const target = PointerTarget.ChunkAddress(new ChunkAddress(XorName.random()));
  
  // Create a pointer and put it on the network
  const { cost, addr } = await client.pointerCreate(secretKey, target, PaymentOption.fromWallet(wallet));

  // Verify the cost is returned as a string
  t.true(typeof cost === 'string');
  
  // Get the pointer from the network
  const pointer = await client.pointerGet(addr);
  
  // Verify the pointer exists
  const exists = await client.pointerCheckExistance(addr);
  t.true(exists);

  // Verify the pointer
  Client.pointerVerify(pointer);

  // Update the pointer to point to itself
  const selfTarget = PointerTarget.PointerAddress(addr);
  await client.pointerUpdate(secretKey, selfTarget);

  // Get the updated pointer
  const updatedPointer = await client.pointerGet(addr);

  // Verify the target has changed
  t.not(pointer.target().toHex(), updatedPointer.target().toHex());
})

test('pointer cost', async (t) => {
  const client = await Client.initLocal();
  
  // Create a random public key
  const secretKey = SecretKey.random();
  const publicKey = secretKey.publicKey();
  
  // Get the cost of creating a pointer
  const cost = await client.pointerCost(publicKey);
  
  // Verify the cost is returned as a string
  t.true(typeof cost === 'string');
})

test('pointer address', async (t) => {
  // Generate a random secret key
  const secretKey = SecretKey.random();
  const publicKey = secretKey.publicKey();
  
  // Create a pointer address from the public key
  const addr = new PointerAddress(publicKey);
  
  // Test owner method
  const owner = addr.owner();
  t.true(owner instanceof PublicKey);
  t.deepEqual(owner, publicKey);
  
  // Test xorname method
  const xorname = addr.xorname();
  t.true(xorname instanceof XorName);
  
  // Test to_hex and from_hex methods
  const hex = addr.toHex();
  t.true(typeof hex === 'string');
  const addr2 = PointerAddress.fromHex(hex);
  t.is(addr.toHex(), addr2.toHex());
})
