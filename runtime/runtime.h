// Tinox Runtime Header
#ifndef TINOX_RUNTIME_H
#define TINOX_RUNTIME_H

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>

// Memory management
void* tinox_alloc(size_t size);
void tinox_free(void* ptr);

// Print functions
void tinox_print_int(int64_t val);
void tinox_print_float(double val);
void tinox_print_string(const char* val);
void tinox_print_bool(bool val);
void tinox_print_newline(void);

// Panic handling
void tinox_panic(const char* msg);

// Array operations
void* tinox_array_alloc(size_t element_size, size_t length);

// String operations
int64_t tinox_string_length(const char* str);
char* tinox_string_concat(const char* a, const char* b);

#endif // TINOX_RUNTIME_H
