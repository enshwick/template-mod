# Runs the tests of the Windows-only crates off Windows: `cargo xtask check
# --windows`. The Wine prefix is created at build time so runs start fast.
FROM --platform=linux/amd64 debian:trixie-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad
RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends wine64 wine \
    && rm -rf /var/lib/apt/lists/*
RUN WINEDEBUG=-all wineboot -i
