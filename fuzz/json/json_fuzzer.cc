// libFuzzer harness for runtime.c's jsonParse(). Links the real runtime
// (compiled with -DTINOX_NO_GC, same mode `make asan` uses, so plain
// malloc/calloc back every allocation — no Boehm GC / GC_INIT needed) and
// feeds it mutated byte strings. Crashes/hangs/ASan reports point straight
// at a real jsonParse() bug, no separate copy of the parser to keep in
// sync. See fuzz/README.md for build/run instructions.

#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <cstring>

extern "C" int64_t *jsonParse(const char *text);

// runtime.c's main() (renamed at compile time, see build.sh) calls into
// tinox_main()/__tinox_err — symbols codegen normally supplies for a real
// Tinox program. The renamed main is unreachable (libFuzzer's own main
// drives this binary) but still needs to link, so stub both out here.
extern "C" __thread int64_t __tinox_err = 0;
extern "C" int64_t tinox_main(void) { return 0; }

extern "C" int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size) {
    // jsonParse() takes a NUL-terminated C string (Tinox's String ABI),
    // not a (ptr, len) pair — libFuzzer's raw buffer isn't NUL-terminated
    // and may contain embedded NULs, both of which matter for exercising
    // the parser the same way real Tinox String values would.
    char *text = (char *)malloc(size + 1);
    if (!text) return 0;
    memcpy(text, data, size);
    text[size] = '\0';
    jsonParse(text);
    free(text);
    return 0;
}
