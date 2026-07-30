// libFuzzer harness for runtime.c's ZIP reader (tinox_zip_parse(), reached
// via zipEntryCount()). Unlike jsonParse(), the zip*() functions take a
// filesystem path, not an in-memory buffer, so each iteration writes the
// fuzz input to an anonymous O_TMPFILE (no real directory entry, no
// filesystem races between parallel -fork workers, auto-reclaimed on
// close) and passes it in via the /proc/self/fd/<n> magic-symlink path.
//
// zipEntryCount() alone is enough to exercise the whole parser: it calls
// tinox_zip_parse() directly, which reads and decodes every local-file
// header eagerly (see runtime.c's tinox_zip_parse, the same function
// zipEntryName/zipEntrySize/zipExtractFile all call internally) rather
// than lazily on later calls. See fuzz/README.md.

#ifndef _GNU_SOURCE
#define _GNU_SOURCE // O_TMPFILE
#endif
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fcntl.h>
#include <unistd.h>

extern "C" int64_t zipEntryCount(const char *path);

// See fuzz/json/json_fuzzer.cc for why these two stubs are needed: the
// renamed runtime.c main() (see build.sh) references them.
extern "C" __thread int64_t __tinox_err = 0;
extern "C" int64_t tinox_main(void) { return 0; }

extern "C" int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size) {
    int fd = open("/tmp", O_TMPFILE | O_RDWR, 0600);
    if (fd < 0) return 0; // O_TMPFILE unsupported on this /tmp — nothing to fuzz
    if (size > 0) {
        size_t written = 0;
        while (written < size) {
            ssize_t n = write(fd, data + written, size - written);
            if (n <= 0) { close(fd); return 0; }
            written += (size_t)n;
        }
    }

    char path[64];
    snprintf(path, sizeof(path), "/proc/self/fd/%d", fd);
    zipEntryCount(path);

    close(fd);
    return 0;
}
