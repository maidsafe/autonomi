The client API for Autonomi. This Node.js addon provides bindings into the Rust `autonomi` crate.

# Usage

Add the `@withautonomi/autonomi` package to your project. For example, using `npm`:
```console
$ npm install @withautonomi/autonomi
```

Using a modern version of Node.js we can use `import` and `async` easily when we use the `.mjs` extension. Import the `Client` and you're ready to connect to the network!

```js
// main.mjs
import { Client } from '@withautonomi/autonomi'
const client = await Client.initLocal()
```

Run the script:

```console
$ node main.js
```

## Examples

> Work in progress:
> 
> For general guides and usage, see the [Developer Documentation](https://docs.autonomi.com/developers). This is currently worked on specifically to include Node.js usage.

For example usage, see the [`__test__`](./__test__) directory. Replace `import { .. } from '../index.js'` to import from `@withautonomi/autonomi` instead.

## Register Operations

Registers are mutable data structures on the Autonomi network that allow you to create updateable content with a versioned history. Each register is owned by a specific key and can only be updated by the owner.

### Basic Register Usage

```js
import { Client, Wallet, Network, PaymentOption, RegisterAddress, SecretKey } from '@withautonomi/autonomi'

// Initialize client and wallet
const client = await Client.initLocal()
const wallet = Wallet.newFromPrivateKey(
  new Network(true), 
  "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
)
const payment = PaymentOption.fromWallet(wallet)

// Generate keys
const mainKey = SecretKey.random()
const registerKey = Client.registerKeyFromName(mainKey, "my-register-1")
```

### Core Register Methods

#### `client.registerCost(publicKey)`
Calculate the cost to create a register.
- **Parameters**: `publicKey` - The public key for the register
- **Returns**: Promise resolving to cost in atto tokens

```js
const cost = await client.registerCost(registerKey.publicKey())
console.log(`Register creation cost: ${cost} AttoTokens`)
```

#### `client.registerCreate(key, content, payment)`
Create a new register on the network.
- **Parameters**: 
  - `key` - Secret key that owns this register
  - `content` - Initial content as RegisterValue
  - `payment` - Payment option
- **Returns**: Promise resolving to `{cost, addr}` object

```js
const content = Client.registerValueFromBytes(Buffer.from("Hello, World!"))
const {cost, addr} = await client.registerCreate(registerKey, content, payment)
console.log(`Register created at: ${addr.toHex()}`)
```

#### `client.registerGet(address)`
Retrieve current value of a register.
- **Parameters**: `address` - RegisterAddress to retrieve from
- **Returns**: Promise resolving to RegisterValue

```js
const currentValue = await client.registerGet(addr)
console.log(`Current value: ${currentValue}`)
```

#### `client.registerUpdate(key, content, payment)`
Update an existing register with new content.
- **Parameters**:
  - `key` - Secret key that owns the register
  - `content` - New content as RegisterValue  
  - `payment` - Payment option
- **Returns**: Promise resolving to cost in atto tokens

```js
const newContent = Client.registerValueFromBytes(Buffer.from("Updated!"))
const updateCost = await client.registerUpdate(registerKey, newContent, payment)
console.log(`Update cost: ${updateCost} AttoTokens`)
```

#### `client.registerHistory(address)`
Get the complete version history of a register.
- **Parameters**: `address` - RegisterAddress to get history for
- **Returns**: RegisterHistory iterator

```js
// Iterate through history
const history = client.registerHistory(addr)
let version = await history.next()
while (version !== null) {
  console.log(`Version: ${version}`)
  version = await history.next()
}

// Or collect all versions at once
const allVersions = await history.collect()
console.log(`History has ${allVersions.length} versions`)
```

### Utility Methods

#### `Client.registerKeyFromName(mainKey, name)`
Generate a deterministic register key from a main key and name.
- **Parameters**: 
  - `mainKey` - Main secret key
  - `name` - String name for the register
- **Returns**: Secret key for the register

```js
const registerKey = Client.registerKeyFromName(mainKey, "counter-1")
```

#### `Client.registerValueFromBytes(data)`
Create RegisterValue from byte data (limited to 32 bytes).
- **Parameters**: `data` - Buffer containing data (max 32 bytes)
- **Returns**: RegisterValue

```js
const content = Client.registerValueFromBytes(Buffer.from("counter: 42"))
```

### RegisterAddress Class

#### Constructor
```js
const address = new RegisterAddress(publicKey)
```

#### Methods

- `toHex()` - Convert register address to hex string
- `fromHex(hex)` - Create register address from hex string (static method)
- `owner()` - Get the public key that owns this register
- `toUnderlyingGraphRoot()` - Get underlying graph root address
- `toUnderlyingHeadPointer()` - Get underlying head pointer address

```js
// Create address from public key
const addr = new RegisterAddress(registerKey.publicKey())

// Convert to/from hex
const hex = addr.toHex()
const addr2 = RegisterAddress.fromHex(hex)

// Get owner
const owner = addr.owner()
```

### Complete Register Example

```js
import { Client, Wallet, Network, PaymentOption, SecretKey } from '@withautonomi/autonomi'

async function registerExample() {
  // Initialize client and wallet
  const client = await Client.initLocal()
  const wallet = Wallet.newFromPrivateKey(
    new Network(true), 
    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
  )
  const payment = PaymentOption.fromWallet(wallet)
  
  // Generate keys
  const mainKey = SecretKey.random()
  const registerKey = Client.registerKeyFromName(mainKey, "my-counter")
  
  // Create register content (max 32 bytes)
  const initialContent = Client.registerValueFromBytes(Buffer.from("counter: 0"))
  
  // Check cost before creating
  const cost = await client.registerCost(registerKey.publicKey())
  console.log(`Register creation cost: ${cost} AttoTokens`)
  
  // Create the register
  const {cost: creationCost, addr} = await client.registerCreate(
    registerKey, initialContent, payment
  )
  console.log(`Register created at: ${addr.toHex()}`)
  
  // Wait for network replication
  await new Promise(resolve => setTimeout(resolve, 5000))
  
  // Read current value
  const currentValue = await client.registerGet(addr)
  console.log(`Current value: ${currentValue}`)
  
  // Update the register
  const newContent = Client.registerValueFromBytes(Buffer.from("counter: 1"))
  const updateCost = await client.registerUpdate(registerKey, newContent, payment)
  console.log(`Update cost: ${updateCost} AttoTokens`)
  
  // Wait for replication
  await new Promise(resolve => setTimeout(resolve, 5000))
  
  // Get updated value
  const updatedValue = await client.registerGet(addr)
  console.log(`Updated value: ${updatedValue}`)
  
  // Get complete history
  const history = client.registerHistory(addr)
  const allVersions = await history.collect()
  console.log(`History has ${allVersions.length} versions:`)
  allVersions.forEach((version, i) => {
    console.log(`  Version ${i}: ${version}`)
  })
}

// Run the example
registerExample().catch(console.error)
```

### Register Use Cases

1. **Mutable Configuration**: Store application settings that need periodic updates
2. **Status Updates**: Maintain current status or state information  
3. **Version Control**: Track document or data versions with full history
4. **Counters**: Implement distributed counters or sequence numbers
5. **Metadata**: Store changeable metadata for files or applications

### Important Register Notes

1. **Size Limit**: Register values are limited to 32 bytes maximum
2. **Ownership**: Only the key holder can update a register
3. **Network Delays**: Allow time for network replication after operations (typically 5+ seconds)
4. **Deterministic Keys**: Use `Client.registerKeyFromName()` for consistent key generation
5. **Payment Required**: Both creation and updates require payment
6. **History Preserved**: All versions are permanently stored and accessible
7. **Error Handling**: 
   - Creating an existing register throws `AlreadyExists` error
   - Updating a non-existent register throws `CannotUpdateNewRegister` error

# Contributing, compilation and publishing

To contribute or develop on the source code directly, we need a few requirements.

- Yarn
  - `npm install --global yarn`
- We need the NAPI RS CLI tool
  - `yarn global add @napi-rs/cli`

Install the dependencies for the project:
```console
$ yarn install
```

## Build

Then build using the `napi` CLI:
```console
$ npx napi build
```

## Running tests

Run the `test` script:

```console
yarn test
# Or run a specific test
yarn test __test__/register.spec.mjs -m 'registers errors'
```

## Publishing

Before publishing, bump the versions of *all* packages with the following:
```console
$ npm version patch --no-git-tag-version
```

Use `major` or `minor` instead of `patch` depending on the release.

It's a good practice to have an unreleased version number ready to go. So if `0.4.0` is the version released on NPM currently, `package.json` should be at `0.4.1`.

### Workflow

Use the 'JS publish to NPM' workflow (`nodejs-publish.yml`) to publish the package from `main` or a tag. This workflow has to be manually dispatched through GitHub.
