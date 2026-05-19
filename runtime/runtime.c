// Tinox Runtime

#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#include <ctype.h>
#include <math.h>
#include <pthread.h>
#include <unistd.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <netinet/tcp.h>
#include <arpa/inet.h>
#include <poll.h>
#include <sys/uio.h>
#include <signal.h>
#include <sys/epoll.h>
#include <time.h>

// Boehm GC — redirect all heap allocation through the collector
#define GC_THREADS
#include <gc.h>
#undef malloc
#undef calloc
#undef realloc
#undef free
#undef strdup
#define malloc(s)    GC_malloc(s)
#define calloc(n,s)  GC_malloc((size_t)(n)*(size_t)(s))
#define realloc(p,s) GC_realloc((p),(s))
#define free(p)      GC_free(p)
#define strdup(s)    GC_strdup(s)

// Memory allocation (kept for ABI compatibility — codegen calls these)
void* tinox_alloc(size_t size) {
    return GC_malloc(size);
}

void tinox_free(void* ptr) {
    GC_free(ptr);
}

// Print functions
void tinox_print_int(int64_t val) {
    printf("%ld", val);
}

void tinox_print_float(double val) {
    printf("%g", val);
}

void tinox_print_string(const char* val) {
    printf("%s", val);
}

void tinox_print_bool(bool val) {
    printf("%s", val ? "true" : "false");
}

void tinox_print_newline() {
    printf("\n");
}

// Panic/Error handling
void tinox_panic(const char* msg) {
    fprintf(stderr, "PANIC: %s\n", msg);
    exit(1);
}

// Array allocation
void* tinox_array_alloc(size_t element_size, size_t length) {
    return calloc(element_size * length, 1);
}

// String functions
int64_t tinox_string_length(const char* str) {
    int64_t len = 0;
    while (str[len] != '\0') len++;
    return len;
}

char* tinox_string_concat(const char* a, const char* b) {
    size_t len_a = strlen(a);
    size_t len_b = strlen(b);
    char* result = malloc(len_a + len_b + 1);
    memcpy(result, a, len_a);
    memcpy(result + len_a, b, len_b);
    result[len_a + len_b] = '\0';
    return result;
}

char* tinox_int_to_string(int64_t val) {
    char buf[32];
    int len = 0;
    int neg = val < 0;
    if (neg) val = -val;
    do { buf[len++] = '0' + (val % 10); val /= 10; } while (val > 0);
    if (neg) buf[len++] = '-';
    char* result = malloc(len + 1);
    for (int i = 0; i < len; i++) result[i] = buf[len - 1 - i];
    result[len] = '\0';
    return result;
}

char* tinox_float_to_string(double val) {
    char* result = malloc(32);
    snprintf(result, 32, "%g", val);
    return result;
}

int64_t* tinox_array_slice(int64_t* data, int64_t from, int64_t to) {
    int64_t len = to - from;
    if (len <= 0) { int64_t* r = (int64_t*)malloc(sizeof(int64_t)); r[0] = 0; return r + 1; }
    int64_t* raw = (int64_t*)malloc((len + 1) * sizeof(int64_t));
    raw[0] = len;
    int64_t* nd = raw + 1;
    for (int64_t i = 0; i < len; i++) nd[i] = data[from + i];
    return nd;
}

int64_t* tinox_array_push(int64_t* data, int64_t val) {
    int64_t len = data[-1];
    int64_t* raw = (int64_t*)malloc((len + 2) * sizeof(int64_t));
    raw[0] = len + 1;
    int64_t* nd = raw + 1;
    for (int64_t i = 0; i < len; i++) nd[i] = data[i];
    nd[len] = val;
    return nd;
}

int64_t* tinox_array_pop(int64_t* data) {
    int64_t len = data[-1];
    if (len == 0) return data;
    int64_t* raw = (int64_t*)malloc(len * sizeof(int64_t));
    raw[0] = len - 1;
    int64_t* nd = raw + 1;
    for (int64_t i = 0; i < len - 1; i++) nd[i] = data[i];
    return nd;
}

char* tinox_char_at(const char* s, int64_t i) {
    char* result = malloc(2);
    result[0] = s[i];
    result[1] = '\0';
    return result;
}

void tinox_print_char(int32_t c) {
    printf("%c", (char)c);
}

int64_t tinox_string_to_int(const char* s) {
    int64_t result = 0;
    int neg = 0;
    if (*s == '-') { neg = 1; s++; }
    while (*s >= '0' && *s <= '9') { result = result * 10 + (*s++ - '0'); }
    return neg ? -result : result;
}

double tinox_string_to_float(const char* s) {
    double result = 0.0, frac = 0.1;
    int neg = 0, in_frac = 0;
    if (*s == '-') { neg = 1; s++; }
    while (*s) {
        if (*s == '.') { in_frac = 1; }
        else if (in_frac) { result += (*s - '0') * frac; frac *= 0.1; }
        else { result = result * 10 + (*s - '0'); }
        s++;
    }
    return neg ? -result : result;
}

char* tinox_bool_to_string(int val) {
    const char* s = val ? "true" : "false";
    size_t len = val ? 4 : 5;
    char* result = malloc(len + 1);
    for (size_t i = 0; i <= len; i++) result[i] = s[i];
    return result;
}

// String utility functions
int64_t tinox_string_equals(const char* a, const char* b) {
    if (a == b) return 1;
    if (!a || !b) return 0;
    return strcmp(a, b) == 0 ? 1 : 0;
}

int64_t tinox_string_contains(const char* haystack, const char* needle) {
    return strstr(haystack, needle) != NULL ? 1 : 0;
}

int64_t tinox_string_index_of(const char* haystack, const char* needle) {
    const char* pos = strstr(haystack, needle);
    return pos ? (int64_t)(pos - haystack) : -1;
}

char* tinox_string_to_upper(const char* s) {
    size_t len = strlen(s);
    char* result = malloc(len + 1);
    for (size_t i = 0; i <= len; i++)
        result[i] = (s[i] >= 'a' && s[i] <= 'z') ? s[i] - 32 : s[i];
    return result;
}

char* tinox_string_to_lower(const char* s) {
    size_t len = strlen(s);
    char* result = malloc(len + 1);
    for (size_t i = 0; i <= len; i++)
        result[i] = (s[i] >= 'A' && s[i] <= 'Z') ? s[i] + 32 : s[i];
    return result;
}

int64_t tinox_string_starts_with(const char* s, const char* prefix) {
    if (!s || !prefix) return 0;
    size_t plen = strlen(prefix);
    return strncmp(s, prefix, plen) == 0 ? 1 : 0;
}

int64_t tinox_string_ends_with(const char* s, const char* suffix) {
    size_t slen = strlen(s), suflen = strlen(suffix);
    if (suflen > slen) return 0;
    return strcmp(s + slen - suflen, suffix) == 0 ? 1 : 0;
}

char* tinox_string_trim(const char* s) {
    while (*s == ' ' || *s == '\t' || *s == '\n' || *s == '\r') s++;
    size_t len = strlen(s);
    while (len > 0 && (s[len-1] == ' ' || s[len-1] == '\t' || s[len-1] == '\n' || s[len-1] == '\r')) len--;
    char* result = malloc(len + 1);
    memcpy(result, s, len);
    result[len] = '\0';
    return result;
}

// Array utility functions
static int cmp_i64(const void* a, const void* b) {
    int64_t x = *(const int64_t*)a, y = *(const int64_t*)b;
    return (x > y) - (x < y);
}

// Fast inlined int64 sort: insertion sort for small ranges, quicksort otherwise.
// No function-pointer overhead, no temp-buffer malloc (unlike glibc qsort/msort).
static inline void i64_swap(int64_t* a, int64_t* b) { int64_t t = *a; *a = *b; *b = t; }

static void sort_i64_range(int64_t* arr, int64_t lo, int64_t hi) {
    while (lo < hi) {
        if (hi - lo < 16) {
            // Insertion sort: optimal for small ranges, no overhead
            for (int64_t i = lo + 1; i <= hi; i++) {
                int64_t key = arr[i];
                int64_t j = i;
                while (j > lo && arr[j-1] > key) { arr[j] = arr[j-1]; j--; }
                arr[j] = key;
            }
            return;
        }
        // Median-of-3 pivot to avoid worst-case
        int64_t mid = lo + (hi - lo) / 2;
        if (arr[lo] > arr[mid]) i64_swap(&arr[lo], &arr[mid]);
        if (arr[lo] > arr[hi])  i64_swap(&arr[lo], &arr[hi]);
        if (arr[mid] > arr[hi]) i64_swap(&arr[mid], &arr[hi]);
        int64_t pivot = arr[mid];
        i64_swap(&arr[mid], &arr[hi-1]);
        int64_t i = lo, j = hi - 1;
        while (1) {
            while (arr[++i] < pivot);
            while (arr[--j] > pivot);
            if (i >= j) break;
            i64_swap(&arr[i], &arr[j]);
        }
        i64_swap(&arr[i], &arr[hi-1]);
        // Recurse on smaller partition to bound stack depth
        if (i - lo < hi - i) { sort_i64_range(arr, lo, i - 1); lo = i + 1; }
        else                  { sort_i64_range(arr, i + 1, hi); hi = i - 1; }
    }
}

static __thread int64_t* g_sort_buf = NULL;
static __thread int64_t  g_sort_cap = 0;

int64_t* tinox_array_sort(int64_t* data) {
    int64_t len = data[-1];
    if (len + 1 > g_sort_cap) {
        int64_t nc = g_sort_cap ? g_sort_cap * 2 : 256;
        while (nc < len + 1) nc *= 2;
        g_sort_buf = (int64_t*)realloc(g_sort_buf, (size_t)nc * sizeof(int64_t));
        g_sort_cap = nc;
    }
    g_sort_buf[0] = len;
    int64_t* nd = g_sort_buf + 1;
    memcpy(nd, data, (size_t)len * sizeof(int64_t));
    if (len > 1) sort_i64_range(nd, 0, len - 1);
    return nd;
}

int64_t* tinox_array_reverse(int64_t* data) {
    int64_t len = data[-1];
    int64_t* raw = malloc((len + 1) * sizeof(int64_t));
    raw[0] = len;
    int64_t* nd = raw + 1;
    for (int64_t i = 0; i < len; i++) nd[i] = data[len - 1 - i];
    return nd;
}

int64_t tinox_array_contains(int64_t* data, int64_t val) {
    int64_t len = data[-1];
    for (int64_t i = 0; i < len; i++) if (data[i] == val) return 1;
    return 0;
}

int64_t tinox_array_index_of(int64_t* data, int64_t val) {
    int64_t len = data[-1];
    for (int64_t i = 0; i < len; i++) if (data[i] == val) return i;
    return -1;
}

// ---- Async runtime ----

typedef struct {
    pthread_t thread;
} TinoxTask;

typedef struct TinoxChannelNode {
    int64_t value;
    struct TinoxChannelNode* next;
} TinoxChannelNode;

typedef struct {
    TinoxChannelNode* head;
    TinoxChannelNode* tail;
    pthread_mutex_t mutex;
    pthread_cond_t  cond;
} TinoxChannel;

void* tinox_task_spawn(void* (*fn)(void*), void* args) {
    TinoxTask* task = malloc(sizeof(TinoxTask));
    pthread_create(&task->thread, NULL, fn, args);
    return task;
}

int64_t tinox_task_await(void* handle) {
    TinoxTask* task = (TinoxTask*)handle;
    void* retval = NULL;
    pthread_join(task->thread, &retval);
    free(task);
    return (int64_t)(uintptr_t)retval;
}

void* tinox_channel_create(void) {
    TinoxChannel* ch = calloc(1, sizeof(TinoxChannel));
    pthread_mutex_init(&ch->mutex, NULL);
    pthread_cond_init(&ch->cond, NULL);
    return ch;
}

void tinox_channel_send(void* handle, int64_t value) {
    TinoxChannel* ch = (TinoxChannel*)handle;
    TinoxChannelNode* node = malloc(sizeof(TinoxChannelNode));
    node->value = value;
    node->next = NULL;
    pthread_mutex_lock(&ch->mutex);
    if (ch->tail) ch->tail->next = node; else ch->head = node;
    ch->tail = node;
    pthread_cond_signal(&ch->cond);
    pthread_mutex_unlock(&ch->mutex);
}

int64_t tinox_channel_recv(void* handle) {
    TinoxChannel* ch = (TinoxChannel*)handle;
    pthread_mutex_lock(&ch->mutex);
    while (!ch->head) pthread_cond_wait(&ch->cond, &ch->mutex);
    TinoxChannelNode* node = ch->head;
    ch->head = node->next;
    if (!ch->head) ch->tail = NULL;
    int64_t val = node->value;
    free(node);
    pthread_mutex_unlock(&ch->mutex);
    return val;
}

