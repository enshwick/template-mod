# Runs the tests of the Windows-only crates off Windows: `cargo xtask check
# --windows`. The Wine prefix is created at build time so runs start fast.
# Debian's Wine, not WineHQ's newer stable release or GE-Proton's as on CI:
# Wine 11 and GE-Proton 11 abort making their prefix under the amd64
# emulation Docker uses on Apple silicon (a page size assertion,
# 2026-10-27), where Wine 10 works.
FROM --platform=linux/amd64 debian:trixie-slim@sha256:89c1ea91e897f0fe0a3731595330bc45b8bdef4047464a07116f891a94054b31
RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends wine64 wine \
    && rm -rf /var/lib/apt/lists/*
RUN WINEDEBUG=-all wineboot -i
