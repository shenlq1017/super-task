# Changelog

All notable changes to SuperTask are documented here.

## [0.1.1] - 2026-09-03

### Features

- New eclipse-orbit app icon with matching browser favicon and unified
  run-operation icons.
- In-app auto-update now checks a cnb.cool mirror first (faster in
  mainland China) with GitHub Releases as fallback.

### Fixes

- Port placeholder detection now matches on port + working directory +
  program kind; foreign-owned placeholders prompt to change the port and
  block startup instead of being killed.
- Unified menu / tab / button icons and fixed mixed CJK-Latin text
  alignment in group titles.
- Hardened git tests (canonical temp roots, deterministic pull-conflict
  setup) and compiled the gateway probe on unix targets.

### Internal

- CI runs `cargo fmt --check`; release artifacts are mirrored to cnb.cool
  automatically.
- Dependency upgrades: windows 0.62.2 and consolidated minor bumps.

## [0.1.0] - 2026-09-02

Initial open-source release candidate.

- Desktop workbench for Spring Boot, Node, Python, Go, generic processes,
  Docker Compose, and gateway workflows.
- CLI and MCP integration.
- Aggregated logs, PTY terminal, health checks, workspace packages, README
  import, AI assistance, and optional cloud synchronization.
- Experimental self-hosted cloud reference server and admin console.

Known limitations are documented in the repository inventory and cloud server
specification.
