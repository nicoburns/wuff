/* Minimal <string.h> replacement for bare WebAssembly targets (wasm32-unknown-unknown /
 * wasm32v1-none), which have no libc. Used only when building the vendored Brotli decoder for
 * those targets; see `build.rs`.
 *
 * These functions are compiler intrinsics. Rust's `compiler_builtins` crate defines them on bare
 * wasm targets, so they resolve at link time without any libc. */
#ifndef WUFF_WASM_LIBC_SHIM_STRING_H_
#define WUFF_WASM_LIBC_SHIM_STRING_H_

#include <stddef.h>

void* memcpy(void* dest, const void* src, size_t n);
void* memmove(void* dest, const void* src, size_t n);
void* memset(void* dest, int c, size_t n);
int memcmp(const void* a, const void* b, size_t n);

#endif /* WUFF_WASM_LIBC_SHIM_STRING_H_ */