// Non-blocking recv: returns 1 and stores value if a message is ready, else returns 0.
int tinox_channel_try_recv(void* handle, int64_t* out) {
    TinoxChannel* ch = (TinoxChannel*)handle;
    pthread_mutex_lock(&ch->mutex);
    if (!ch->head) {
        pthread_mutex_unlock(&ch->mutex);
        return 0;
    }
    TinoxChannelNode* node = ch->head;
    ch->head = node->next;
    if (!ch->head) ch->tail = NULL;
    *out = node->value;
    free(node);
    pthread_mutex_unlock(&ch->mutex);
    return 1;
}

// ---- Map (open-addressing hash table, string keys, i64 values) ----

#define TINOX_MAP_INIT_CAP 16
#define TINOX_MAP_LOAD_NUM 3
#define TINOX_MAP_LOAD_DEN 4

typedef struct TinoxMapEntry {
    char*   key;   // NULL = empty slot, (char*)1 = tombstone
    int64_t value;
} TinoxMapEntry;

typedef struct TinoxMap {
    TinoxMapEntry* entries;
    size_t cap;
    size_t len;
    int borrowed_keys; // 1 = keys/entries are arena-owned, don't free
} TinoxMap;

static size_t map_hash(const char* key, size_t cap) {
    size_t h = 14695981039346656037ULL;
    for (const unsigned char* p = (const unsigned char*)key; *p; p++)
        h = (h ^ *p) * 1099511628211ULL;
    return h & (cap - 1);
}

static void map_rehash(TinoxMap* m); // forward declaration

// Reset a map without freeing keys/entries (for static maps with borrowed_keys=1)
static void tinox_map_reset(TinoxMap* m) {
    m->len = 0;
    memset(m->entries, 0, m->cap * sizeof(TinoxMapEntry));
}

// Store key without strdup — caller guarantees key lifetime exceeds map use
static void tinox_map_set_borrow(void* map, const char* key, int64_t value) {
    TinoxMap* m = (TinoxMap*)map;
    if (m->len * TINOX_MAP_LOAD_DEN >= m->cap * TINOX_MAP_LOAD_NUM) map_rehash(m);
    size_t idx = map_hash(key, m->cap);
    while (1) {
        char* k = m->entries[idx].key;
        if (!k || k == (char*)1) {
            m->entries[idx].key   = (char*)key; // no strdup
            m->entries[idx].value = value;
            m->len++;
            return;
        }
        if (strcmp(k, key) == 0) { m->entries[idx].value = value; return; }
        idx = (idx + 1) & (m->cap - 1);
    }
}

static void map_rehash(TinoxMap* m) {
    size_t new_cap = m->cap * 2;
    TinoxMapEntry* new_entries = calloc(new_cap, sizeof(TinoxMapEntry));
    for (size_t i = 0; i < m->cap; i++) {
        char* k = m->entries[i].key;
        if (!k || k == (char*)1) continue;
        size_t idx = map_hash(k, new_cap);
        while (new_entries[idx].key) idx = (idx + 1) & (new_cap - 1);
        new_entries[idx].key   = k;
        new_entries[idx].value = m->entries[i].value;
    }
    if (!m->borrowed_keys) free(m->entries);
    m->entries = new_entries;
    m->cap     = new_cap;
    m->borrowed_keys = 0; // entries now heap-allocated
}

void* tinox_map_create(void) {
    TinoxMap* m = malloc(sizeof(TinoxMap));
    m->cap          = TINOX_MAP_INIT_CAP;
    m->len          = 0;
    m->entries      = calloc(m->cap, sizeof(TinoxMapEntry));
    m->borrowed_keys = 0;
    return m;
}

void tinox_map_set(void* map, const char* key, int64_t value) {
    TinoxMap* m = (TinoxMap*)map;
    if (m->len * TINOX_MAP_LOAD_DEN >= m->cap * TINOX_MAP_LOAD_NUM)
        map_rehash(m);
    size_t idx = map_hash(key, m->cap);
    while (1) {
        char* k = m->entries[idx].key;
        if (!k || k == (char*)1) {
            m->entries[idx].key   = m->borrowed_keys ? (char*)key : strdup(key);
            m->entries[idx].value = value;
            m->len++;
            return;
        }
        if (strcmp(k, key) == 0) {
            m->entries[idx].value = value;
            return;
        }
        idx = (idx + 1) & (m->cap - 1);
    }
}

int64_t tinox_map_get(void* map, const char* key) {
    TinoxMap* m = (TinoxMap*)map;
    size_t idx = map_hash(key, m->cap);
    while (1) {
        char* k = m->entries[idx].key;
        if (!k) return 0;
        if (k != (char*)1 && strcmp(k, key) == 0) return m->entries[idx].value;
        idx = (idx + 1) & (m->cap - 1);
    }
}

int64_t tinox_map_contains(void* map, const char* key) {
    TinoxMap* m = (TinoxMap*)map;
    size_t idx = map_hash(key, m->cap);
    while (1) {
        char* k = m->entries[idx].key;
        if (!k) return 0;
        if (k != (char*)1 && strcmp(k, key) == 0) return 1;
        idx = (idx + 1) & (m->cap - 1);
    }
}

void tinox_map_remove(void* map, const char* key) {
    TinoxMap* m = (TinoxMap*)map;
    size_t idx = map_hash(key, m->cap);
    while (1) {
        char* k = m->entries[idx].key;
        if (!k) return;
        if (k != (char*)1 && strcmp(k, key) == 0) {
            free(m->entries[idx].key);
            m->entries[idx].key = (char*)1; // tombstone
            m->len--;
            return;
        }
        idx = (idx + 1) & (m->cap - 1);
    }
}

int64_t tinox_map_len(void* map) {
    return (int64_t)((TinoxMap*)map)->len;
}

int64_t* tinox_map_keys(void* map) {
    TinoxMap* m = (TinoxMap*)map;
    int64_t* raw = (int64_t*)malloc((m->len + 1) * sizeof(int64_t));
    raw[0] = (int64_t)m->len;
    int64_t* nd = raw + 1;
    size_t j = 0;
    for (size_t i = 0; i < m->cap; i++) {
        char* k = m->entries[i].key;
        if (k && k != (char*)1)
            nd[j++] = (int64_t)(uintptr_t)k;
    }
    return nd;
}

int64_t* tinox_map_values(void* map) {
    TinoxMap* m = (TinoxMap*)map;
    int64_t* raw = (int64_t*)malloc((m->len + 1) * sizeof(int64_t));
    raw[0] = (int64_t)m->len;
    int64_t* nd = raw + 1;
    size_t j = 0;
    for (size_t i = 0; i < m->cap; i++) {
        char* k = m->entries[i].key;
        if (k && k != (char*)1)
            nd[j++] = m->entries[i].value;
    }
    return nd;
}

void tinox_map_free(void* map) {
    TinoxMap* m = (TinoxMap*)map;
    if (m->borrowed_keys) return; // arena-owned memory, nothing to free
    for (size_t i = 0; i < m->cap; i++) {
        char* k = m->entries[i].key;
        if (k && k != (char*)1) free(k);
    }
    free(m->entries);
    free(m);
}

// Returns a partially masked version of a string:
// keeps up to 2 leading and 2 trailing chars, replaces the middle with "***".
// Short strings (len <= 4) are fully replaced with "***".
char* tinox_string_mask_partial(const char* s) {
    size_t len = strlen(s);
    if (len <= 4) {
        char* r = malloc(4); memcpy(r, "***", 4); return r;
    }
    size_t prefix = 2, suffix = 2;
    // result: prefix chars + "***" + suffix chars
    size_t rlen = prefix + 3 + suffix;
    char* result = malloc(rlen + 1);
    memcpy(result, s, prefix);
    memcpy(result + prefix, "***", 3);
    memcpy(result + prefix + 3, s + len - suffix, suffix);
    result[rlen] = '\0';
    return result;
}

char* tinox_string_substring(const char* s, int64_t from, int64_t to) {
    int64_t len = (int64_t)strlen(s);
    if (from < 0) from = 0;
    if (to > len) to = len;
    if (from >= to) { char* r = malloc(1); r[0] = '\0'; return r; }
    int64_t slen = to - from;
    char* result = malloc(slen + 1);
    memcpy(result, s + from, slen);
    result[slen] = '\0';
    return result;
}

char* tinox_string_replace(const char* s, const char* from, const char* to) {
    if (!from || !*from) { size_t l = strlen(s); char* r = malloc(l+1); memcpy(r,s,l+1); return r; }
    size_t flen = strlen(from), tlen = strlen(to), slen = strlen(s);
    // Count occurrences
    size_t count = 0;
    const char* p = s;
    while ((p = strstr(p, from)) != NULL) { count++; p += flen; }
    if (count == 0) { char* r = malloc(slen+1); memcpy(r,s,slen+1); return r; }
    size_t rlen = slen + count * (tlen - flen);
    char* result = malloc(rlen + 1);
    char* out = result;
    p = s;
    const char* found;
    while ((found = strstr(p, from)) != NULL) {
        size_t pre = (size_t)(found - p);
        memcpy(out, p, pre); out += pre;
        memcpy(out, to, tlen); out += tlen;
        p = found + flen;
    }
    size_t rest = strlen(p);
    memcpy(out, p, rest);
    out[rest] = '\0';
    return result;
}

// ---- String split / Array join ----

int64_t* tinox_string_split(const char* str, const char* delim) {
    size_t dlen = strlen(delim);
    size_t count = 1;
    if (dlen > 0) {
        const char* p = str;
        while ((p = strstr(p, delim)) != NULL) { count++; p += dlen; }
    } else {
        count = strlen(str);
        if (count == 0) count = 1;
    }
    int64_t* raw = (int64_t*)malloc((count + 1) * sizeof(int64_t));
    raw[0] = (int64_t)count;
    int64_t* nd = raw + 1;
    if (dlen == 0) {
        for (size_t i = 0; i < count; i++) {
            char* s = (char*)malloc(2);
            s[0] = str[i]; s[1] = '\0';
            nd[i] = (int64_t)(uintptr_t)s;
        }
        return nd;
    }
    size_t i = 0;
    const char* start = str;
    const char* found;
    while ((found = strstr(start, delim)) != NULL) {
        size_t plen = (size_t)(found - start);
        char* part = (char*)malloc(plen + 1);
        memcpy(part, start, plen); part[plen] = '\0';
        nd[i++] = (int64_t)(uintptr_t)part;
        start = found + dlen;
    }
    size_t plen = strlen(start);
    char* part = (char*)malloc(plen + 1);
    memcpy(part, start, plen); part[plen] = '\0';
    nd[i] = (int64_t)(uintptr_t)part;
    return nd;
}

char* tinox_string_join(int64_t* arr, const char* sep) {
    int64_t len = arr[-1];
    if (len == 0) { char* r = (char*)malloc(1); r[0] = '\0'; return r; }
    size_t seplen = strlen(sep);
    size_t total = 0;
    for (int64_t i = 0; i < len; i++) {
        const char* s = (const char*)(uintptr_t)arr[i];
        total += strlen(s);
        if (i < len - 1) total += seplen;
    }
    char* result = (char*)malloc(total + 1);
    char* p = result;
    for (int64_t i = 0; i < len; i++) {
        const char* s = (const char*)(uintptr_t)arr[i];
        size_t slen = strlen(s);
        memcpy(p, s, slen); p += slen;
        if (i < len - 1) { memcpy(p, sep, seplen); p += seplen; }
    }
    *p = '\0';
    return result;
}

// ---- File I/O ----

void* tinox_file_open(const char* path, const char* mode) {
    FILE* f = fopen(path, mode);
    return (void*)f;
}

void tinox_file_close(void* handle) {
    if (handle) fclose((FILE*)handle);
}

char* tinox_file_read(void* handle) {
    if (!handle) return (char*)tinox_alloc(1);
    FILE* f = (FILE*)handle;
    fseek(f, 0, SEEK_END);
    long size = ftell(f);
    fseek(f, 0, SEEK_SET);
    char* buf = (char*)tinox_alloc(size + 1);
    fread(buf, 1, size, f);
    buf[size] = '\0';
    return buf;
}

char* tinox_file_readline(void* handle) {
    if (!handle) return (char*)tinox_alloc(1);
    FILE* f = (FILE*)handle;
    size_t cap = 256;
    char* buf = (char*)tinox_alloc(cap);
    size_t len = 0;
    int c;
    while ((c = fgetc(f)) != EOF && c != '\n') {
        if (len + 1 >= cap) {
            cap *= 2;
            char* nb = (char*)tinox_alloc(cap);
            memcpy(nb, buf, len);
            free(buf);
            buf = nb;
        }
        buf[len++] = (char)c;
    }
    buf[len] = '\0';
    return buf;
}

void tinox_file_write(void* handle, const char* s) {
    if (handle) fputs(s, (FILE*)handle);
}

int64_t tinox_file_eof(void* handle) {
    if (!handle) return 1;
    return feof((FILE*)handle) ? 1 : 0;
}

int64_t tinox_file_exists(const char* path) {
    FILE* f = fopen(path, "r");
    if (f) { fclose(f); return 1; }
    return 0;
}

void tinox_file_delete(const char* path) {
    remove(path);
}

static size_t fast_i64_write(int64_t val, char* buf);

// ---- HTTP Server ----

