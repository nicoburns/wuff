/* Empty <sys/types.h> replacement for bare WebAssembly targets. Brotli's `common/platform.h`
 * includes it hoping to pick up `<endian.h>`, but clang already defines `__BYTE_ORDER__` for
 * wasm, which Brotli checks first. Nothing else from this header is used. */
#ifndef WUFF_WASM_LIBC_SHIM_SYS_TYPES_H_
#define WUFF_WASM_LIBC_SHIM_SYS_TYPES_H_
#endif /* WUFF_WASM_LIBC_SHIM_SYS_TYPES_H_ */
