# Contributing to the Autonomi Network

:tada: Thank you for your interest in contributing to the Autonomi Network! :tada:

This document is a set of guidelines for contributing to the Autonomi Network. These are guidelines, not rules. This guide is designed to make it easy for you to get involved.

Notice something amiss? Have an idea for a new feature? Feel free to create an issue in this GitHub repository about anything that you feel could be fixed or improved. Examples include:

- Bugs, crashes
- Enhancement ideas
- Unclear documentation
- Lack of tutorials and hello world examples
- ... and more

See our [Issues and Feature Requests](#issues-and-feature-requests) section below for further information on creating new issues.

Of course, after submitting an issue you are free to assign it to yourself and tackle the problem, or pick up any of the other outstanding issues yet to be actioned - see the [Development](#development) section below for more information.

Further support is available [here](#support).

This project adheres to the [Contributor Covenant](https://www.contributor-covenant.org/). By participating, we sincerely hope that you honour this code.

## What we're working on

The best way to follow our progress is to read the development updates, which are published on the [Autonomi Community Forum](https://forum.autonomi.community/).

For more information about the project, visit the [Autonomi Documentation](https://docs.autonomi.com/).

## Issues and Feature Requests

Each MaidSafe repository should have a `bug report` and a `feature request` template option when creating a new issue, with guidance and required information specific to that repository detailed within. Opening an issue in each repository will auto-populate your issue with this template.

As per the issue templates, bug reports should clearly lay out the problem, platform(s) experienced on, as well as steps to reproduce the issue. This aids in fixing the issue and validating that the issue has indeed been fixed if the reproduction steps are followed. Feature requests should clearly explain what any proposed new feature would include, resolve or offer.

Each issue is labelled by the team depending on its type, typically the standard labels we use are:

- `bug`: the issue is a bug in the product
- `feature`: the issue is a new and non-existent feature to be implemented in the product
- `enhancement`: the issue is an enhancement to either an existing feature in the product or to the infrastructure around the development process of the product
- `blocked`: the issue cannot be resolved as it depends on a fix in any of its dependencies
- `good first issue`: an issue considered more accessible for any developer who would like to start contributing
- `help wanted`: an issue considered lower priority for the MaidSafe team, but one that would appear to be suitable for an outside developer who would like to contribute

These labels are meant as a soft guide, if you want to work on an issue which doesn't have a `good first issue` or `help wanted` label, by all means fill your boots!

## Development

At MaidSafe, we follow a common development process. We use [Git](https://git-scm.com/) as our [version control system](https://en.wikipedia.org/wiki/Version_control). We develop new features in separate Git branches, raise [pull requests](https://help.github.com/en/articles/about-pull-requests), put them under peer review, and merge them only after they pass QA checks and [continuous integration](https://en.wikipedia.org/wiki/Continuous_integration) (CI). We do not commit directly to the `main` branch.

For useful resources, please see:

- [Git basics](https://git-scm.com/book/en/v1/Getting-Started-Git-Basics) for Git beginners
- [Git best practices](https://sethrobertson.github.io/GitBestPractices/)

We ask that if you are working on a particular issue, you ensure that the issue is logged in the GitHub repository and you assign that issue to yourself to prevent duplication of work.

### Code Style

In our [Rust Programming Language](https://www.rust-lang.org/) repositories we follow standard Rust conventions. You should install `rustfmt` and `clippy` and run them before each of your Git commits.

For our non-Rust repositories we follow the standard lint suggestions, pre-linting before commit. We encourage our contributors to use a sensible naming convention, split their files up accordingly, and include accompanying tests.

### Commits

We use the [Conventional Commit](https://www.conventionalcommits.org/en/v1.0.0-beta.3/) message style, usually including a scope. You can have a look at the commit history within each repository to see examples of our commits.

All code should be pre-linted before commit. The use of pre-commit Git hooks is highly recommended to catch formatting and linting errors early.

### Pull Requests

If you are a newbie to pull requests (PRs), click [here](https://github.com/firstcontributions/first-contributions) for an easy-to-follow guide (with pictures!).

We follow the standard procedure for submitting PRs. Please refer to the [official GitHub documentation](https://help.github.com/articles/creating-a-pull-request/) if you are unfamiliar with the procedure. If you still need help, we are more than happy to guide you along!

We are in the process of adding pull request templates to each MaidSafe repository, with guidance specific to that repository detailed within. Opening a PR in each repository will auto-populate your PR with this template. PRs should clearly reference an issue when applicable. A PR that implements/fixes an issue is linked using one of the [GitHub keywords](https://help.github.com/articles/closing-issues-using-keywords).

Pull requests should strive to tackle one issue/feature, and code should be pre-linted before commit.

Each pull request's total lines changed should be <= 200 lines. This is calculated as `lines added` + `lines deleted`. Please split up any PRs which are larger than this, otherwise they may be rejected. A CI check has been added to fail PRs which are larger than 200 lines changed.

Ideally, a multi-commit PR should be a sequence of commits "telling a story", going in atomic and easily reviewable steps from the initial to the final state.

Each PR should be rebased on the latest upstream commit; avoid merging from the upstream branch into the feature branch/PR. This means that a PR will probably see one or more force-pushes to keep up to date with changes in the upstream branch.

Fixes to review comments should preferably be pushed as additional commits to make it easier for the reviewer to see the changes. As a final step once the reviewer is happy the author should consider squashing these fixes with the relevant commit.

Smaller PRs can have their commits squashed together and fast-forward merged, while larger PRs should probably have the chain of commits left intact and fast-forward merged into the upstream branch.

Where appropriate, commits should always contain tests for the code in question.

#### Running tests

Submitted PRs are expected to pass continuous integration (CI), which runs a test suite on your PR to make sure that your code has not regressed the code base.

To run tests locally, use the following commands for specific packages as they require different setups:

```bash
# Unit tests for specific packages
cargo test --release --package autonomi --lib
cargo test --release --package ant-bootstrap
cargo test --release --package ant-node --lib
cargo test --release --package node-launchpad
cargo test --release --package ant-networking --features="open-metrics"
cargo test --release --package ant-protocol
cargo test --release --package ant-logging

# E2E tests (requires local network setup using ant-local-testnet-action)
# The following tests are run in CI with a local testnet:
cargo test --package autonomi --tests -- --nocapture
cargo test --release -p ant-node --test data_with_churn -- --nocapture
cargo test --release -p ant-node --test verify_routing_table -- --nocapture
cargo test --release -p ant-node --test verify_data_location -- --nocapture

# Check code formatting
cargo fmt --all -- --check

# Run clippy linter
cargo clippy --all-targets --all-features -- -Dwarnings

# Check documentation
RUSTDOCFLAGS="--deny=warnings" cargo doc --no-deps --workspace --exclude=ant-cli
```

#### Code Review

Your PR will be automatically assigned to the team member(s) specified in the `codeowners` file, who may either review the PR himself/herself or assign it to another team member. More often than not, a code submission will be met with review comments and changes requested. It's nothing personal, but nobody's perfect; we leave each other review comments all the time.

Fixes to review comments should preferably be pushed as additional commits to make it easier for the reviewer to see the changes. As a final step once the reviewer is happy the author should consider squashing these fixes with the relevant commit.


## Releases and Changelog

The majority of our repositories have a Continuous Integration, Delivery & Deployment pipeline in place (CI/CD). Any PR raised must pass the automated CI tests and a peer review from a member of the team before being merged. Once merged there is no further manual involvement - the CD process kicks in and automatically increments the versioning according to the [Semantic Versioning specification](https://semver.org/), updates the Changelog, and deploys the latest code as appropriate for that repository. Every PR merged to main will result in a new release.

In repositories where CD has not been implemented yet, the release process is triggered by the maintainers of each repository, also with versioning increments according to the [Semantic Versioning specification](https://semver.org/). Releases are typically generated through our CI setup, which releases upon a trigger commit title (e.g. `Version change...`), or through specific programming language release tools such as `cargo release` or `yarn bump`.

Typically, for non CD repositories we only update/regenerate the [CHANGELOG file](CHANGELOG.md) with the latest changes on a new version release, where all changes since the last release are then added to the changelog file.

If a repository is for a library, or perhaps multiple libraries, then often no release artefact is produced. A tag would always be added to the repository on each release though, these tags can be viewed in the `/releases` page of each repository. Repositories which do produce artefacts, such as `.AppImage`, `.dmg` or `.exe` files, will have the release files available in the repository's `/release` page, or instructions there on how to obtain it.

## Development Process

PRs should target the `main` branch. Please use the [Conventional Commits](https://www.conventionalcommits.org/) specification for commit messages.

## Support

Contributors and users can get support through the following official channels:

- **GitHub issues**: Log an issue in the repository where you require support.
- **Discord**: https://discord.gg/autonomi - Join our Discord server for real-time discussions
- **Forum**: https://forum.autonomi.community/ - Community forum for discussions and support
- **Documentation**: https://docs.autonomi.com/ - Comprehensive project documentation
