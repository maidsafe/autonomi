import test from 'ava'
import crypto from 'crypto'
import {
    Client,
    Wallet,
    Network,
    PaymentOption,
    SecretKey,
    DataAddress,
    XorName
} from '../index.js'

test('data get', async (t) => {
    // Initialize client
    const client = await Client.initLocal()
    const wallet = Wallet.newFromPrivateKey(new Network(true), "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")

    const data = crypto.randomBytes(1024 * 1024); // 1MiB

    const { cost, dataMap } = await client.dataPut(data, PaymentOption.fromWallet(wallet))

    const dataFetched = await client.dataGet(dataMap)
    t.deepEqual(data, dataFetched, "data fetched should match data put");
});

test('data get public', async (t) => {
    // Initialize client
    const client = await Client.initLocal()
    const wallet = Wallet.newFromPrivateKey(new Network(true), "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")

    const data = crypto.randomBytes(1024 * 1024); // 1MiB

    const { cost, addr } = await client.dataPutPublic(data, PaymentOption.fromWallet(wallet))

    const dataFetched = await client.dataGetPublic(addr)
    t.deepEqual(data, dataFetched, "data fetched should match data put");
});

test('data address', (t) => {
    const xorName = XorName.random();

    const addr = new DataAddress(xorName);
    t.true(addr.constructor.name === "DataAddress");

    const hex = addr.toHex();
    t.true(typeof hex === 'string');

    const addr2 = DataAddress.fromHex(hex);
    t.is(addr.toHex(), addr2.toHex());

    const addr3 = new DataAddress(addr.xorname())
    t.is(addr.toHex(), addr3.toHex());
});

