# Contributing

You need Rust (`rust-toolchain.toml` installs the pinned version through
rustup), [Bun](https://bun.sh) for the git hooks, cargo-nextest, and off
Windows [cargo-xwin](https://github.com/rust-cross/cargo-xwin).

```bash
bun install          # the git hooks
cargo xtask check    # clippy, tests and doctests; --all adds fmt, rustdoc, cargo-deny
cargo xtask dist     # the player's archive of each mod, in target/dist
```

A package that builds only for Windows sets `windows-only = true` under
`[package.metadata.enshwick]`. Off Windows it stays out of the host checks,
and `cargo xtask check --windows` (or `--all`) runs its tests under Wine:
directly on an amd64 Linux host that has Wine, otherwise in Docker, in an
image built from `tools/wine/Dockerfile` on first use. On Apple silicon the
container runs through Rosetta, which cannot run tests of crashes, detours,
patches and threads, so those are skipped there.

The hooks check formatting, spelling, Markdown and commit messages and
compile nothing. A pull request title is a conventional commit,
`<type>(<scope>): <subject>`, and becomes the squashed commit.
