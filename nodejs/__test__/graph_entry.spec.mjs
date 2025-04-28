import test from 'ava'

import { 
  Client, 
  Wallet, 
  Network, 
  PaymentOption, 
  GraphEntry, 
  GraphEntryAddress, 
  SecretKey, 
  PublicKey, 
  XorName 
} from '../index.js'

test('graph entry creation and verification', (t) => {
  const owner = SecretKey.random();
  const content = new Uint8Array(32).fill(1);
  const graphEntry = new GraphEntry(owner, [], content, []);
  
  t.true(graphEntry.verifySignature());
  t.is(graphEntry.parents().length, 0);
  t.is(graphEntry.descendants().length, 0);
  
  // Check content matches what we set
  const retrievedContent = graphEntry.content();
  t.is(retrievedContent.length, 32);
  t.is(retrievedContent[0], 1);
})

test('graph entry with parents and descendants', (t) => {
  const owner = SecretKey.random();
  const parent1 = SecretKey.random().publicKey();
  const parent2 = SecretKey.random().publicKey();
  
  const content = new Uint8Array(32).fill(2);
  
  const descendantKey = SecretKey.random().publicKey();
  const descendantContent = new Uint8Array(32).fill(3);
  
  const graphEntry = new GraphEntry(
    owner, 
    [parent1, parent2], 
    content, 
    [[descendantKey, descendantContent]]
  );
  
  t.true(graphEntry.verifySignature());
  t.is(graphEntry.parents().length, 2);
  t.is(graphEntry.descendants().length, 1);
  
  // Check parent public keys match
  const parents = graphEntry.parents();
  t.is(parents[0].toHex(), parent1.toHex());
  t.is(parents[1].toHex(), parent2.toHex());
  
  // Check descendant data matches
  const descendants = graphEntry.descendants();
  t.is(descendants[0].publicKey.toHex(), descendantKey.toHex());
  t.deepEqual(descendants[0].content, descendantContent);
})

test('graph entry address', (t) => {
  const owner = SecretKey.random();
  const content = new Uint8Array(32).fill(4);
  const graphEntry = new GraphEntry(owner, [], content, []);
  
  const address = graphEntry.address();
  t.true(address instanceof GraphEntryAddress);
  
  // Address should be derived from owner's public key
  const addressFromPublicKey = new GraphEntryAddress(owner.publicKey());
  t.is(address.toHex(), addressFromPublicKey.toHex());
  
  // XorName should be derivable from the address
  const xorname = address.xorname();
  t.true(xorname instanceof XorName);
})

test('graph entry put and get', async (t) => {
  const client = await Client.initLocal();
  const wallet = Wallet.newFromPrivateKey(new Network(true), "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80");
  
  const owner = SecretKey.random();
  const content = new Uint8Array(32).fill(5);
  const graphEntry = new GraphEntry(owner, [], content, []);
  
  // Check cost calculation
  const cost = await client.graphEntryCost(owner.publicKey());
  t.true(typeof cost === 'string');
  
  // Put the graph entry
  const { cost: putCost, addr } = await client.graphEntryPut(graphEntry, PaymentOption.fromWallet(wallet));
  t.true(typeof putCost === 'string');
  t.true(addr instanceof GraphEntryAddress);
  
  // Get the graph entry
  const retrievedEntry = await client.graphEntryGet(addr);
  t.true(retrievedEntry instanceof GraphEntry);
  t.true(retrievedEntry.verifySignature());

  // Check existence
  const exists = await client.graphEntryCheckExistance(addr);
  t.true(exists);
  
  // Verify content matches
  const retrievedContent = retrievedEntry.content();
  t.is(retrievedContent.length, 32);
  t.is(retrievedContent[0], 5);
})

test('graph entry size checks', (t) => {
  const owner = SecretKey.random();
  const content = new Uint8Array(32).fill(6);
  const graphEntry = new GraphEntry(owner, [], content, []);
  
  // Size should be a positive number
  const size = graphEntry.size();
  t.is(typeof size, 'bigint');
  t.true(size > 0);
  
  // A simple graph entry should not be too big
  t.false(graphEntry.isTooBig());
})

test('graph entry with signature creation', (t) => {
  const owner = SecretKey.random();
  const ownerPublicKey = owner.publicKey();
  const content = new Uint8Array(32).fill(7);
  
  // Create a normal graph entry first
  const normalEntry = new GraphEntry(owner, [], content, []);
  
  // Create a new entry with the same data but using the factory method
  const factoryEntry = GraphEntry.newWithSignature(
    ownerPublicKey,
    [],
    content,
    [],
    // In a real scenario, you'd use the actual signature bytes
    // Here we're just demonstrating the API
    normalEntry.signature
  );
  
  t.true(factoryEntry instanceof GraphEntry);
  // Note: This would fail verification since we didn't use a real signature
  // This is just to test the API
})