int64_t httpServerCreate(int64_t port) {
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) return -1;
    int opt = 1;
    setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt));
    setsockopt(fd, SOL_SOCKET, SO_REUSEPORT, &opt, sizeof(opt));
    struct sockaddr_in addr = {0};
    addr.sin_family      = AF_INET;
    addr.sin_addr.s_addr = INADDR_ANY;
    addr.sin_port        = htons((uint16_t)port);
    if (bind(fd, (struct sockaddr*)&addr, sizeof(addr)) < 0) { close(fd); return -1; }
    if (listen(fd, 4096) < 0) { close(fd); return -1; }
    return (int64_t)fd;
}

int64_t httpServerAcceptConn(int64_t server_fd) {
    struct sockaddr_in client = {0};
    socklen_t len = sizeof(client);
    int fd = accept((int)server_fd, (struct sockaddr*)&client, &len);
    if (fd >= 0) {
        int one = 1;
        setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &one, sizeof(one));
        struct timeval tv = { .tv_sec = 5, .tv_usec = 0 }; // 5s zombie guard (poll handles keep-alive)
        setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
    }
    return (int64_t)fd;
}

// Static recv buffer — reused across requests, grows as needed
static __thread char*  g_recv_buf = NULL;
static __thread size_t g_recv_cap = 0;

// Reads a full HTTP/1.1 request from the socket into g_recv_buf (static, not freed by caller).
char* httpServerReadRequest(int64_t client_fd) {
    if (!g_recv_buf) { g_recv_cap = 4096; g_recv_buf = (char*)malloc(g_recv_cap); }
    size_t used = 0;
    char* buf = g_recv_buf;
    size_t cap = g_recv_cap;
    int fd = (int)client_fd;
    while (1) {
        if (used + 1 >= cap) {
            cap *= 2;
            buf = (char*)realloc(buf, cap);
            g_recv_buf = buf; g_recv_cap = cap;
        }
        ssize_t n = recv(fd, buf + used, cap - used - 1, 0);
        if (n <= 0) break;
        used += (size_t)n;
        buf[used] = '\0';
        // Stop once we have the full headers (and body if Content-Length matches)
        char* hdr_end = strstr(buf, "\r\n\r\n");
        if (!hdr_end) continue;
        // Check for Content-Length — scan line by line, no strcasestr overhead
        char* cl = NULL;
        for (char* s = buf; *s; ) {
            while (*s && *s != '\n') s++;
            if (*s) s++;
            if ((s[0]=='C'||s[0]=='c') && (s[1]=='o'||s[1]=='O') && (s[2]=='n'||s[2]=='N') &&
                (s[3]=='t'||s[3]=='T') && (s[4]=='e'||s[4]=='E') && (s[5]=='n'||s[5]=='N') &&
                (s[6]=='t'||s[6]=='T') &&  s[7]=='-' &&
                (s[8]=='L'||s[8]=='l') && (s[9]=='e'||s[9]=='E') && (s[10]=='n'||s[10]=='N') &&
                (s[11]=='g'||s[11]=='G') && (s[12]=='t'||s[12]=='T') && (s[13]=='h'||s[13]=='H') &&
                s[14]==':') { cl = s; break; }
        }
        if (cl) {
            long body_len = atol(cl + 15);
            long header_len = (long)(hdr_end - buf) + 4;
            long total = header_len + body_len;
            while ((long)used < total) {
                while (cap < (size_t)total + 1) {
                cap *= 2; buf = (char*)realloc(buf, cap);
                g_recv_buf = buf; g_recv_cap = cap;
            }
                ssize_t m = recv(fd, buf + used, (size_t)(total - (long)used), 0);
                if (m <= 0) break;
                used += (size_t)m;
                buf[used] = '\0';
            }
        }
        break;
    }
    buf[used] = '\0';
    return buf;
}

// Sends a raw HTTP response string and returns.
void httpServerSendRaw(int64_t client_fd, const char* data) {
    if (!data) return;
    size_t len = strlen(data);
    size_t sent = 0;
    while (sent < len) {
        ssize_t n = send((int)client_fd, data + sent, len - sent, MSG_NOSIGNAL);
        if (n <= 0) break;
        sent += (size_t)n;
    }
}

void httpServerCloseConn(int64_t client_fd) {
    close((int)client_fd);
}

void httpServerClose(int64_t server_fd) {
    close((int)server_fd);
}

// ---- HttpServer route-based API ----

#define TINOX_MAX_ROUTES 64
#define TINOX_MAX_BODY   (4 * 1024 * 1024)  /* 4 MB */

typedef void (*TinoxRouteHandler)(int64_t ctx);

typedef struct {
    char method[8];
    char path[256];
    TinoxRouteHandler handler;
} TinoxRoute;

typedef struct {
    int64_t port;
    TinoxRoute routes[TINOX_MAX_ROUTES];
    int route_count;
} TinoxHttpServer;

extern void* tinox_alloc(size_t size);
extern void* tinox_map_create(void);

int64_t* tinox_HttpServer_new(int64_t port) {
    TinoxHttpServer* srv = (TinoxHttpServer*)malloc(sizeof(TinoxHttpServer));
    memset(srv, 0, sizeof(TinoxHttpServer));
    srv->port = port;
    return (int64_t*)srv;
}

static void http_server_add_route(int64_t* server, const char* method, char* path, int64_t handler) {
    TinoxHttpServer* srv = (TinoxHttpServer*)server;
    if (srv->route_count < TINOX_MAX_ROUTES) {
        strncpy(srv->routes[srv->route_count].method, method, 7);
        strncpy(srv->routes[srv->route_count].path, path, 255);
        srv->routes[srv->route_count].handler = (TinoxRouteHandler)(intptr_t)handler;
        srv->route_count++;
    }
}

void tinox_HttpServer_get(int64_t* server, char* path, int64_t handler) {
    http_server_add_route(server, "GET", path, handler);
}

void tinox_HttpServer_post(int64_t* server, char* path, int64_t handler) {
    http_server_add_route(server, "POST", path, handler);
}

void tinox_HttpServer_put(int64_t* server, char* path, int64_t handler) {
    http_server_add_route(server, "PUT", path, handler);
}

void tinox_HttpServer_patch(int64_t* server, char* path, int64_t handler) {
    http_server_add_route(server, "PATCH", path, handler);
}

void tinox_HttpServer_delete(int64_t* server, char* path, int64_t handler) {
    http_server_add_route(server, "DELETE", path, handler);
}

static const char* http_status_text(int64_t code) {
    switch (code) {
        case 200: return "OK";
        case 201: return "Created";
        case 204: return "No Content";
        case 400: return "Bad Request";
        case 401: return "Unauthorized";
        case 403: return "Forbidden";
        case 404: return "Not Found";
        case 405: return "Method Not Allowed";
        case 415: return "Unsupported Media Type";
        case 500: return "Internal Server Error";
        default:  return "OK";
    }
}

static int route_matches(const char* pattern, const char* path, void* params_map) {
    while (*pattern && *path) {
        if (*pattern == ':') {
            // Parse parameter name
            const char* pname = pattern + 1;
            const char* pend  = pname;
            while (*pend && *pend != '/') pend++;
            char pname_buf[64];
            size_t nlen = (size_t)(pend - pname);
            if (nlen >= sizeof(pname_buf)) nlen = sizeof(pname_buf) - 1;
            memcpy(pname_buf, pname, nlen);
            pname_buf[nlen] = '\0';
            // Extract value from path
            const char* vend = path;
            while (*vend && *vend != '/') vend++;
            size_t vlen = (size_t)(vend - path);
            char* val = (char*)malloc(vlen + 1);
            memcpy(val, path, vlen);
            val[vlen] = '\0';
            if (params_map) tinox_map_set(params_map, pname_buf, (int64_t)(uintptr_t)val);
            pattern = pend;
            path    = vend;
        } else if (*pattern == *path) {
            pattern++; path++;
        } else {
            return 0;
        }
    }
    // Skip trailing slash on pattern
    while (*pattern == '/') pattern++;
    while (*path   == '/') path++;
    return *pattern == '\0' && *path == '\0';
}

// Thread-local response buffer — reused across requests per thread
static __thread char*  g_resp_buf = NULL;
static __thread size_t g_resp_cap = 0;

// Thread-local per-request structs — reused each request, one set per thread
static __thread int64_t  g_response[3];
static __thread int64_t  g_request[6];
static __thread int64_t  g_ctx[2];
static __thread char     g_empty_body[1];
static __thread TinoxMap g_req_headers_map;
static __thread TinoxMap g_resp_headers_map;
static __thread TinoxMap g_path_params_map;
static __thread int      g_thread_inited = 0;

static TinoxMap* make_static_map(TinoxMap* m, size_t cap) {
    m->entries      = (TinoxMapEntry*)calloc(cap, sizeof(TinoxMapEntry));
    m->cap          = cap;
    m->len          = 0;
    m->borrowed_keys = 1;
    return m;
}

static void thread_local_init(void) {
    if (g_thread_inited) return;
    g_resp_cap = 4096; g_resp_buf = (char*)malloc(g_resp_cap);
    make_static_map(&g_req_headers_map,  16);
    make_static_map(&g_resp_headers_map, 8);
    make_static_map(&g_path_params_map,  8);
    g_empty_body[0] = '\0';
    g_thread_inited = 1;
}

// Normalize HTTP header key to canonical Title-Case-Hyphenated form in place.
// e.g. "content-type" → "Content-Type", "ACCEPT" → "Accept"
static void normalize_header_key(char* k) {
    int cap_next = 1;
    for (; *k; k++) {
        if (*k == '-') { cap_next = 1; }
        else if (cap_next) { *k = (char)toupper((unsigned char)*k); cap_next = 0; }
        else { *k = (char)tolower((unsigned char)*k); }
    }
}

