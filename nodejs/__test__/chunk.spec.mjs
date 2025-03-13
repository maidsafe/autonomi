import test from 'ava'

import { Client, Wallet, Network, PaymentOption, ChunkAddress, XorName } from '../index.js'

test('chunk put and get', async (t) => {
  const client = await Client.initLocal();
  const wallet = Wallet.newFromPrivateKey(new Network(true), "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80");

  const data = Buffer.from("Hello, World.");
  const { cost, addr } = await client.chunkPut(data, PaymentOption.fromWallet(wallet));
  const dataRetrieved = await client.chunkGet(addr);
  
  t.deepEqual(data, dataRetrieved);
})

test('chunk cost', async (t) => {
  const client = await Client.initLocal();

  const addr = new ChunkAddress(XorName.random());
  const cost = await client.chunkCost(addr);
  
  t.true(typeof cost === 'string');
})

test('chunk address', async (t) => {
  const addr = new ChunkAddress(XorName.random());
  const xorname = addr.xorname();
  t.true(xorname instanceof XorName);

  const hex = addr.toHex();
  t.true(typeof hex === 'string');
  const addr2 = ChunkAddress.fromHex(hex);
  t.is(addr.toHex(), addr2.toHex());
})
