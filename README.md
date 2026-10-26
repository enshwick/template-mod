# template-mod

The [copier](https://copier.readthedocs.io) template of an enshwick mod
repository: one mod crate on the `enshwick` API, its `cargo xtask check`, the
git hooks, CI with its nightly lane, the dev build and release-plz on the
organization runners, and the shared configuration (toolchain, rustfmt,
clippy, cargo-deny, lints, Renovate).

```bash
copier copy --trust gh:enshwick/template-mod mod-my-mod   # a new mod repository
copier update --trust                                     # in a mod repository: merge template changes
```

Then `bun install` once for the git hooks, and commit `bun.lock`.
