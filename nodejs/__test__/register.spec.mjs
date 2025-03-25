import test from 'ava'

import { Client, Wallet, Network, PaymentOption, RegisterAddress, PublicKey, SecretKey } from '../index.js'

test('registers usage', async (t) => {
  const client = await Client.initLocal();
  const wallet = Wallet.newFromPrivateKey(new Network(true), "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80");
  const mainKey = SecretKey.random();

  const registerKey = Client.registerKeyFromName(mainKey, "register1");
  const content = Client.registerValueFromBytes(Buffer.from("Hello, World!"));
  const cost = await client.registerCost(registerKey.publicKey());

  // create the register
  const { cost: createCost, addr } = await client.registerCreate(registerKey, content, PaymentOption.fromWallet(wallet));
  t.deepEqual(addr, new RegisterAddress(registerKey.publicKey()));

  // wait for the register to be replicated
  await new Promise(resolve => setTimeout(resolve, 5000));

  // get the register
  const value = await client.registerGet(addr);
  t.deepEqual(value, content);

  // update the register
  const newContent = Client.registerValueFromBytes(Buffer.from("any 32 bytes of fresh data"));
  const updateCost = await client.registerUpdate(registerKey, newContent, PaymentOption.fromWallet(wallet));

  // wait for the register to be replicated
  await new Promise(resolve => setTimeout(resolve, 5000));

  // get the register again
  const updatedValue = await client.registerGet(addr);
  t.deepEqual(updatedValue, newContent);
});

test('registers errors', async (t) => {
  const client = await Client.initLocal();
  const wallet = Wallet.newFromPrivateKey(new Network(true), "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80");
  const mainKey = SecretKey.random();

  const registerKey = Client.registerKeyFromName(mainKey, "register1");
  const content = Client.registerValueFromBytes(Buffer.from("Hello, World!"));
  const cost = await client.registerCost(registerKey.publicKey());

  // try to update non existing register
  const error = await t.throwsAsync(async () => {
    await client.registerUpdate(registerKey, content, PaymentOption.fromWallet(wallet));
  });
  t.true(error.message.includes('CannotUpdateNewRegister'));

  // create the register
  const { cost: createCost, addr } = await client.registerCreate(registerKey, content, PaymentOption.fromWallet(wallet));
  t.deepEqual(addr, new RegisterAddress(registerKey.publicKey()));

  // wait for the register to be replicated
  await new Promise(resolve => setTimeout(resolve, 5000));

  // try to create the register again
  const createError = await t.throwsAsync(async () => {
    await client.registerCreate(registerKey, content, PaymentOption.fromWallet(wallet));
  });
  t.true(createError.message.includes('AlreadyExists'));
});

test('test register history', async (t) => {
  const client = await Client.initLocal();
  const wallet = Wallet.newFromPrivateKey(new Network(true), "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80");
  const mainKey = SecretKey.random();
  const registerKey = Client.registerKeyFromName(mainKey, "history_test");
  const content1 = Client.registerValueFromBytes(Buffer.from("Massive"));
  const { addr } = await client.registerCreate(registerKey, content1, PaymentOption.fromWallet(wallet));

  // let the network replicate the register
  await new Promise(resolve => setTimeout(resolve, 5000));

  let history = client.registerHistory(addr);
  const first = await history.next();
  t.deepEqual(first, content1);
  const second = await history.next();
  t.is(second, null);

  const content2 = Client.registerValueFromBytes(Buffer.from("Array"));
  await client.registerUpdate(registerKey, content2, PaymentOption.fromWallet(wallet));

  // let the network replicate the updates
  await new Promise(resolve => setTimeout(resolve, 5000));

  history = client.registerHistory(addr);
  const all = await history.collect();
  t.is(all.length, 2);
  t.deepEqual(all[0], content1);
  t.deepEqual(all[1], content2);

  const content3 = Client.registerValueFromBytes(Buffer.from("Internet"));
  await client.registerUpdate(registerKey, content3, PaymentOption.fromWallet(wallet));
  const content4 = Client.registerValueFromBytes(Buffer.from("Disk"));
  await client.registerUpdate(registerKey, content4, PaymentOption.fromWallet(wallet));

  // let the network replicate the updates
  await new Promise(resolve => setTimeout(resolve, 5000));

  history = client.registerHistory(addr);
  const allFour = await history.collect();
  t.is(allFour.length, 4);
  t.deepEqual(allFour[0], content1);
  t.deepEqual(allFour[1], content2);
  t.deepEqual(allFour[2], content3);
  t.deepEqual(allFour[3], content4);
});

test('register address', (t) => {
  // Generate a random secret key
  const secretKey = SecretKey.random();
  const publicKey = secretKey.publicKey();
  
  // Create a register address from the public key
  const addr = new RegisterAddress(publicKey);
  
  // Test owner method
  const owner = addr.owner();
  t.true(owner instanceof PublicKey);
  t.deepEqual(owner, publicKey);
  
  // Test to_underlying_graph_root method
  const graphRoot = addr.toUnderlyingGraphRoot();
  t.true(graphRoot.constructor.name === "GraphEntryAddress");
  
  // Test to_underlying_head_pointer method
  const headPointer = addr.toUnderlyingHeadPointer();
  t.true(headPointer.constructor.name === "PointerAddress");
  
  // Test to_hex and from_hex methods
  const hex = addr.toHex();
  t.true(typeof hex === 'string');
  const addr2 = RegisterAddress.fromHex(hex);
  t.is(addr.toHex(), addr2.toHex());
});
