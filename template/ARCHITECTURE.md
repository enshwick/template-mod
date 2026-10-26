# Architecture

One crate, a Windows DLL the Enshwick loader loads from the game's `mods/`
folder. The mod holds only its own logic: what the game needs from it goes
through the `enshwick` API, and what another mod could use belongs in the
API, not here.

| Path | What |
| --- | --- |
| `src/` | the mod |
| `tools/xtask/` | `cargo xtask check`, the checks before a merge |
