# template-mod

The [copier](https://copier.readthedocs.io) template for an Enshwick mod
repository. A new repository gets:

- one mod crate on the `enshwick` API;
- `cargo xtask check`, the checks before a merge;
- the git hooks;
- CI with a nightly run, the dev build and release-plz;
- the shared configuration: toolchain, rustfmt, clippy, cargo-deny, lints and
  Renovate.

```bash
copier copy --trust gh:enshwick/template-mod mod-my-mod   # a new mod repository
copier update --trust                                     # in a mod repository: merge template changes
```

Then run `bun install` once for the git hooks, and commit `bun.lock`.

[shared-files.txt](shared-files.txt) lists the files a rendered repository
keeps exactly as rendered. CI fails a pull request whose copy differs.
