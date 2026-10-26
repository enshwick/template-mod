# Runs the tests of the Windows-only crates off Windows: `cargo xtask check
# --windows`. The Wine prefix is created at build time so runs start fast.
FROM --platform=linux/amd64 debian:trixie-slim
RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends wine64 wine \
    && rm -rf /var/lib/apt/lists/*
RUN WINEDEBUG=-all wineboot -i
