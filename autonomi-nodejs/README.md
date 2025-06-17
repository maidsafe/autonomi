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

# Contributing, compilation and publishing

To contribute or develop on the source code directly, Node.js must be installed (installation instructions [here](https://nodejs.org/en/download)).

With Node.js installed, change the working directory to `autonomi-nodejs/`:
```console
$ cd ./autonomi-nodejs/
```

Then install the dependencies for the project:
```console
$ npm install
```

## Build

Then build using the build script (which calls the `napi` CLI):
```console
$ npm run build
```

## Running tests

Run the `test` script:

```console
npm test
# Or run a specific test
npm test __test__/register.spec.mjs -m 'registers errors'
```

## Publishing

Before publishing, bump the versions of *all* packages with the following:
```console
$ npm version patch --no-git-tag-version
```

Use `major` or `minor` instead of `patch` depending on the release.

It's a good practice to have an unreleased version number ready to go. So if `0.4.0` is the version released on NPM currently, `package.json` should be at `0.4.1`.

### Workflow

Use the 'Node.js (release)' workflow (`nodejs-publish.yml`) to publish the package from `main` or a tag. This workflow has to be manually dispatched through GitHub.
