import { Client, SecretKey, Wallet, Network, PaymentOption } from '@withautonomi/autonomi';

async function scratchpadExample() {
    try {
        // Initialize client and wallet
        const client = await Client.initLocal();
        const network = new Network(true);  // Use testnet for local development
        // For mainnet use: new Network(false)
        const privateKey = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
        const wallet = Wallet.newFromPrivateKey(network, privateKey);
        const payment = PaymentOption.fromWallet(wallet);

        // Create secret key for scratchpad
        const key = SecretKey.random();
        const publicKey = key.publicKey();

        // Check cost
        const estimatedCost = await client.scratchpadCost(publicKey);
        console.log(`Estimated scratchpad cost: ${estimatedCost}`);

        // Create scratchpad
        const contentType = 42n;
        const initialData = Buffer.from("Hello, Autonomi!");
        const { cost: actualCost, addr } = await client.scratchpadCreate(key, contentType, initialData, payment);
        console.log(`Created at ${addr.toHex()}`);
        console.log(`Actual cost: ${actualCost}`);

        // Get scratchpad
        const scratchpad = await client.scratchpadGet(addr);
        console.assert(scratchpad.counter() === 0n);
        console.log(`Retrieved scratchpad with counter: ${scratchpad.counter()}`);
        
        // Decrypt content
        const decrypted = scratchpad.decryptData(key);
        console.assert(Buffer.compare(decrypted, initialData) === 0);
        console.log("✓ Decrypted content matches initial data");

        // Update scratchpad (free)
        const newData = Buffer.from("Updated content!");
        await client.scratchpadUpdate(key, contentType, newData);
        console.log("✓ Scratchpad updated successfully");

        // Get updated scratchpad
        const updated = await client.scratchpadGet(addr);
        console.assert(updated.counter() === 1n);
        const updatedContent = updated.decryptData(key);
        console.assert(Buffer.compare(updatedContent, newData) === 0);
        console.log(`✓ Updated scratchpad verified with counter: ${updated.counter()}`);
        console.log("✓ All scratchpad operations completed successfully!");

    } catch (error) {
        console.error('Error:', error.message);
    }
}

scratchpadExample();
