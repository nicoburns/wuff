# Vendored Brotli decoder

This directory contains an unmodified subset of Google's reference Brotli implementation,
<https://github.com/google/brotli>, used by the `brotli-c` feature of `wuff` (see `build.rs`).

- Upstream version: **v1.2.0** (commit `028fb5a23661f123017c060daa546b55cf4bde29`, 2025-10-27)
- License: MIT (see `LICENSE`, copied from upstream)

Only the decoder and its shared support code are included:

| Here               | Upstream                                                            |
| ------------------ | ------------------------------------------------------------------- |
| `LICENSE`          | `LICENSE`                                                           |
| `include/brotli/`  | `c/include/brotli/{decode,port,shared_dictionary,types}.h`          |
| `common/`          | `c/common/*.c`, `c/common/*.h` (the `dictionary.bin*` blobs are omitted; the dictionary is embedded via `dictionary_inc.h`) |
| `dec/`             | `c/dec/*.c`, `c/dec/*.h`                                            |

## Updating

```sh
git clone --depth 1 --branch <tag> https://github.com/google/brotli.git /tmp/brotli
cd wuff/vendor/brotli
rm -rf include common dec LICENSE
mkdir -p include/brotli common dec
cp /tmp/brotli/LICENSE .
cp /tmp/brotli/c/include/brotli/{decode,port,shared_dictionary,types}.h include/brotli/
cp /tmp/brotli/c/common/*.c /tmp/brotli/c/common/*.h common/
cp /tmp/brotli/c/dec/*.c /tmp/brotli/c/dec/*.h dec/
```

Then update the version/commit above, check the `SOURCES` list in `build.rs` against the new
`common/*.c` and `dec/*.c` files, and check that the wasm libc shims in `csrc/wasm-libc-shim`
still cover every libc header and symbol the decoder uses (`grep '#include <'` and
`grep -w -E 'malloc|free|exit|abort'` over `common/` and `dec/`).