static void tinox_handle_one(TinoxHttpServer* srv, int64_t client_fd, int* keep_alive_out) {
    *keep_alive_out = 0;
    char* raw_req = httpServerReadRequest(client_fd);
    if (!raw_req || !raw_req[0]) return;

    char method[8];
    char path[256];
    char* query = (char*)"";
    {
        const char* sp = strchr(raw_req, ' ');
        if (sp) {
            int mlen = (int)(sp - raw_req); if (mlen > 7) mlen = 7;
            memcpy(method, raw_req, mlen); method[mlen] = '\0';
            sp++;
            const char* ep = sp;
            while (*ep && *ep != ' ' && *ep != '\r' && *ep != '\n' && (ep - sp) < 255) ep++;
            int plen = (int)(ep - sp);
            memcpy(path, sp, plen); path[plen] = '\0';
        } else {
            method[0] = '\0'; path[0] = '\0';
        }
    }
    char* qmark = strchr(path, '?');
    if (qmark) { query = qmark + 1; *qmark = '\0'; }

    tinox_map_reset(&g_path_params_map);
    TinoxRouteHandler handler = NULL;
    for (int i = 0; i < srv->route_count; i++) {
        if (strcmp(srv->routes[i].method, method) != 0) continue;
        if (route_matches(srv->routes[i].path, path, &g_path_params_map)) {
            handler = srv->routes[i].handler;
            break;
        }
        tinox_map_reset(&g_path_params_map);
    }

    // Parse HTTP headers — normalize keys to Title-Case, track Connection header
    tinox_map_reset(&g_req_headers_map);
    int req_close = 0; // HTTP/1.1 default: keep-alive
    char* hdr_line = strchr(raw_req, '\n');
    while (hdr_line) {
        hdr_line++;
        if (*hdr_line == '\r' || *hdr_line == '\n' || *hdr_line == '\0') break;
        char* colon = strchr(hdr_line, ':');
        char* eol = strchr(hdr_line, '\n');
        if (colon && eol && colon < eol) {
            *colon = '\0';
            char* hkey = hdr_line;
            normalize_header_key(hkey);
            char* vstart = colon + 1;
            while (*vstart == ' ') vstart++;
            size_t vlen = (size_t)(eol - vstart);
            while (vlen > 0 && (vstart[vlen-1] == '\r' || vstart[vlen-1] == ' ')) vlen--;
            vstart[vlen] = '\0';
            if (strcmp(hkey, "Connection") == 0 && strcmp(vstart, "close") == 0) req_close = 1;
            tinox_map_set_borrow(&g_req_headers_map, hkey, (int64_t)vstart);
        }
        hdr_line = eol;
    }

    char* req_body = "";
    if (hdr_line) {
        if (*hdr_line == '\r') req_body = hdr_line + 2;
        else if (*hdr_line == '\n') req_body = hdr_line + 1;
    }

    tinox_map_reset(&g_resp_headers_map);
    g_response[0] = handler ? 200 : 404;
    g_response[1] = (int64_t)&g_resp_headers_map;
    g_response[2] = (int64_t)g_empty_body;

    g_request[0] = (int64_t)method;
    g_request[1] = (int64_t)path;
    g_request[2] = (int64_t)req_body;
    g_request[3] = (int64_t)&g_req_headers_map;
    g_request[4] = (int64_t)query;
    g_request[5] = (int64_t)&g_path_params_map;

    g_ctx[0] = (int64_t)g_request;
    g_ctx[1] = (int64_t)g_response;

    if (handler) handler((int64_t)g_ctx);

    char* body = (char*)g_response[2];
    if (!body) body = "";
    int64_t status = g_response[0];
    void* resp_hdr_map = (void*)g_response[1];
    char hdr_buf[4096];
    size_t hdr_off = 0;
    TinoxMap* rhm = (TinoxMap*)resp_hdr_map;
    if (rhm && rhm->len > 0) {
        int64_t* hkeys = tinox_map_keys(resp_hdr_map);
        int64_t hklen = hkeys ? hkeys[-1] : 0;
        for (int64_t hi = 0; hi < hklen; hi++) {
            const char* hk = (const char*)(uintptr_t)hkeys[hi];
            const char* hv = (const char*)(uintptr_t)tinox_map_get(resp_hdr_map, hk);
            if (hk && hv) {
                size_t kl = strlen(hk), vl = strlen(hv);
                if (hdr_off + kl + vl + 4 < sizeof(hdr_buf)) {
                    memcpy(hdr_buf + hdr_off, hk, kl); hdr_off += kl;
                    hdr_buf[hdr_off++] = ':'; hdr_buf[hdr_off++] = ' ';
                    memcpy(hdr_buf + hdr_off, hv, vl); hdr_off += vl;
                    hdr_buf[hdr_off++] = '\r'; hdr_buf[hdr_off++] = '\n';
                }
            }
        }
        free(hkeys - 1);
    }
    if (tinox_map_contains(resp_hdr_map, "Content-Type") == 0) {
        static const char ct[] = "Content-Type: application/json\r\n";
        memcpy(hdr_buf + hdr_off, ct, sizeof(ct) - 1);
        hdr_off += sizeof(ct) - 1;
    }
    // Connection header
    static const char conn_ka[]    = "Connection: keep-alive\r\n";
    static const char conn_close[] = "Connection: close\r\n";
    const char* conn_hdr     = req_close ? conn_close : conn_ka;
    size_t      conn_hdr_len = req_close ? (sizeof(conn_close) - 1) : (sizeof(conn_ka) - 1);

    size_t body_len = strlen(body);
    const char* status_text = http_status_text(status);
    size_t st_len = strlen(status_text);
    size_t resp_cap = 9 + 3 + 1 + st_len + 2 + hdr_off + 16 + 20 + 2 + conn_hdr_len + 2 + body_len + 1;
    if (resp_cap > g_resp_cap) {
        while (g_resp_cap < resp_cap) g_resp_cap *= 2;
        g_resp_buf = (char*)realloc(g_resp_buf, g_resp_cap);
    }
    char* http_resp = g_resp_buf;
    char* out = http_resp;
    memcpy(out, "HTTP/1.1 ", 9); out += 9;
    out[0] = '0' + (char)(status / 100);
    out[1] = '0' + (char)(status / 10 % 10);
    out[2] = '0' + (char)(status % 10);
    out[3] = ' '; out += 4;
    memcpy(out, status_text, st_len); out += st_len;
    out[0] = '\r'; out[1] = '\n'; out += 2;
    memcpy(out, hdr_buf, hdr_off); out += hdr_off;
    memcpy(out, "Content-Length: ", 16); out += 16;
    out += fast_i64_write((int64_t)body_len, out);
    out[0] = '\r'; out[1] = '\n'; out += 2;
    memcpy(out, conn_hdr, conn_hdr_len); out += conn_hdr_len;
    out[0] = '\r'; out[1] = '\n'; out += 2;
    memcpy(out, body, body_len); out += body_len;
    size_t resp_total = (size_t)(out - http_resp);

    // Send with pre-computed length (no strlen)
    size_t sent_bytes = 0;
    while (sent_bytes < resp_total) {
        ssize_t n = send((int)client_fd, http_resp + sent_bytes, resp_total - sent_bytes, MSG_NOSIGNAL);
        if (n <= 0) break;
        sent_bytes += (size_t)n;
    }
    *keep_alive_out = !req_close;
}

// Per-connection state for epoll-based multi-connection handler
#define EPOLL_MAX_CONNS 4096
#define EPOLL_KEEP_ALIVE_MS 500  // close idle connections after 500ms

typedef struct {
    int      fd;          // -1 = unused slot
    uint64_t last_ms;     // last activity timestamp (milliseconds)
} EpollConnSlot;

static __thread EpollConnSlot g_epoll_slots[EPOLL_MAX_CONNS];
static __thread int           g_epoll_nconns = 0;  // number of active client connections

static uint64_t epoll_now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC_COARSE, &ts);
    return (uint64_t)ts.tv_sec * 1000 + (uint64_t)(ts.tv_nsec / 1000000);
}

static void epoll_slot_add(int fd) {
    int idx = fd % EPOLL_MAX_CONNS;
    if (g_epoll_slots[idx].fd < 0) g_epoll_nconns++;
    g_epoll_slots[idx].fd = fd;
    g_epoll_slots[idx].last_ms = epoll_now_ms();
}

static void epoll_slot_remove(int fd) {
    int idx = fd % EPOLL_MAX_CONNS;
    if (g_epoll_slots[idx].fd >= 0) g_epoll_nconns--;
    g_epoll_slots[idx].fd = -1;
}

