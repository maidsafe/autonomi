import test from 'ava'

import { 
  Client, 
  Wallet, 
  Network, 
  PaymentOption, 
  Scratchpad, 
  ScratchpadAddress, 
  SecretKey, 
  PublicKey, 
  XorName 
} from '../index.js'

test('scratchpad creation and verification', (t) => {
  const owner = SecretKey.random();
  const dataEncoding = 42n; // content type
  const data = Buffer.from("Massive Array of Internet Disks");
  const counter = 0n;
  
  const scratchpad = new Scratchpad(owner, dataEncoding, data, counter);
  
  t.true(scratchpad.verifySignature());
  t.is(scratchpad.counter(), 0n);
  t.is(scratchpad.dataEncoding(), 42n);
  
  // Check owner matches
  const ownerPublicKey = scratchpad.owner();
  t.is(ownerPublicKey.toHex(), owner.publicKey().toHex());
})

test('scratchpad address', (t) => {
  const owner = SecretKey.random();
  const dataEncoding = 42n;
  const data = Buffer.from("Secure Access For Everyone");
  const counter = 0n;
  
  const scratchpad = new Scratchpad(owner, dataEncoding, data, counter);
  
  const address = scratchpad.address();
  t.true(address instanceof ScratchpadAddress);
  
  // Address should be derived from owner's public key
  const addressFromPublicKey = new ScratchpadAddress(owner.publicKey());
  t.is(address.toHex(), addressFromPublicKey.toHex());
  
  // XorName should be derivable from the address
  const xorname = address.xorname();
  t.true(xorname instanceof XorName);
  
  // Owner should be accessible from the address
  const addressOwner = address.owner();
  t.is(addressOwner.toHex(), owner.publicKey().toHex());
})

test('scratchpad data decryption', (t) => {
  const owner = SecretKey.random();
  const dataEncoding = 42n;
  const originalData = Buffer.from("what's the meaning of life the universe and everything?");
  const counter = 0n;
  
  const scratchpad = new Scratchpad(owner, dataEncoding, originalData, counter);
  
  // Decrypt the data
  const decryptedData = scratchpad.decryptData(owner);
  t.deepEqual(decryptedData, originalData);
})

test('scratchpad put and get', async (t) => {
  const client = await Client.initLocal();
  const wallet = Wallet.newFromPrivateKey(new Network(true), "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80");
  
  const owner = SecretKey.random();
  const dataEncoding = 42n;
  const data = Buffer.from("Massive Array of Internet Disks");
  const counter = 0n;
  
  const scratchpad = new Scratchpad(owner, dataEncoding, data, counter);
  
  // Check cost calculation
  const cost = await client.scratchpadCost(owner.publicKey());
  t.true(typeof cost === 'string');
  
  // Put the scratchpad
  const { cost: putCost, addr } = await client.scratchpadPut(scratchpad, PaymentOption.fromWallet(wallet));
  t.true(typeof putCost === 'string');
  t.true(addr instanceof ScratchpadAddress);

  // Get the scratchpad
  const retrievedScratchpad = await client.scratchpadGet(addr);
  t.true(retrievedScratchpad instanceof Scratchpad);
  t.true(retrievedScratchpad.verifySignature());

  // Check existence
  const exists = await client.scratchpadCheckExistance(addr);
  t.true(exists);

  // Verify content matches
  const retrievedData = retrievedScratchpad.decryptData(owner);
  t.deepEqual(retrievedData, data);
})

test('scratchpad create and update', async (t) => {
  const client = await Client.initLocal();
  const wallet = Wallet.newFromPrivateKey(new Network(true), "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80");
  
  const owner = SecretKey.random();
  const dataEncoding = 42n;
  const initialData = Buffer.from("what's the meaning of life the universe and everything?");
  
  // Create the scratchpad
  const { cost, addr } = await client.scratchpadCreate(
    owner, 
    dataEncoding, 
    initialData, 
    PaymentOption.fromWallet(wallet)
  );
  
  t.true(typeof cost === 'string');
  t.true(addr instanceof ScratchpadAddress);
  
  // Get the scratchpad
  const scratchpad1 = await client.scratchpadGet(addr);
  t.is(scratchpad1.dataEncoding(), dataEncoding);
  t.is(scratchpad1.counter(), 0n);
  t.deepEqual(scratchpad1.decryptData(owner), initialData);
  
  // Update the scratchpad
  const updatedData = Buffer.from("42");
  await client.scratchpadUpdate(owner, dataEncoding, updatedData);
  
  // Get the updated scratchpad
  const scratchpad2 = await client.scratchpadGet(addr);
  t.is(scratchpad2.dataEncoding(), dataEncoding);
  t.is(scratchpad2.counter(), 1n);
  t.deepEqual(scratchpad2.decryptData(owner), updatedData);
})

test('scratchpad get from public key', async (t) => {
  const client = await Client.initLocal();
  const wallet = Wallet.newFromPrivateKey(new Network(true), "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80");
  
  const owner = SecretKey.random();
  const publicKey = owner.publicKey();
  const dataEncoding = 42n;
  const data = Buffer.from("Secure Access For Everyone");
  const counter = 0n;
  
  const scratchpad = new Scratchpad(owner, dataEncoding, data, counter);
  
  // Put the scratchpad
  await client.scratchpadPut(scratchpad, PaymentOption.fromWallet(wallet));
  
  // Get the scratchpad using the public key
  const retrievedScratchpad = await client.scratchpadGetFromPublicKey(publicKey);
  t.true(retrievedScratchpad instanceof Scratchpad);
  t.true(retrievedScratchpad.verifySignature());
  
  // Verify content matches
  const retrievedData = retrievedScratchpad.decryptData(owner);
  t.deepEqual(retrievedData, data);
})

test('scratchpad verification', async (t) => {
  const client = await Client.initLocal();
  
  const owner = SecretKey.random();
  const dataEncoding = 42n;
  const data = Buffer.from("Verification Test");
  const counter = 0n;
  
  const scratchpad = new Scratchpad(owner, dataEncoding, data, counter);
  
  // Verify the scratchpad
  await t.notThrowsAsync(async () => {
    await Client.scratchpadVerify(scratchpad);
  });
})
