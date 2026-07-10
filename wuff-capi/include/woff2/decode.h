/* Copyright 2014 Google Inc. All Rights Reserved.

   Distributed under MIT license.
   See file LICENSE for detail or copy at https://opensource.org/licenses/MIT
*/

/* Library for converting WOFF2 format font files to their TTF versions.

   This is a header-only reimplementation of the decoding API from the woff2
   C++ library (https://github.com/google/woff2), provided by the wuff-capi
   Rust crate. The `woff2` namespace functions below are implemented on top
   of C symbols exported by the wuff-capi Rust library, which must be linked
   into the final binary.
*/

#ifndef WOFF2_WOFF2_DEC_H_
#define WOFF2_WOFF2_DEC_H_

#include <stddef.h>
#include <inttypes.h>
#include <woff2/output.h>

extern "C" {

/* Compute the size of the final uncompressed font by reading the
   totalSfntSize field of the WOFF2 header, or 0 on error. */
size_t wuff_woff2_compute_final_size(const uint8_t *data, size_t length);

/* Decompress a WOFF2 font into a newly-allocated buffer. On success, returns
   a pointer to the decompressed font and stores its length in *result_length.
   The buffer must be freed with wuff_woff2_free. On failure, returns NULL and
   stores 0 in *result_length. */
uint8_t *wuff_woff2_decode(const uint8_t *data, size_t length,
                           size_t *result_length);

/* Free a buffer previously returned by wuff_woff2_decode. `length` must be
   the value stored in *result_length by that call. */
void wuff_woff2_free(uint8_t *ptr, size_t length);

} // extern "C"

namespace woff2 {

// Compute the size of the final uncompressed font, or 0 on error.
inline size_t ComputeWOFF2FinalSize(const uint8_t *data, size_t length) {
  return wuff_woff2_compute_final_size(data, length);
}

// Decompresses the font into out. Returns true on success.
// Works even if WOFF2Header totalSfntSize is wrong.
// Please prefer this API.
inline bool ConvertWOFF2ToTTF(const uint8_t *data, size_t length,
                              WOFF2Out* out) {
  size_t result_length = 0;
  uint8_t *result = wuff_woff2_decode(data, length, &result_length);
  if (result == NULL) {
    return false;
  }
  bool ok = out->Write(result, result_length);
  wuff_woff2_free(result, result_length);
  return ok;
}

// Decompresses the font into the target buffer. The result_length should
// be the same as determined by ComputeFinalSize(). Returns true on successful
// decompression.
// DEPRECATED; please prefer the version that takes a WOFF2Out*
inline bool ConvertWOFF2ToTTF(uint8_t *result, size_t result_length,
                              const uint8_t *data, size_t length) {
  WOFF2MemoryOut out(result, result_length);
  return ConvertWOFF2ToTTF(data, length, &out);
}

} // namespace woff2

#endif  // WOFF2_WOFF2_DEC_H_