static void tinox_handle_connections(TinoxHttpServer* srv, int64_t server_fd) {
    thread_local_init();

    // Initialize slot table
    for (int i = 0; i < EPOLL_MAX_CONNS; i++) g_epoll_slots[i].fd = -1;

    int epfd = epoll_create1(EPOLL_CLOEXEC);
    if (epfd < 0) { perror("epoll_create1"); return; }

    // Register server socket for incoming connections
    struct epoll_event ev;
    ev.events  = EPOLLIN;
    ev.data.fd = (int)server_fd;
    epoll_ctl(epfd, EPOLL_CTL_ADD, (int)server_fd, &ev);

    struct epoll_event events[64];

    while (1) {
        int n = epoll_wait(epfd, events, 64, 50); // 50ms timeout for stale-connection scan

        uint64_t now_ms = epoll_now_ms();

        for (int i = 0; i < n; i++) {
            int fd = events[i].data.fd;

            if (fd == (int)server_fd) {
                // Accept one connection per epoll event (level-triggered: fires again if more pending)
                struct sockaddr_in client = {0};
                socklen_t len = sizeof(client);
                int cfd = accept(fd, (struct sockaddr*)&client, &len);
                if (cfd >= 0) {
                    int one = 1;
                    setsockopt(cfd, IPPROTO_TCP, TCP_NODELAY, &one, sizeof(one));
                    struct timeval tv = { .tv_sec = 5 }; // zombie guard
                    setsockopt(cfd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
                    struct epoll_event cev;
                    cev.events  = EPOLLIN;
                    cev.data.fd = cfd;
                    epoll_ctl(epfd, EPOLL_CTL_ADD, cfd, &cev);
                    epoll_slot_add(cfd);
                }
            } else {
                // Handle one request on this client connection
                int keep_alive = 0;
                tinox_handle_one(srv, (int64_t)fd, &keep_alive);
                if (keep_alive) {
                    g_epoll_slots[fd % EPOLL_MAX_CONNS].last_ms = now_ms;
                } else {
                    epoll_ctl(epfd, EPOLL_CTL_DEL, fd, NULL);
                    epoll_slot_remove(fd);
                    close(fd);
                }
            }
        }

        // Scan for stale connections (only when we have active clients)
        if (g_epoll_nconns > 0) {
            for (int i = 0; i < EPOLL_MAX_CONNS; i++) {
                if (g_epoll_slots[i].fd < 0) continue;
                if (now_ms - g_epoll_slots[i].last_ms >= EPOLL_KEEP_ALIVE_MS) {
                    int fd = g_epoll_slots[i].fd;
                    epoll_ctl(epfd, EPOLL_CTL_DEL, fd, NULL);
                    epoll_slot_remove(fd);
                    close(fd);
                }
            }
        }
    }
}

struct TinoxWorkerArgs { TinoxHttpServer* srv; int64_t port; };

static void* tinox_worker_run(void* arg) {
    struct TinoxWorkerArgs* wa = (struct TinoxWorkerArgs*)arg;
    // Each worker creates its own SO_REUSEPORT socket for zero-contention accept()
    int64_t server_fd = httpServerCreate(wa->port);
    if (server_fd >= 0) tinox_handle_connections(wa->srv, server_fd);
    return NULL;
}

void tinox_HttpServer_listen(int64_t* server) {
    signal(SIGPIPE, SIG_IGN); // writev/send to closed connection should not kill process
    TinoxHttpServer* srv = (TinoxHttpServer*)server;
    int64_t port = srv->port;
    fprintf(stderr, "HttpServer listening on port %lld\n", (long long)port);

    int ncpus = (int)sysconf(_SC_NPROCESSORS_ONLN);
    int nthreads = ncpus > 0 ? ncpus : 8; // one thread per CPU, each handles multiple conns via epoll

    static struct TinoxWorkerArgs worker_args;
    worker_args.srv  = srv;
    worker_args.port = port;

    for (int i = 1; i < nthreads; i++) {
        pthread_t tid;
        pthread_create(&tid, NULL, tinox_worker_run, &worker_args);
        pthread_detach(tid);
    }

    // Main thread creates its own SO_REUSEPORT socket
    int64_t server_fd = httpServerCreate(port);
    if (server_fd < 0) { fprintf(stderr, "HttpServer: failed to bind\n"); return; }
    tinox_handle_connections(srv, server_fd);
    httpServerClose(server_fd);
}

// ---- JSON ----

#define JSON_NULL      0
#define JSON_BOOL      1
#define JSON_INT       2
#define JSON_FLOAT     3
#define JSON_STRING    4
#define JSON_ARRAY     5
#define JSON_OBJECT    6
#define JSON_INT_ARRAY 7  // fast-path: arr_val points to int64 values, arr_val[-1]=len

// Arena allocator: all JsonValue nodes + string data for one parse live here.
// Reset per jsonParse() call — valid until the next call.
typedef struct { char* buf; size_t used; size_t cap; } JsonArena;
static __thread JsonArena g_json_arena;

static void* json_arena_alloc(size_t size) {
    size = (size + 7) & ~(size_t)7;
    if (g_json_arena.used + size > g_json_arena.cap) {
        size_t nc = g_json_arena.cap ? g_json_arena.cap * 2 : 65536;
        while (nc < g_json_arena.used + size) nc *= 2;
        g_json_arena.buf = (char*)realloc(g_json_arena.buf, nc);
        g_json_arena.cap = nc;
    }
    void* p = g_json_arena.buf + g_json_arena.used;
    g_json_arena.used += size;
    return p;
}

typedef struct TinoxJsonValue {
    int64_t type;
    union {
        int64_t  bool_val;
        int64_t  int_val;
        double   float_val;
        char*    str_val;
        int64_t* arr_val;  // tinox-style array (len at [-1])
        void*    obj_val;  // TinoxMap*
    };
} TinoxJsonValue;

static TinoxJsonValue* json_alloc(int64_t type) {
    TinoxJsonValue* v = (TinoxJsonValue*)json_arena_alloc(sizeof(TinoxJsonValue));
    v->type = type;
    return v;
}

#define json_skip_ws(p) ({ const char* _p = (p); while (*_p == ' ' || *_p == '\t' || *_p == '\r' || *_p == '\n') _p++; _p; })

// JSON-object map: all memory from arena, keys not strdup'd (no leaks on arena reset)
static void* json_obj_map_create(void) {
    TinoxMap* m = (TinoxMap*)json_arena_alloc(sizeof(TinoxMap));
    m->cap = 4;
    m->len = 0;
    m->entries = (TinoxMapEntry*)json_arena_alloc(4 * sizeof(TinoxMapEntry));
    memset(m->entries, 0, 4 * sizeof(TinoxMapEntry));
    m->borrowed_keys = 1;
    return m;
}

static void json_obj_map_set(void* map, const char* key, int64_t value) {
    TinoxMap* m = (TinoxMap*)map;
    if (m->len * TINOX_MAP_LOAD_DEN >= m->cap * TINOX_MAP_LOAD_NUM) {
        size_t new_cap = m->cap * 2;
        TinoxMapEntry* ne = (TinoxMapEntry*)json_arena_alloc(new_cap * sizeof(TinoxMapEntry));
        memset(ne, 0, new_cap * sizeof(TinoxMapEntry));
        for (size_t i = 0; i < m->cap; i++) {
            char* k = m->entries[i].key;
            if (!k || k == (char*)1) continue;
            size_t idx = map_hash(k, new_cap);
            while (ne[idx].key) idx = (idx + 1) & (new_cap - 1);
            ne[idx] = m->entries[i];
        }
        m->entries = ne;
        m->cap = new_cap;
    }
    size_t idx = map_hash(key, m->cap);
    while (1) {
        char* k = m->entries[idx].key;
        if (!k || k == (char*)1) {
            m->entries[idx].key   = (char*)key; // arena key, no strdup
            m->entries[idx].value = value;
            m->len++;
            return;
        }
        if (strcmp(k, key) == 0) { m->entries[idx].value = value; return; }
        idx = (idx + 1) & (m->cap - 1);
    }
}


static TinoxJsonValue* json_parse_value(const char** p);

static char* json_parse_string_raw(const char** p) {
    (*p)++; // skip '"'
    // Pre-scan to get exact length — avoids malloc+realloc
    const char* scan = *p;
    size_t max_len = 0;
    while (*scan && *scan != '"') {
        if (*scan == '\\') scan++;
        scan++;
        max_len++;
    }
    char* buf = (char*)json_arena_alloc(max_len + 1);
    size_t len = 0;
    while (**p && **p != '"') {
        if (**p == '\\') {
            (*p)++;
            char esc = **p;
            if      (esc == 'n')  buf[len++] = '\n';
            else if (esc == 't')  buf[len++] = '\t';
            else if (esc == 'r')  buf[len++] = '\r';
            else if (esc == '"')  buf[len++] = '"';
            else if (esc == '\\') buf[len++] = '\\';
            else if (esc == '/')  buf[len++] = '/';
            else                  buf[len++] = esc;
        } else {
            buf[len++] = **p;
        }
        (*p)++;
    }
    if (**p == '"') (*p)++;
    buf[len] = '\0';
    return buf;
}

static TinoxJsonValue* json_parse_value(const char** p) {
    *p = json_skip_ws(*p);
    if (!**p) return json_alloc(JSON_NULL);

    if (**p == '"') {
        TinoxJsonValue* v = json_alloc(JSON_STRING);
        v->str_val = json_parse_string_raw(p);
        return v;
    }
    if (**p == '{') {
        TinoxJsonValue* v = json_alloc(JSON_OBJECT);
        v->obj_val = json_obj_map_create(); // arena-allocated, no strdup for keys
        (*p)++; // skip '{'
        *p = json_skip_ws(*p);
        while (**p && **p != '}') {
            *p = json_skip_ws(*p);
            if (**p != '"') break;
            char* key = json_parse_string_raw(p);
            *p = json_skip_ws(*p);
            if (**p == ':') (*p)++;
            TinoxJsonValue* val = json_parse_value(p);
            json_obj_map_set(v->obj_val, key, (int64_t)(uintptr_t)val);
            *p = json_skip_ws(*p);
            if (**p == ',') (*p)++;
        }
        if (**p == '}') (*p)++;
        return v;
    }
    if (**p == '[') {
        (*p)++; // skip '['
        *p = json_skip_ws(*p);
        // Single-pass fast-path: parse integers directly with malloc+doubling.
        // If a non-integer is found, free and fall through to generic parser.
        if (**p == ']') {
            (*p)++;
            TinoxJsonValue* v = json_alloc(JSON_INT_ARRAY);
            int64_t* raw = (int64_t*)json_arena_alloc(sizeof(int64_t));
            raw[0] = 0;
            v->arr_val = raw + 1;
            return v;
        }
        const char* saved_p = *p;
        // Stack buffer covers typical arrays without any malloc
        int64_t stack_buf[256];
        size_t fast_cap = 256, fast_len = 0;
        int64_t* fast_arr = stack_buf;
        int64_t* heap_buf = NULL;
        int is_int_array = 1;
        while (**p && **p != ']') {
            *p = json_skip_ws(*p);
            int neg = (**p == '-');
            if (neg) (*p)++;
            if (**p < '0' || **p > '9') { is_int_array = 0; break; }
            int64_t val = 0;
            while (**p >= '0' && **p <= '9') val = val * 10 + (*(*p)++ - '0');
            *p = json_skip_ws(*p);
            if (**p != ',' && **p != ']') { is_int_array = 0; break; }
            if (fast_len >= fast_cap) {
                fast_cap *= 2;
                if (!heap_buf) {
                    heap_buf = (int64_t*)malloc(fast_cap * sizeof(int64_t));
                    memcpy(heap_buf, stack_buf, fast_len * sizeof(int64_t));
                } else {
                    heap_buf = (int64_t*)realloc(heap_buf, fast_cap * sizeof(int64_t));
                }
                fast_arr = heap_buf;
            }
            fast_arr[fast_len++] = neg ? -val : val;
            if (**p == ',') (*p)++;
        }
        if (is_int_array && **p == ']') {
            (*p)++;
            TinoxJsonValue* v = json_alloc(JSON_INT_ARRAY);
            int64_t* raw = (int64_t*)json_arena_alloc((fast_len + 1) * sizeof(int64_t));
            raw[0] = (int64_t)fast_len;
            memcpy(raw + 1, fast_arr, fast_len * sizeof(int64_t));
            if (heap_buf) free(heap_buf);
            v->arr_val = raw + 1;
            return v;
        }
        if (heap_buf) free(heap_buf);
        *p = saved_p; // restore position for generic fallback
        // Generic fallback for mixed-type arrays
        TinoxJsonValue* v = json_alloc(JSON_ARRAY);
        size_t cap = 8, len = 0;
        int64_t* raw = (int64_t*)malloc((cap + 1) * sizeof(int64_t));
        int64_t* arr = raw + 1;
        while (**p && **p != ']') {
            TinoxJsonValue* elem = json_parse_value(p);
            if (len >= cap) {
                cap *= 2;
                raw = (int64_t*)realloc(raw, (cap + 1) * sizeof(int64_t));
                arr = raw + 1;
            }
            arr[len++] = (int64_t)(uintptr_t)elem;
            *p = json_skip_ws(*p);
            if (**p == ',') (*p)++;
            *p = json_skip_ws(*p);
        }
        if (**p == ']') (*p)++;
        raw[0] = (int64_t)len;
        v->arr_val = arr;
        return v;
    }
    if (strncmp(*p, "true", 4) == 0) {
        TinoxJsonValue* v = json_alloc(JSON_BOOL);
        v->bool_val = 1; *p += 4; return v;
    }
    if (strncmp(*p, "false", 5) == 0) {
        TinoxJsonValue* v = json_alloc(JSON_BOOL);
        v->bool_val = 0; *p += 5; return v;
    }
    if (strncmp(*p, "null", 4) == 0) { *p += 4; return json_alloc(JSON_NULL); }
    // Number
    const char* start = *p;
    int is_float = 0;
    if (**p == '-') (*p)++;
    while (**p >= '0' && **p <= '9') (*p)++;
    if (**p == '.') { is_float = 1; (*p)++; while (**p >= '0' && **p <= '9') (*p)++; }
    if (**p == 'e' || **p == 'E') { is_float = 1; (*p)++; if (**p == '+' || **p == '-') (*p)++; while (**p >= '0' && **p <= '9') (*p)++; }
    if (is_float) {
        TinoxJsonValue* v = json_alloc(JSON_FLOAT);
        v->float_val = atof(start);
        return v;
    } else {
        TinoxJsonValue* v = json_alloc(JSON_INT);
        // Inline fast int parse — avoids strtol machinery of atoll
        int64_t val = 0;
        const char* s = start;
        int neg = (*s == '-');
        if (neg) s++;
        while (*s >= '0' && *s <= '9') val = val * 10 + (*s++ - '0');
        v->int_val = neg ? -val : val;
        return v;
    }
}

int64_t* jsonParse(const char* text) {
    if (!text) return (int64_t*)json_alloc(JSON_NULL);
    g_json_arena.used = 0;  // reset arena — previous parse tree is invalidated
    const char* p = text;
    return (int64_t*)json_parse_value(&p);
}

static size_t fast_i64_write(int64_t val, char* buf);
static void json_stringify_value(TinoxJsonValue* v, char** out, size_t* len, size_t* cap);

static void json_append(char** out, size_t* len, size_t* cap, const char* s, size_t slen) {
    while (*len + slen + 1 >= *cap) { *cap *= 2; *out = (char*)realloc(*out, *cap); }
    memcpy(*out + *len, s, slen);
    *len += slen;
    (*out)[*len] = '\0';
}

static void json_append_str(char** out, size_t* len, size_t* cap, const char* s) {
    // Escape and append a string value (with surrounding quotes)
    json_append(out, len, cap, "\"", 1);
    for (const char* p = s; *p; p++) {
        if      (*p == '"')  json_append(out, len, cap, "\\\"", 2);
        else if (*p == '\\') json_append(out, len, cap, "\\\\", 2);
        else if (*p == '\n') json_append(out, len, cap, "\\n",  2);
        else if (*p == '\r') json_append(out, len, cap, "\\r",  2);
        else if (*p == '\t') json_append(out, len, cap, "\\t",  2);
        else                 json_append(out, len, cap, p,       1);
    }
    json_append(out, len, cap, "\"", 1);
}

static void json_stringify_value(TinoxJsonValue* v, char** out, size_t* len, size_t* cap) {
    if (!v) { json_append(out, len, cap, "null", 4); return; }
    char buf[64];
    int n;
    switch (v->type) {
        case JSON_NULL:   json_append(out, len, cap, "null",  4); break;
        case JSON_BOOL:   json_append(out, len, cap, v->bool_val ? "true" : "false", v->bool_val ? 4 : 5); break;
        case JSON_INT:    n = snprintf(buf, sizeof(buf), "%lld", (long long)v->int_val); json_append(out, len, cap, buf, n); break;
        case JSON_FLOAT:  n = snprintf(buf, sizeof(buf), "%g",   v->float_val); json_append(out, len, cap, buf, n); break;
        case JSON_STRING: json_append_str(out, len, cap, v->str_val ? v->str_val : ""); break;
        case JSON_INT_ARRAY: {
            json_append(out, len, cap, "[", 1);
            if (v->arr_val) {
                int64_t alen = v->arr_val[-1];
                char nbuf[24];
                for (int64_t i = 0; i < alen; i++) {
                    if (i > 0) json_append(out, len, cap, ",", 1);
                    size_t nlen = fast_i64_write(v->arr_val[i], nbuf);
                    json_append(out, len, cap, nbuf, nlen);
                }
            }
            json_append(out, len, cap, "]", 1);
            break;
        }
        case JSON_ARRAY: {
            json_append(out, len, cap, "[", 1);
            if (v->arr_val) {
                int64_t alen = v->arr_val[-1];
                for (int64_t i = 0; i < alen; i++) {
                    if (i > 0) json_append(out, len, cap, ",", 1);
                    json_stringify_value((TinoxJsonValue*)(uintptr_t)v->arr_val[i], out, len, cap);
                }
            }
            json_append(out, len, cap, "]", 1);
            break;
        }
        case JSON_OBJECT: {
            json_append(out, len, cap, "{", 1);
            if (v->obj_val) {
                int64_t* keys = tinox_map_keys(v->obj_val);
                int64_t klen = keys ? keys[-1] : 0;
                for (int64_t i = 0; i < klen; i++) {
                    if (i > 0) json_append(out, len, cap, ",", 1);
                    const char* k = (const char*)(uintptr_t)keys[i];
                    json_append_str(out, len, cap, k);
                    json_append(out, len, cap, ":", 1);
                    int64_t vptr = tinox_map_get(v->obj_val, k);
                    json_stringify_value((TinoxJsonValue*)(uintptr_t)vptr, out, len, cap);
                }
            }
            json_append(out, len, cap, "}", 1);
            break;
        }
        default: json_append(out, len, cap, "null", 4); break;
    }
}

char* jsonStringify(int64_t* value) {
    size_t cap = 256, len = 0;
    char* out = (char*)malloc(cap);
    out[0] = '\0';
    json_stringify_value((TinoxJsonValue*)value, &out, &len, &cap);
    return out;
}

int64_t jsonGetInt(int64_t* value) {
    TinoxJsonValue* v = (TinoxJsonValue*)value;
    if (!v) return 0;
    if (v->type == JSON_FLOAT) return (int64_t)v->float_val;
    return v->int_val;
}

double jsonGetFloat(int64_t* value) {
    TinoxJsonValue* v = (TinoxJsonValue*)value;
    if (!v) return 0.0;
    if (v->type == JSON_INT) return (double)v->int_val;
    return v->float_val;
}

char* jsonGetString(int64_t* value) {
    TinoxJsonValue* v = (TinoxJsonValue*)value;
    if (!v || v->type != JSON_STRING || !v->str_val) return "";
    return v->str_val;
}

int64_t jsonGetBool(int64_t* value) {
    TinoxJsonValue* v = (TinoxJsonValue*)value;
    if (!v) return 0;
    return v->bool_val;
}

void* jsonGetObject(int64_t* value) {
    TinoxJsonValue* v = (TinoxJsonValue*)value;
    if (!v || v->type != JSON_OBJECT) return tinox_map_create();
    return v->obj_val;
}

int64_t* jsonGetArray(int64_t* value) {
    TinoxJsonValue* v = (TinoxJsonValue*)value;
    if (!v || v->type != JSON_ARRAY || !v->arr_val) {
        int64_t* empty = (int64_t*)malloc(sizeof(int64_t));
        empty[0] = 0; return empty + 1;
    }
    return v->arr_val;
}

int64_t jsonIsNull(int64_t* value)   { return (!value || ((TinoxJsonValue*)value)->type == JSON_NULL)  ? 1 : 0; }
int64_t jsonIsString(int64_t* value) { return (value && ((TinoxJsonValue*)value)->type == JSON_STRING) ? 1 : 0; }
int64_t jsonIsInt(int64_t* value)    { return (value && ((TinoxJsonValue*)value)->type == JSON_INT)    ? 1 : 0; }
int64_t jsonIsFloat(int64_t* value)  { return (value && ((TinoxJsonValue*)value)->type == JSON_FLOAT)  ? 1 : 0; }
int64_t jsonIsBool(int64_t* value)   { return (value && ((TinoxJsonValue*)value)->type == JSON_BOOL)   ? 1 : 0; }
int64_t jsonIsObject(int64_t* value) { return (value && ((TinoxJsonValue*)value)->type == JSON_OBJECT) ? 1 : 0; }
int64_t jsonIsArray(int64_t* value)  { return (value && ((TinoxJsonValue*)value)->type == JSON_ARRAY)  ? 1 : 0; }

int64_t* jsonGetField(int64_t* obj, const char* key) {
    TinoxJsonValue* v = (TinoxJsonValue*)obj;
    if (!v || v->type != JSON_OBJECT || !v->obj_val) return NULL;
    int64_t vptr = tinox_map_get(v->obj_val, key);
    return (int64_t*)(uintptr_t)vptr;
}

static __thread int64_t* g_jia_buf = NULL;
static __thread int64_t  g_jia_cap = 0;

static int64_t* jia_ensure(int64_t need) {
    if (need + 1 > g_jia_cap) {
        int64_t nc = g_jia_cap ? g_jia_cap * 2 : 256;
        while (nc < need + 1) nc *= 2;
        g_jia_buf = (int64_t*)realloc(g_jia_buf, (size_t)nc * sizeof(int64_t));
        g_jia_cap = nc;
    }
    return g_jia_buf;
}

int64_t* jsonIntArrayFromJson(int64_t* json_array) {
    TinoxJsonValue* v = (TinoxJsonValue*)json_array;
    if (!v) { int64_t* b = jia_ensure(0); b[0] = 0; return b + 1; }
    // Fast-path: pure int array — just alias the arena data directly
    if (v->type == JSON_INT_ARRAY) {
        int64_t len = v->arr_val ? v->arr_val[-1] : 0;
        int64_t* buf = jia_ensure(len);
        buf[0] = len;
        if (len > 0) memcpy(buf + 1, v->arr_val, (size_t)len * sizeof(int64_t));
        return buf + 1;
    }
    // Generic JSON_ARRAY path
    int64_t len = (v->type == JSON_ARRAY && v->arr_val) ? v->arr_val[-1] : 0;
    int64_t* buf = jia_ensure(len);
    buf[0] = len;
    for (int64_t i = 0; i < len; i++) {
        TinoxJsonValue* elem = (TinoxJsonValue*)(uintptr_t)v->arr_val[i];
        if (elem) {
            if      (elem->type == JSON_INT)   buf[i + 1] = elem->int_val;
            else if (elem->type == JSON_FLOAT) buf[i + 1] = (int64_t)elem->float_val;
            else                               buf[i + 1] = 0;
        } else {
            buf[i + 1] = 0;
        }
    }
    return buf + 1;
}

static const char g_digit_pairs[201] =
    "00010203040506070809"
    "10111213141516171819"
    "20212223242526272829"
    "30313233343536373839"
    "40414243444546474849"
    "50515253545556575859"
    "60616263646566676869"
    "70717273747576777879"
    "80818283848586878889"
    "90919293949596979899";

__attribute__((noinline)) static size_t fast_i64_write(int64_t val, char* buf) {
    if ((uint64_t)val < 10) { buf[0] = '0' + (char)val; return 1; }
    if ((uint64_t)val < 100) {
        int d = (int)val * 2;
        buf[0] = g_digit_pairs[d]; buf[1] = g_digit_pairs[d + 1];
        return 2;
    }
    char tmp[21];
    int neg = val < 0;
    uint64_t uval = neg ? -(uint64_t)val : (uint64_t)val;
    int n = 0;
    while (uval >= 100) {
        int d = (int)(uval % 100) * 2;
        tmp[n++] = g_digit_pairs[d + 1];
        tmp[n++] = g_digit_pairs[d];
        uval /= 100;
    }
    if (uval >= 10) {
        int d = (int)uval * 2;
        tmp[n++] = g_digit_pairs[d + 1];
        tmp[n++] = g_digit_pairs[d];
    } else {
        tmp[n++] = '0' + (int)uval;
    }
    if (neg) tmp[n++] = '-';
    for (int i = 0; i < n; i++) buf[i] = tmp[n - 1 - i];
    return (size_t)n;
}

static __thread char*  g_wrap_buf = NULL;
static __thread size_t g_wrap_cap = 0;

// Builds {"key":[val,...]} into a thread-local buffer — zero malloc per call
char* jsonIntArrayWrap(const char* key, int64_t* arr) {
    int64_t len = arr ? arr[-1] : 0;
    size_t klen = strlen(key);
    size_t need = 5 + klen + (size_t)len * 22 + 3;
    if (need > g_wrap_cap) {
        size_t nc = g_wrap_cap ? g_wrap_cap * 2 : 4096;
        while (nc < need) nc *= 2;
        g_wrap_buf = (char*)realloc(g_wrap_buf, nc);
        g_wrap_cap = nc;
    }
    char* out = g_wrap_buf;
    size_t pos = 0;
    out[pos++] = '{';
    out[pos++] = '"';
    memcpy(out + pos, key, klen); pos += klen;
    out[pos++] = '"';
    out[pos++] = ':';
    out[pos++] = '[';
    if (arr) {
        for (int64_t i = 0; i < len; i++) {
            if (i > 0) out[pos++] = ',';
            pos += fast_i64_write(arr[i], out + pos);
        }
    }
    out[pos++] = ']';
    out[pos++] = '}';
    out[pos] = '\0';
    return out;
}

char* jsonIntArrayToString(int64_t* arr) {
    if (!arr) return strdup("[]");
    int64_t len = arr[-1];
    size_t cap = (size_t)(len * 21 + 4);
    if (cap < 4) cap = 4;
    char* out = (char*)malloc(cap);
    size_t pos = 0;
    out[pos++] = '[';
    for (int64_t i = 0; i < len; i++) {
        if (i > 0) out[pos++] = ',';
        pos += fast_i64_write(arr[i], out + pos);
    }
    out[pos++] = ']';
    out[pos] = '\0';
    return out;
}

// ---- JsonBuilder — fast @JsonSerializable serialization ----

typedef struct {
    char*  buf;
    size_t len;
    size_t cap;
    int    first;
} JsonBuilder;

static void jb_grow(JsonBuilder* b, size_t need) {
    if (b->len + need <= b->cap) return;
    while (b->cap < b->len + need) b->cap *= 2;
    b->buf = (char*)realloc(b->buf, b->cap);
}

static void jb_key(JsonBuilder* b, const char* key) {
    size_t kl = strlen(key);
    jb_grow(b, kl + 4); // comma + quote + key + quote + colon
    if (!b->first) b->buf[b->len++] = ',';
    b->first = 0;
    b->buf[b->len++] = '"';
    memcpy(b->buf + b->len, key, kl); b->len += kl;
    b->buf[b->len++] = '"';
    b->buf[b->len++] = ':';
}

char* jsonBuilderCreate(void) {
    JsonBuilder* b = (JsonBuilder*)malloc(sizeof(JsonBuilder));
    b->cap = 256;
    b->buf = (char*)malloc(b->cap);
    b->len = 0;
    b->first = 1;
    b->buf[b->len++] = '{';
    return (char*)b;
}

void jsonBuilderAddInt(char* handle, const char* key, int64_t val) {
    JsonBuilder* b = (JsonBuilder*)handle;
    jb_key(b, key);
    jb_grow(b, 21);
    b->len += fast_i64_write(val, b->buf + b->len);
}

void jsonBuilderAddFloat(char* handle, const char* key, double val) {
    JsonBuilder* b = (JsonBuilder*)handle;
    jb_key(b, key);
    char tmp[32];
    int n = snprintf(tmp, sizeof(tmp), "%g", val);
    jb_grow(b, (size_t)n);
    memcpy(b->buf + b->len, tmp, (size_t)n); b->len += (size_t)n;
}

void jsonBuilderAddBool(char* handle, const char* key, int val) {
    JsonBuilder* b = (JsonBuilder*)handle;
    jb_key(b, key);
    if (val) { jb_grow(b, 4); memcpy(b->buf + b->len, "true",  4); b->len += 4; }
    else      { jb_grow(b, 5); memcpy(b->buf + b->len, "false", 5); b->len += 5; }
}

void jsonBuilderAddString(char* handle, const char* key, const char* val) {
    JsonBuilder* b = (JsonBuilder*)handle;
    jb_key(b, key);
    size_t vl = val ? strlen(val) : 0;
    jb_grow(b, vl * 2 + 2); // worst-case: every char escaped
    b->buf[b->len++] = '"';
    if (val) {
        for (size_t i = 0; i < vl; i++) {
            unsigned char c = (unsigned char)val[i];
            if      (c == '"')  { b->buf[b->len++] = '\\'; b->buf[b->len++] = '"'; }
            else if (c == '\\') { b->buf[b->len++] = '\\'; b->buf[b->len++] = '\\'; }
            else if (c == '\n') { b->buf[b->len++] = '\\'; b->buf[b->len++] = 'n'; }
            else if (c == '\r') { b->buf[b->len++] = '\\'; b->buf[b->len++] = 'r'; }
            else if (c == '\t') { b->buf[b->len++] = '\\'; b->buf[b->len++] = 't'; }
            else                { b->buf[b->len++] = (char)c; }
        }
    }
    b->buf[b->len++] = '"';
}

void jsonBuilderAddIntList(char* handle, const char* key, int64_t* arr) {
    JsonBuilder* b = (JsonBuilder*)handle;
    jb_key(b, key);
    int64_t len = arr ? arr[-1] : 0;
    jb_grow(b, (size_t)(len * 21 + 4));
    b->buf[b->len++] = '[';
    for (int64_t i = 0; i < len; i++) {
        if (i > 0) b->buf[b->len++] = ',';
        b->len += fast_i64_write(arr[i], b->buf + b->len);
    }
    b->buf[b->len++] = ']';
}

char* jsonBuilderFinish(char* handle) {
    JsonBuilder* b = (JsonBuilder*)handle;
    jb_grow(b, 2);
    b->buf[b->len++] = '}';
    b->buf[b->len] = '\0';
    char* result = b->buf;
    free(b); // free the builder header only; result owns the buffer
    return result;
}

// ---- fromJson field helpers — avoid two runtime calls per field ----

int64_t jsonGetIntField(int64_t* obj, const char* key) {
    return jsonGetInt(jsonGetField(obj, key));
}

double jsonGetFloatField(int64_t* obj, const char* key) {
    return jsonGetFloat(jsonGetField(obj, key));
}

int jsonGetBoolField(int64_t* obj, const char* key) {
    return (int)jsonGetBool(jsonGetField(obj, key));
}

char* jsonGetStringField(int64_t* obj, const char* key) {
    return jsonGetString(jsonGetField(obj, key));
}

int64_t* jsonGetIntListField(int64_t* obj, const char* key) {
    return jsonIntArrayFromJson(jsonGetField(obj, key));
}

// ---- Config (@Config annotation) ----
// Reads key=value pairs from application.properties in the current directory.

#define TINOX_CONFIG_MAX_ENTRIES 256
#define TINOX_CONFIG_MAX_LINE    1024

typedef struct { char* key; char* value; } TinoxConfigEntry;

static TinoxConfigEntry tinox_config_entries[TINOX_CONFIG_MAX_ENTRIES];
static int              tinox_config_count = -1; // -1 = not loaded

static void tinox_config_load(void) {
    tinox_config_count = 0;
    FILE* f = fopen("application.properties", "r");
    if (!f) return;
    char line[TINOX_CONFIG_MAX_LINE];
    while (fgets(line, sizeof(line), f)) {
        // strip newline
        size_t len = strlen(line);
        while (len > 0 && (line[len-1] == '\n' || line[len-1] == '\r')) line[--len] = '\0';
        // skip empty lines and comments
        if (len == 0 || line[0] == '#' || line[0] == '!') continue;
        char* eq = strchr(line, '=');
        if (!eq) continue;
        *eq = '\0';
        char* key = line;
        char* val = eq + 1;
        // trim trailing whitespace from key
        char* kend = eq - 1;
        while (kend >= key && (*kend == ' ' || *kend == '\t')) *kend-- = '\0';
        // trim leading whitespace from value
        while (*val == ' ' || *val == '\t') val++;
        if (tinox_config_count < TINOX_CONFIG_MAX_ENTRIES) {
            tinox_config_entries[tinox_config_count].key   = strdup(key);
            tinox_config_entries[tinox_config_count].value = strdup(val);
            tinox_config_count++;
        }
    }
    fclose(f);
}

static const char* tinox_config_lookup(const char* key) {
    if (tinox_config_count < 0) tinox_config_load();
    for (int i = 0; i < tinox_config_count; i++) {
        if (strcmp(tinox_config_entries[i].key, key) == 0)
            return tinox_config_entries[i].value;
    }
    return "";
}

char* tinox_config_get(const char* key) {
    return (char*)tinox_config_lookup(key);
}

int64_t tinox_config_get_int(const char* key) {
    const char* v = tinox_config_lookup(key);
    if (!v || *v == '\0') return 0;
    return (int64_t)atoll(v);
}

int64_t tinox_config_get_bool(const char* key) {
    const char* v = tinox_config_lookup(key);
    if (!v || *v == '\0') return 0;
    return (strcmp(v, "true") == 0 || strcmp(v, "1") == 0 || strcmp(v, "yes") == 0) ? 1 : 0;
}

// ---- CLI argument parsing (@Command / @Option / @Argument) ----

static int    _tinox_argc = 0;
static char** _tinox_argv = NULL;

// Scans argv for --long-name or -s and returns the following value, or NULL.
char* tinox_cli_get_string(const char* long_name, const char* short_name) {
    for (int i = 1; i < _tinox_argc - 1; i++) {
        if ((long_name  && strcmp(_tinox_argv[i], long_name)  == 0) ||
            (short_name && *short_name && strcmp(_tinox_argv[i], short_name) == 0)) {
            return _tinox_argv[i + 1];
        }
    }
    return NULL;
}

// Returns 1 if the flag is present, 0 otherwise.
int64_t tinox_cli_has_flag(const char* long_name, const char* short_name) {
    for (int i = 1; i < _tinox_argc; i++) {
        if ((long_name  && strcmp(_tinox_argv[i], long_name)  == 0) ||
            (short_name && *short_name && strcmp(_tinox_argv[i], short_name) == 0))
            return 1;
    }
    return 0;
}

// Returns integer value after --long-name/-s, or default_val if absent.
int64_t tinox_cli_get_int(const char* long_name, const char* short_name, int64_t default_val) {
    char* s = tinox_cli_get_string(long_name, short_name);
    if (!s) return default_val;
    return (int64_t)atoll(s);
}

// Returns the positional argument at position `index` (0-based, skipping option tokens).
char* tinox_cli_get_positional(int32_t index) {
    int pos = 0;
    int i = 1;
    while (i < _tinox_argc) {
        char* arg = _tinox_argv[i];
        if (arg[0] == '-') {
            // skip option token; if next token is not a flag treat it as the value
            if (i + 1 < _tinox_argc && _tinox_argv[i + 1][0] != '-')
                i += 2;
            else
                i += 1;
        } else {
            if (pos == index) return arg;
            pos++;
            i++;
        }
    }
    return NULL;
}

// Prints a single help line "  -s, --long-name   description"
void tinox_cli_print_option(const char* names, const char* description) {
    printf("  %-22s  %s\n", names, description ? description : "");
}

// ---- Metrics ----

#define TINOX_MAX_METRICS 512

typedef struct {
    char   name[256];
    int64_t value;
} TinoxCounter;

typedef struct {
    char    name[256];
    int64_t count;
    int64_t sum_ns;
    int64_t min_ns;
    int64_t max_ns;
} TinoxHistogram;

typedef struct {
    char    name[256];
    int64_t value;
} TinoxGauge;

static TinoxCounter   _tinox_counters[TINOX_MAX_METRICS];
static TinoxHistogram _tinox_histograms[TINOX_MAX_METRICS];
static TinoxGauge     _tinox_gauges[TINOX_MAX_METRICS];
static int _tinox_counter_n   = 0;
static int _tinox_histogram_n = 0;
static int _tinox_gauge_n     = 0;
static pthread_mutex_t _tinox_metrics_mu = PTHREAD_MUTEX_INITIALIZER;

int64_t tinox_clock_nanos(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)ts.tv_sec * 1000000000LL + ts.tv_nsec;
}

