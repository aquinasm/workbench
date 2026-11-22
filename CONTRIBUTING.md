# Contributing to Workbench

Thank you for your interest in contributing to Workbench.

## Development Setup

1. Fork the repository and clone your fork locally.
2. Install prerequisites:
   - Node.js (v20+)
   - Rust (stable toolchain via `rustup`)
   - [Tauri v2 system dependencies](https://v2.tauri.app/start/prerequisites/) for your OS
3. Install dependencies and start the app:

```bash
npm install
npm run tauri dev
```

## Making Changes

1. Create a branch from `main` with a descriptive name (for example, `fix/service-port-conflict` or `feat/postgres-support`).
2. Keep pull requests focused on a single feature, bug fix, or improvement.
3. Run checks before opening a pull request:

```bash
# Frontend type and Svelte checks
npm run check

# Rust formatting, lints, and tests
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo test --manifest-path src-tauri/Cargo.toml
```

## Updating Runtime or Service Manifests

Downloadable runtimes and services are defined in:

- `manifests/runtimes.json`
- `manifests/services.json`

When adding or updating entries, verify that download URLs point to official upstream releases and test extraction on your platform.

## Pull Request Guidelines

- Describe what changed and why.
- Include screenshots or recordings for UI changes when helpful.
- Ensure all checks pass and no generated build artifacts are committed.

## License

By contributing to Workbench, you agree that your contributions will be licensed under the [MIT License](LICENSE).
