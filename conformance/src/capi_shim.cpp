// Shim exercising the wuff-capi C++ wrapper headers (woff2/decode.h and
// woff2/output.h) exactly as a C++ consumer of the woff2 decoding API would:
// via woff2::ComputeWOFF2FinalSize, woff2::WOFF2StringOut and
// woff2::ConvertWOFF2ToTTF. The result is returned to Rust through a small
// extern "C" interface.

#include <string>

#include <woff2/decode.h>

extern "C" {

// Decode `data` using the wuff-capi C++ wrapper API. On success, returns a
// malloc'd buffer (free with conformance_capi_free) and stores its length in
// *result_length. On failure, returns null and stores 0.
uint8_t* conformance_capi_decode(const uint8_t* data, size_t length,
                                 size_t* result_length) {
  *result_length = 0;
  size_t final_size = woff2::ComputeWOFF2FinalSize(data, length);
  std::string output;
  output.reserve(final_size);
  woff2::WOFF2StringOut out(&output);
  // Like real consumers of this API (e.g. the ots sanitiser), raise the
  // default 128MB output cap for fonts that decompress to something larger.
  if (final_size > out.MaxSize()) {
    out.SetMaxSize(final_size);
  }
  if (!woff2::ConvertWOFF2ToTTF(data, length, &out)) {
    return nullptr;
  }
  uint8_t* result = static_cast<uint8_t*>(malloc(out.Size()));
  if (result == nullptr) {
    return nullptr;
  }
  memcpy(result, output.data(), out.Size());
  *result_length = out.Size();
  return result;
}

void conformance_capi_free(uint8_t* ptr) { free(ptr); }

}  // extern "C"
