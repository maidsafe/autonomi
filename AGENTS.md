# Repository Guidelines

## Project Structure & Module Organization
The root `Cargo.toml` defines a Rust workspace spanning crates such as `ant-node` (node runtime), `ant-cli` (client CLI), `autonomi` (core API), `ant-node-manager` (controller CLI), and shared utilities in `test-utils` and `resources`. Node bindings live in `autonomi-nodejs` and `ant-node-nodejs` with their own `package.json` files. Rust source sits under each crate's `src/`; integration and property tests are under `tests/` or `proptest-regressions/`. Scripts supporting local networks and automation reside at the root (`test.sh`, `test_runner.py`, `Justfile`).

## Build, Test, and Development Commands
- `cargo check --workspace --all-features` verifies the full Rust workspace quickly.
- `cargo build --workspace --release` produces optimized binaries (`ant`, `antnode`, `antctl`, `evm-testnet`).
- `cargo run --bin evm-testnet` starts a local EVM node and emits credentials; pair it with `cargo run --bin antctl -- local run --build --clean --rewards-address <addr>` to bootstrap a 25-node local network.
- `cargo run --bin antctl -- status` inspects network health, and `cargo run --bin antctl -- local kill` cleans up.
- For Node bindings, run `yarn install` followed by `yarn build` or `yarn test` in `autonomi-nodejs/` (Ava test runner).

## Coding Style & Naming Conventions
Use standard Rust formatting: four-space indents, snake_case modules, CamelCase types. Before committing run `cargo fmt --all` and `cargo clippy --workspace --all-targets --all-features -D warnings` to respect workspace lint settings forbidding `unsafe` and failing on `unwrap`/`expect`. JavaScript/TypeScript code should follow the existing idioms in `autonomi-nodejs/src` and remain `eslint`-clean if tooling is added; generated artifacts belong under `npm/`.

## Testing Guidelines
Default to `cargo test --workspace --all-features`. Property and integration tests live in crate-specific `tests/` directories; keep names descriptive (e.g. `mod_name_behaviour.rs`). End-to-end scenarios require a local network: run the EVM node and `antctl local run`, then execute `ANT_PEERS=local SECRET_KEY=<key> ./test.sh` or targeted binaries. Ava-based Node tests run via `yarn test` and should mirror Rust fixtures where possible.

## Commit & Pull Request Guidelines
Commits typically follow Conventional Commit phrases (`feat(launchpad): …`, `fix(node): …`); match that style with clear scopes. Include only formatted code and passing tests, and reference issues in the body when relevant. Pull requests should describe the change, note any new commands or env vars, and link affected READMEs. Add screenshots or logs for UI or tooling updates, and remind reviewers of any manual network setup required for verification.