void tinox_counter_inc(const char* name) {
    pthread_mutex_lock(&_tinox_metrics_mu);
    for (int i = 0; i < _tinox_counter_n; i++) {
        if (strcmp(_tinox_counters[i].name, name) == 0) {
            _tinox_counters[i].value++;
            pthread_mutex_unlock(&_tinox_metrics_mu);
            return;
        }
    }
    if (_tinox_counter_n < TINOX_MAX_METRICS) {
        strncpy(_tinox_counters[_tinox_counter_n].name, name, 255);
        _tinox_counters[_tinox_counter_n].name[255] = '\0';
        _tinox_counters[_tinox_counter_n].value = 1;
        _tinox_counter_n++;
    }
    pthread_mutex_unlock(&_tinox_metrics_mu);
}

void tinox_histogram_record(const char* name, int64_t duration_ns) {
    pthread_mutex_lock(&_tinox_metrics_mu);
    for (int i = 0; i < _tinox_histogram_n; i++) {
        if (strcmp(_tinox_histograms[i].name, name) == 0) {
            _tinox_histograms[i].count++;
            _tinox_histograms[i].sum_ns += duration_ns;
            if (duration_ns < _tinox_histograms[i].min_ns) _tinox_histograms[i].min_ns = duration_ns;
            if (duration_ns > _tinox_histograms[i].max_ns) _tinox_histograms[i].max_ns = duration_ns;
            pthread_mutex_unlock(&_tinox_metrics_mu);
            return;
        }
    }
    if (_tinox_histogram_n < TINOX_MAX_METRICS) {
        strncpy(_tinox_histograms[_tinox_histogram_n].name, name, 255);
        _tinox_histograms[_tinox_histogram_n].name[255] = '\0';
        _tinox_histograms[_tinox_histogram_n].count   = 1;
        _tinox_histograms[_tinox_histogram_n].sum_ns  = duration_ns;
        _tinox_histograms[_tinox_histogram_n].min_ns  = duration_ns;
        _tinox_histograms[_tinox_histogram_n].max_ns  = duration_ns;
        _tinox_histogram_n++;
    }
    pthread_mutex_unlock(&_tinox_metrics_mu);
}

