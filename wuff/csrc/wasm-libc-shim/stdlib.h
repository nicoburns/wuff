/* Minimal <stdlib.h> replacement for bare WebAssembly targets (wasm32-unknown-unknown /
 * wasm32v1-none), which have no libc. Used only when building the vendored Brotli decoder for
 * those targets; see `build.rs`.
 *
 * There is no `malloc`/`free` on these targets, so Brotli's uses of them are redirected to
 * allocator shims implemented in Rust (`src/brotli/c_backend.rs`) on top of the global
 * allocator. Brotli only reaches these when no custom `brotli_alloc_func`/`brotli_free_func`
 * pair is supplied, i.e. via `BrotliDefaultAllocFunc`/`BrotliDefaultFreeFunc` and
 * `BrotliDecoderCreateInstance(NULL, NULL, NULL)`. */
#ifndef WUFF_WASM_LIBC_SHIM_STDLIB_H_
#define WUFF_WASM_LIBC_SHIM_STDLIB_H_

#include <stddef.h>

void* wuff_brotli_malloc(size_t size);
void wuff_brotli_free(void* ptr);

#define malloc wuff_brotli_malloc
#define free wuff_brotli_free

#endif /* WUFF_WASM_LIBC_SHIM_STDLIB_H_ */
