# Contributing to SuperTask

Thanks for contributing.

## Development setup

Install Rust stable, Node.js 20+, and the Tauri platform prerequisites for your
operating system. From the repository root:

```text
npm ci
npm --prefix frontend ci
cargo test -p supertask-core
cargo test -p supertask-cli
cargo check -p supertask
npm --prefix frontend run build
```

The cloud reference server and its console are optional:

```text
cargo test -p supertask-cloud-server
npm --prefix cloud-console ci
npm --prefix cloud-console run build
```

Keep business logic in `supertask-core`; the Tauri crate is an IPC adapter.
Do not commit credentials, local databases, build output, screenshots, or
machine-specific paths.

## Pull requests

Explain the behavior change and include focused tests. Run `cargo fmt --all --
--check` before opening a pull request. Changes that affect the desktop UI
should include a short manual verification note.