void tinox_gauge_set(const char* name, int64_t value) {
    pthread_mutex_lock(&_tinox_metrics_mu);
    for (int i = 0; i < _tinox_gauge_n; i++) {
        if (strcmp(_tinox_gauges[i].name, name) == 0) {
            _tinox_gauges[i].value = value;
            pthread_mutex_unlock(&_tinox_metrics_mu);
            return;
        }
    }
    if (_tinox_gauge_n < TINOX_MAX_METRICS) {
        strncpy(_tinox_gauges[_tinox_gauge_n].name, name, 255);
        _tinox_gauges[_tinox_gauge_n].name[255] = '\0';
        _tinox_gauges[_tinox_gauge_n].value = value;
        _tinox_gauge_n++;
    }
    pthread_mutex_unlock(&_tinox_metrics_mu);
}

// Returns a heap-allocated Prometheus-format string; caller need not free (GC-managed).
char* tinox_metrics_prometheus(void) {
    pthread_mutex_lock(&_tinox_metrics_mu);
    // Rough upper bound: 256 bytes per metric entry
    size_t cap = (size_t)(_tinox_counter_n + _tinox_histogram_n + _tinox_gauge_n + 1) * 512 + 64;
    char* buf = (char*)GC_malloc(cap);
    size_t pos = 0;

    for (int i = 0; i < _tinox_counter_n; i++) {
        pos += (size_t)snprintf(buf + pos, cap - pos,
            "# TYPE %s_total counter\n%s_total %lld\n",
            _tinox_counters[i].name, _tinox_counters[i].name,
            (long long)_tinox_counters[i].value);
    }
    for (int i = 0; i < _tinox_histogram_n; i++) {
        double sum_s   = (double)_tinox_histograms[i].sum_ns / 1e9;
        double min_s   = (double)_tinox_histograms[i].min_ns / 1e9;
        double max_s   = (double)_tinox_histograms[i].max_ns / 1e9;
        int64_t count  = _tinox_histograms[i].count;
        const char* n  = _tinox_histograms[i].name;
        pos += (size_t)snprintf(buf + pos, cap - pos,
            "# TYPE %s_duration_seconds summary\n"
            "%s_duration_seconds_count %lld\n"
            "%s_duration_seconds_sum %.9f\n"
            "%s_duration_seconds_min %.9f\n"
            "%s_duration_seconds_max %.9f\n",
            n, n, (long long)count, n, sum_s, n, min_s, n, max_s);
    }
    for (int i = 0; i < _tinox_gauge_n; i++) {
        pos += (size_t)snprintf(buf + pos, cap - pos,
            "# TYPE %s gauge\n%s %lld\n",
            _tinox_gauges[i].name, _tinox_gauges[i].name,
            (long long)_tinox_gauges[i].value);
    }
    pthread_mutex_unlock(&_tinox_metrics_mu);
    return buf;
}

// ---- Database / ORM runtime ----
// Compiled only when libpq is available (postgres driver).
// SQLite and MySQL variants follow the same interface.

#ifdef TINOX_DB_POSTGRES
#include <libpq-fe.h>

static PGconn* _tinox_db_conn = NULL;
static pthread_mutex_t _tinox_db_mu = PTHREAD_MUTEX_INITIALIZER;

void tinox_db_connect(const char* url) {
    _tinox_db_conn = PQconnectdb(url);
    if (PQstatus(_tinox_db_conn) != CONNECTION_OK) {
        fprintf(stderr, "DB connection failed: %s\n", PQerrorMessage(_tinox_db_conn));
        PQfinish(_tinox_db_conn);
        exit(1);
    }
}

void* tinox_db_get_conn(void) {
    return _tinox_db_conn;
}

void* tinox_db_exec(void* conn, const char* sql, const char** params, int64_t n_params) {
    PGresult* res = PQexecParams(
        (PGconn*)conn, sql,
        (int)n_params, NULL,
        params, NULL, NULL, 0
    );
    ExecStatusType status = PQresultStatus(res);
    if (status != PGRES_TUPLES_OK && status != PGRES_COMMAND_OK) {
        fprintf(stderr, "Query error: %s\nSQL: %s\n", PQresultErrorMessage(res), sql);
    }
    return (void*)res;
}

int64_t tinox_db_nrows(void* result) { return (int64_t)PQntuples((PGresult*)result); }
int64_t tinox_db_ncols(void* result) { return (int64_t)PQnfields((PGresult*)result); }

char* tinox_db_getval(void* result, int64_t row, int64_t col) {
    return GC_strdup(PQgetvalue((PGresult*)result, (int)row, (int)col));
}

int64_t tinox_db_getval_int(void* result, int64_t row, int64_t col) {
    char* v = PQgetvalue((PGresult*)result, (int)row, (int)col);
    return v ? (int64_t)atoll(v) : 0LL;
}

bool tinox_db_is_null(void* result, int64_t row, int64_t col) {
    return (bool)PQgetisnull((PGresult*)result, (int)row, (int)col);
}

void tinox_db_free(void* result) { PQclear((PGresult*)result); }

char* tinox_db_error(void* conn) {
    return GC_strdup(PQerrorMessage((PGconn*)conn));
}

#elif defined(TINOX_DB_SQLITE)

// ---- SQLite driver ----
#include <sqlite3.h>

static sqlite3* _tinox_sqlite_db = NULL;

typedef struct {
    int n_cols;
    int n_rows;
    char** data;  // row-major: data[row * n_cols + col]
} TinoxSqliteResult;

void tinox_db_connect(const char* url) {
    // url may be a path or sqlite:///path
    const char* path = url;
    if (strncmp(path, "sqlite:///", 10) == 0) path += 9;
    else if (strncmp(path, "sqlite://", 9) == 0) path += 9;
    if (sqlite3_open(path, &_tinox_sqlite_db) != SQLITE_OK) {
        fprintf(stderr, "SQLite error: %s\n", sqlite3_errmsg(_tinox_sqlite_db));
        exit(1);
    }
}

void* tinox_db_get_conn(void) { return _tinox_sqlite_db; }

// ---- Statement cache (Optimization 1) ----
#define STMT_CACHE_SIZE 64
typedef struct { const char* sql; sqlite3_stmt* stmt; } StmtCacheEntry;
static StmtCacheEntry _stmt_cache[STMT_CACHE_SIZE];

static sqlite3_stmt* _stmt_cache_get(const char* sql) {
    unsigned h = 0;
    for (const char* p = sql; *p; p++) h = h * 31 + (unsigned char)*p;
    h %= STMT_CACHE_SIZE;
    for (int i = 0; i < STMT_CACHE_SIZE; i++) {
        int slot = (h + i) % STMT_CACHE_SIZE;
        if (!_stmt_cache[slot].sql) return NULL;
        if (strcmp(_stmt_cache[slot].sql, sql) == 0) return _stmt_cache[slot].stmt;
    }
    return NULL;
}

static void _stmt_cache_put(const char* sql, sqlite3_stmt* stmt) {
    unsigned h = 0;
    for (const char* p = sql; *p; p++) h = h * 31 + (unsigned char)*p;
    h %= STMT_CACHE_SIZE;
    for (int i = 0; i < STMT_CACHE_SIZE; i++) {
        int slot = (h + i) % STMT_CACHE_SIZE;
        if (!_stmt_cache[slot].sql || strcmp(_stmt_cache[slot].sql, sql) == 0) {
            _stmt_cache[slot].sql = sql;
            _stmt_cache[slot].stmt = stmt;
            return;
        }
    }
    // Cache full: evict slot h (simple strategy)
    sqlite3_finalize(_stmt_cache[h].stmt);
    _stmt_cache[h].sql = sql;
    _stmt_cache[h].stmt = stmt;
}

