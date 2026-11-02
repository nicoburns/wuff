This is wuff 0.2.9, licensed under the MIT license in `LICENSE`.

Ink limits WOFF2 decompressed streams and reconstructed fonts to 16 MiB and
exposes a lower caller-provided limit. It checks intermediate glyf, loca,
table, and header allocations before constructing them. Font collections are
limited to 128 faces and 256 tables. These changes keep untrusted application
fonts within Ink's per-registry memory budget.
