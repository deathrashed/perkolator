# Demo App Agent Guide

Rust workspace for a small CLI. Run `cargo test` before every commit.

## Conventions

- Keep modules under 400 lines.
- Errors use `thiserror`; never `unwrap()` outside tests.