void* tinox_db_exec(void* conn, const char* sql, const char** params, int64_t n_params) {
    sqlite3* db = (sqlite3*)conn;
    sqlite3_stmt* stmt = _stmt_cache_get(sql);
    if (stmt) {
        sqlite3_reset(stmt);
        sqlite3_clear_bindings(stmt);
    } else {
        if (sqlite3_prepare_v2(db, sql, -1, &stmt, NULL) != SQLITE_OK) {
            fprintf(stderr, "SQLite prepare error: %s\n", sqlite3_errmsg(db));
            return NULL;
        }
        _stmt_cache_put(sql, stmt);
    }
    for (int i = 0; i < (int)n_params; i++) {
        sqlite3_bind_text(stmt, i + 1, params[i], -1, SQLITE_STATIC);
    }

    // First pass: count rows
    int n_rows = 0, n_cols = sqlite3_column_count(stmt);
    // Collect all rows into a temporary list
    char*** rows = NULL;
    int rows_cap = 0;
    int rc;
    while ((rc = sqlite3_step(stmt)) == SQLITE_ROW) {
        if (n_rows >= rows_cap) {
            rows_cap = rows_cap ? rows_cap * 2 : 16;
            rows = (char***)realloc(rows, sizeof(char**) * (size_t)rows_cap);
        }
        rows[n_rows] = (char**)GC_malloc(sizeof(char*) * (size_t)(n_cols > 0 ? n_cols : 1));
        for (int c = 0; c < n_cols; c++) {
            const char* val = (const char*)sqlite3_column_text(stmt, c);
            rows[n_rows][c] = val ? GC_strdup(val) : NULL;
        }
        n_rows++;
    }
    // Do NOT finalize — statement is cached for reuse

    TinoxSqliteResult* res = (TinoxSqliteResult*)GC_malloc(sizeof(TinoxSqliteResult));
    res->n_cols = n_cols;
    res->n_rows = n_rows;
    if (n_rows > 0 && n_cols > 0) {
        res->data = (char**)GC_malloc(sizeof(char*) * (size_t)(n_rows * n_cols));
        for (int r = 0; r < n_rows; r++) {
            for (int c = 0; c < n_cols; c++) {
                res->data[r * n_cols + c] = rows[r][c];
            }
        }
    } else {
        res->data = NULL;
    }
    if (rows) free(rows);
    return (void*)res;
}

int64_t tinox_db_nrows(void* r)                       { return r ? ((TinoxSqliteResult*)r)->n_rows : 0; }
int64_t tinox_db_ncols(void* r)                       { return r ? ((TinoxSqliteResult*)r)->n_cols : 0; }
char*   tinox_db_getval(void* r, int64_t row, int64_t col) {
    TinoxSqliteResult* res = (TinoxSqliteResult*)r;
    if (!res || !res->data) return "";
    char* v = res->data[(int)row * res->n_cols + (int)col];
    return v ? v : "";
}
int64_t tinox_db_getval_int(void* result, int64_t row, int64_t col) {
    TinoxSqliteResult* res = (TinoxSqliteResult*)result;
    if (!res || !res->data) return 0;
    char* v = res->data[(int)row * res->n_cols + (int)col];
    if (!v) return 0;
    return (int64_t)atoll(v);
}
bool    tinox_db_is_null(void* r, int64_t row, int64_t col) {
    TinoxSqliteResult* res = (TinoxSqliteResult*)r;
    if (!res || !res->data) return true;
    return res->data[(int)row * res->n_cols + (int)col] == NULL;
}
void    tinox_db_free(void* r) { (void)r; }
char*   tinox_db_error(void* c) { return GC_strdup(sqlite3_errmsg((sqlite3*)c)); }

#elif defined(TINOX_DB_MYSQL)

// ---- MySQL driver ----
#include <mysql/mysql.h>

static MYSQL* _tinox_mysql_conn = NULL;
static pthread_mutex_t _tinox_mysql_mu = PTHREAD_MUTEX_INITIALIZER;

// URL format: mysql://user:pass@host:port/database
static void _parse_mysql_url(const char* url,
    char* host, char* user, char* pass, char* db, unsigned int* port) {
    // Defaults
    strcpy(host, "127.0.0.1");
    strcpy(user, "root");
    strcpy(pass, "");
    strcpy(db,   "");
    *port = 3306;

    // Skip "mysql://"
    const char* p = url;
    if (strncmp(p, "mysql://", 8) == 0) p += 8;

    // user:pass@
    const char* at = strchr(p, '@');
    if (at) {
        char userinfo[256];
        strncpy(userinfo, p, (size_t)(at - p));
        userinfo[at - p] = '\0';
        const char* colon = strchr(userinfo, ':');
        if (colon) {
            strncpy(user, userinfo, (size_t)(colon - userinfo));
            user[colon - userinfo] = '\0';
            strcpy(pass, colon + 1);
        } else {
            strcpy(user, userinfo);
        }
        p = at + 1;
    }

    // host:port/db
    const char* slash = strchr(p, '/');
    if (slash) {
        char hostport[256];
        strncpy(hostport, p, (size_t)(slash - p));
        hostport[slash - p] = '\0';
        strcpy(db, slash + 1);
        const char* portcolon = strchr(hostport, ':');
        if (portcolon) {
            strncpy(host, hostport, (size_t)(portcolon - hostport));
            host[portcolon - hostport] = '\0';
            *port = (unsigned int)atoi(portcolon + 1);
        } else {
            strcpy(host, hostport);
        }
    } else {
        strcpy(host, p);
    }
}

void tinox_db_connect(const char* url) {
    _tinox_mysql_conn = mysql_init(NULL);
    if (!_tinox_mysql_conn) {
        fprintf(stderr, "MySQL init failed\n");
        exit(1);
    }
    char host[256], user[256], pass[256], db[256];
    unsigned int port;
    _parse_mysql_url(url, host, user, pass, db, &port);
    if (!mysql_real_connect(_tinox_mysql_conn, host, user, pass, db, port, NULL, 0)) {
        fprintf(stderr, "MySQL connection failed: %s\n", mysql_error(_tinox_mysql_conn));
        mysql_close(_tinox_mysql_conn);
        exit(1);
    }
}

typedef struct {
    int n_cols;
    int n_rows;
    char** data;   // row-major: data[row * n_cols + col]
} TinoxMysqlResult;

void* tinox_db_get_conn(void) { return _tinox_mysql_conn; }

void* tinox_db_exec(void* conn, const char* sql, const char** params, int64_t n_params) {
    MYSQL_STMT* stmt = mysql_stmt_init((MYSQL*)conn);
    if (mysql_stmt_prepare(stmt, sql, (unsigned long)strlen(sql)) != 0) {
        fprintf(stderr, "MySQL prepare error: %s\n", mysql_stmt_error(stmt));
        mysql_stmt_close(stmt);
        return NULL;
    }

    MYSQL_BIND* bind = NULL;
    unsigned long* lengths = NULL;
    if (n_params > 0) {
        bind    = (MYSQL_BIND*)calloc((size_t)n_params, sizeof(MYSQL_BIND));
        lengths = (unsigned long*)calloc((size_t)n_params, sizeof(unsigned long));
        for (int i = 0; i < (int)n_params; i++) {
            lengths[i] = params[i] ? (unsigned long)strlen(params[i]) : 0;
            bind[i].buffer_type   = MYSQL_TYPE_STRING;
            bind[i].buffer        = (char*)params[i];
            bind[i].buffer_length = lengths[i];
            bind[i].length        = &lengths[i];
        }
        mysql_stmt_bind_param(stmt, bind);
    }

    if (mysql_stmt_execute(stmt) != 0) {
        fprintf(stderr, "MySQL execute error: %s\n", mysql_stmt_error(stmt));
        if (bind)    free(bind);
        if (lengths) free(lengths);
        mysql_stmt_close(stmt);
        return NULL;
    }

    MYSQL_RES* meta = mysql_stmt_result_metadata(stmt);
    int n_cols = meta ? mysql_num_fields(meta) : 0;
    mysql_stmt_store_result(stmt);
    int n_rows = (int)mysql_stmt_num_rows(stmt);

    TinoxMysqlResult* res = (TinoxMysqlResult*)GC_malloc(sizeof(TinoxMysqlResult));
    res->n_cols = n_cols;
    res->n_rows = n_rows;
    res->data   = n_cols * n_rows > 0
        ? (char**)GC_malloc(sizeof(char*) * (size_t)(n_cols * n_rows))
        : NULL;

    if (n_cols > 0 && n_rows > 0) {
        MYSQL_BIND* out_bind = (MYSQL_BIND*)calloc((size_t)n_cols, sizeof(MYSQL_BIND));
        char** bufs    = (char**)calloc((size_t)n_cols, sizeof(char*));
        unsigned long* out_len = (unsigned long*)calloc((size_t)n_cols, sizeof(unsigned long));
        for (int c = 0; c < n_cols; c++) {
            bufs[c] = (char*)GC_malloc(512);
            out_bind[c].buffer_type   = MYSQL_TYPE_STRING;
            out_bind[c].buffer        = bufs[c];
            out_bind[c].buffer_length = 511;
            out_bind[c].length        = &out_len[c];
        }
        mysql_stmt_bind_result(stmt, out_bind);
        for (int r = 0; r < n_rows; r++) {
            mysql_stmt_fetch(stmt);
            for (int c = 0; c < n_cols; c++) {
                bufs[c][out_len[c]] = '\0';
                res->data[r * n_cols + c] = GC_strdup(bufs[c]);
            }
        }
        free(out_bind);
        free(bufs);
        free(out_len);
    }

    if (meta)    mysql_free_result(meta);
    if (bind)    free(bind);
    if (lengths) free(lengths);
    mysql_stmt_close(stmt);
    return (void*)res;
}

int64_t tinox_db_nrows(void* r)              { return r ? ((TinoxMysqlResult*)r)->n_rows : 0; }
int64_t tinox_db_ncols(void* r)              { return r ? ((TinoxMysqlResult*)r)->n_cols : 0; }
char*   tinox_db_getval(void* r, int64_t row, int64_t col) {
    TinoxMysqlResult* res = (TinoxMysqlResult*)r;
    if (!res || !res->data) return "";
    return res->data[(int)row * res->n_cols + (int)col];
}
int64_t tinox_db_getval_int(void* r, int64_t row, int64_t col) {
    TinoxMysqlResult* res = (TinoxMysqlResult*)r;
    if (!res || !res->data) return 0;
    char* v = res->data[(int)row * res->n_cols + (int)col];
    if (!v) return 0;
    return (int64_t)atoll(v);
}
bool    tinox_db_is_null(void* r, int64_t row, int64_t col) {
    TinoxMysqlResult* res = (TinoxMysqlResult*)r;
    if (!res || !res->data) return true;
    return res->data[(int)row * res->n_cols + (int)col] == NULL;
}
void    tinox_db_free(void* r) { (void)r; /* GC managed */ }
char*   tinox_db_error(void* c) { return GC_strdup(mysql_error((MYSQL*)c)); }

#else
// Stub implementations when no DB driver is selected — prevent link errors.
void  tinox_db_connect(const char* url)                                       { (void)url; }
void* tinox_db_get_conn(void)                                                  { return NULL; }
void* tinox_db_exec(void* c, const char* s, const char** p, int64_t n)        { (void)c;(void)s;(void)p;(void)n; return NULL; }
int64_t tinox_db_nrows(void* r)                                                { (void)r; return 0; }
int64_t tinox_db_ncols(void* r)                                                { (void)r; return 0; }
char*   tinox_db_getval(void* r, int64_t row, int64_t col)                    { (void)r;(void)row;(void)col; return ""; }
int64_t tinox_db_getval_int(void* r, int64_t row, int64_t col)                { (void)r;(void)row;(void)col; return 0; }
bool    tinox_db_is_null(void* r, int64_t row, int64_t col)                   { (void)r;(void)row;(void)col; return true; }
void    tinox_db_free(void* r)                                                 { (void)r; }
char*   tinox_db_error(void* c)                                                { (void)c; return ""; }
#endif /* DB driver */

// Param helpers (always available)
char** tinox_params_alloc(int64_t n) {
    return (char**)GC_malloc(sizeof(char*) * (size_t)n);
}

void tinox_params_set(char** params, int64_t idx, const char* val) {
    params[idx] = (char*)val;
}

char* tinox_int_to_param(int64_t val) {
    char* buf = (char*)GC_malloc(32);
    snprintf(buf, 32, "%ld", (long)val);
    return buf;
}

// ---- Entry point ----

extern int64_t tinox_main(void);

int main(int argc, char** argv) {
    GC_INIT();
    _tinox_argc = argc;
    _tinox_argv = argv;
    return (int)tinox_main();
}
