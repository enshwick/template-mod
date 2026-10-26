# Contributing

You need Rust (`rust-toolchain.toml` installs the pinned version through
rustup), [Bun](https://bun.sh) for the git hooks, cargo-nextest, and off
Windows [cargo-xwin](https://github.com/rust-cross/cargo-xwin).

```bash
bun install          # the git hooks
cargo xtask check    # clippy, tests and doctests; --all adds fmt, rustdoc, cargo-deny
```

The hooks check formatting, spelling, Markdown and commit messages and
compile nothing. A pull request title is a conventional commit,
`<type>(<scope>): <subject>`, and becomes the squashed commit.
