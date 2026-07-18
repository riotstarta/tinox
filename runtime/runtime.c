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
#ifdef __GLIBC__
#include <execinfo.h>
#endif

#ifdef TINOX_NO_GC
// Sanitizer-Modus (make asan): plain malloc statt Boehm-GC, damit ASan
// jede Allokation sieht (GC-Heap ist für ASan unsichtbar). Nichts wird
// freigegeben — Leaks sind hier Absicht, Overflows/UAF das Ziel.
#include <string.h>
#define GC_malloc(s)     calloc(1, (s))
#define GC_realloc(p, s) realloc((p), (s))
#define GC_free(p)       ((void)(p))
#define GC_strdup(s)     strdup(s)
#define GC_INIT()        ((void)0)
#else
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
#endif

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

// ---- Tinox arrays: stable handle {len, cap, data} ----
// A Tinox array value is a pointer to this 3-slot handle. push/pop/removeAt
// mutate the handle in place (push is amortized O(1) via geometric growth);
// slice/sort/reverse return fresh arrays. All aliases share the handle.
typedef struct {
    int64_t len;
    int64_t cap;
    int64_t* data;
} TinoxArray;

// ---- --checked: Heap-Kind-Registry (TESTPLAN Phase 4) ----
// Arrays/Maps entstehen ausschließlich über ihre Konstruktoren; in
// checked-Builds (-DTINOX_CHECKED, via `tinox build --checked`)
// registrieren die den Pointer, und jede Array-/Map-Runtime-Funktion
// prüft ihn. Ein Dispatch-Bug (map_len auf einem String, array_push auf
// einer Map) bricht dann laut ab, statt still Heap-Müll zu lesen
// (Bug-15-Klasse). Kein ABI-/Layout-Unterschied zum normalen Build —
// die Registry ist eine Seitentabelle (plain malloc, GC-unsichtbar;
// Adress-Wiederverwendung wird durch Neuregistrierung im Konstruktor
// aktualisiert).
#ifdef TINOX_CHECKED
#define TINOX_KIND_ARRAY 1
#define TINOX_KIND_MAP 2

static pthread_mutex_t _tinox_ck_mu = PTHREAD_MUTEX_INITIALIZER;
static uintptr_t* _tinox_ck_keys = NULL;
static unsigned char* _tinox_ck_kinds = NULL;
static size_t _tinox_ck_cap = 0;
static size_t _tinox_ck_len = 0;

static void _tinox_ck_insert_raw(uintptr_t key, unsigned char kind) {
    size_t i = (key >> 4) & (_tinox_ck_cap - 1);
    while (_tinox_ck_keys[i] != 0 && _tinox_ck_keys[i] != key) {
        i = (i + 1) & (_tinox_ck_cap - 1);
    }
    if (_tinox_ck_keys[i] == 0) _tinox_ck_len++;
    _tinox_ck_keys[i] = key;
    _tinox_ck_kinds[i] = kind;
}

static void tinox_checked_register(const void* p, unsigned char kind) {
    if (!p) return;
    pthread_mutex_lock(&_tinox_ck_mu);
    if (_tinox_ck_len * 2 >= _tinox_ck_cap) {
        size_t new_cap = _tinox_ck_cap ? _tinox_ck_cap * 2 : 1024;
        uintptr_t* old_keys = _tinox_ck_keys;
        unsigned char* old_kinds = _tinox_ck_kinds;
        size_t old_cap = _tinox_ck_cap;
        _tinox_ck_keys = (uintptr_t*)calloc(new_cap, sizeof(uintptr_t));
        _tinox_ck_kinds = (unsigned char*)calloc(new_cap, 1);
        _tinox_ck_cap = new_cap;
        _tinox_ck_len = 0;
        for (size_t i = 0; i < old_cap; i++) {
            if (old_keys[i]) _tinox_ck_insert_raw(old_keys[i], old_kinds[i]);
        }
        free(old_keys);
        free(old_kinds);
    }
    _tinox_ck_insert_raw((uintptr_t)p, kind);
    pthread_mutex_unlock(&_tinox_ck_mu);
}

static const char* _tinox_ck_kind_name(unsigned char kind) {
    switch (kind) {
        case TINOX_KIND_ARRAY: return "Array";
        case TINOX_KIND_MAP: return "Map";
        default: return "unregistriert (String/Objekt?)";
    }
}

static void tinox_checked_expect(const void* p, unsigned char kind, const char* op) {
    if (!p) return;
    unsigned char found = 0;
    pthread_mutex_lock(&_tinox_ck_mu);
    if (_tinox_ck_cap) {
        uintptr_t key = (uintptr_t)p;
        size_t i = (key >> 4) & (_tinox_ck_cap - 1);
        while (_tinox_ck_keys[i] != 0) {
            if (_tinox_ck_keys[i] == key) { found = _tinox_ck_kinds[i]; break; }
            i = (i + 1) & (_tinox_ck_cap - 1);
        }
    }
    pthread_mutex_unlock(&_tinox_ck_mu);
    if (found != kind) {
        fprintf(stderr,
            "tinox --checked: %s auf %s-Pointer %p (erwartet: %s) — "
            "Codegen-Dispatch-Bug, bitte mit Quelldatei melden\n",
            op, _tinox_ck_kind_name(found), p, _tinox_ck_kind_name(kind));
        abort();
    }
}

#define TINOX_CK_REG(p, k) tinox_checked_register((p), (k))
#define TINOX_CK_EXPECT(p, k, op) tinox_checked_expect((p), (k), (op))
#else
#define TINOX_CK_REG(p, k) ((void)0)
#define TINOX_CK_EXPECT(p, k, op) ((void)0)
#endif

int64_t* tinox_array_new(int64_t len, int64_t cap) {
    if (cap < len) cap = len;
    if (cap < 4) cap = 4;
    TinoxArray* a = (TinoxArray*)GC_malloc(sizeof(TinoxArray));
    a->len = len;
    a->cap = cap;
    a->data = (int64_t*)GC_malloc((size_t)cap * sizeof(int64_t));
    TINOX_CK_REG(a, TINOX_KIND_ARRAY);
    return (int64_t*)a;
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
    char* result = malloc(40);
    // shortest representation that round-trips exactly
    for (int prec = 15; prec <= 17; prec++) {
        snprintf(result, 40, "%.*g", prec, val);
        if (strtod(result, NULL) == val) return result;
    }
    return result;
}

int64_t* tinox_array_slice(int64_t* h, int64_t from, int64_t to) {
    TINOX_CK_EXPECT(h, TINOX_KIND_ARRAY, "array_slice");
    TinoxArray* a = (TinoxArray*)h;
    if (from < 0) from = 0;
    if (to > a->len) to = a->len;
    int64_t len = to - from;
    if (len < 0) len = 0;
    int64_t* nh = tinox_array_new(len, 0);
    if (len > 0) memcpy(((TinoxArray*)nh)->data, a->data + from, (size_t)len * sizeof(int64_t));
    return nh;
}

int64_t* tinox_array_push(int64_t* h, int64_t val) {
    TINOX_CK_EXPECT(h, TINOX_KIND_ARRAY, "array_push");
    TinoxArray* a = (TinoxArray*)h;
    if (a->len == a->cap) {
        int64_t ncap = a->cap < 4 ? 4 : a->cap * 2;
        int64_t* nd = (int64_t*)GC_malloc((size_t)ncap * sizeof(int64_t));
        if (a->len > 0) memcpy(nd, a->data, (size_t)a->len * sizeof(int64_t));
        a->data = nd;
        a->cap = ncap;
    }
    a->data[a->len++] = val;
    return h;
}

int64_t* tinox_array_pop(int64_t* h) {
    TINOX_CK_EXPECT(h, TINOX_KIND_ARRAY, "array_pop");
    TinoxArray* a = (TinoxArray*)h;
    if (a->len > 0) a->len--;
    return h;
}

// Serialize a list of @JsonSerializable objects: "[" + toJson(elem) joined
// with "," + "]". to_json is the class's generated ClassName_toJson.
char* tinox_json_list_serialize(int64_t* h, char* (*to_json)(void*)) {
    TinoxArray* a = (TinoxArray*)h;
    int64_t n = a ? a->len : 0;
    char** parts = (char**)malloc(sizeof(char*) * (n > 0 ? (size_t)n : 1));
    size_t total = 2; // "[" + "]"
    for (int64_t i = 0; i < n; i++) {
        parts[i] = to_json((void*)(uintptr_t)a->data[i]);
        total += strlen(parts[i]) + 1; // + ","
    }
    char* out = (char*)malloc(total + 1);
    size_t pos = 0;
    out[pos++] = '[';
    for (int64_t i = 0; i < n; i++) {
        if (i > 0) out[pos++] = ',';
        size_t l = strlen(parts[i]);
        memcpy(out + pos, parts[i], l);
        pos += l;
    }
    out[pos++] = ']';
    out[pos] = '\0';
    return out;
}

// Insert val at index idx (clamped to [0, len]), shifting the tail right.
int64_t* tinox_array_insert(int64_t* h, int64_t idx, int64_t val) {
    TINOX_CK_EXPECT(h, TINOX_KIND_ARRAY, "array_insert");
    TinoxArray* a = (TinoxArray*)h;
    if (idx < 0) idx = 0;
    if (idx > a->len) idx = a->len;
    if (a->len == a->cap) {
        int64_t ncap = a->cap < 4 ? 4 : a->cap * 2;
        int64_t* nd = (int64_t*)GC_malloc((size_t)ncap * sizeof(int64_t));
        if (a->len > 0) memcpy(nd, a->data, (size_t)a->len * sizeof(int64_t));
        a->data = nd;
        a->cap = ncap;
    }
    memmove(a->data + idx + 1, a->data + idx, (size_t)(a->len - idx) * sizeof(int64_t));
    a->data[idx] = val;
    a->len++;
    return h;
}

char* tinox_char_at(const char* s, int64_t i) {
    char* result = malloc(2);
    result[0] = s[i];
    result[1] = '\0';
    return result;
}

// Single-char string from a byte value (fromCharCode builtin)
char* tinox_from_char_code(int64_t c) {
    char* result = malloc(2);
    result[0] = (char)c;
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

int64_t tinox_string_compare(const char* a, const char* b) {
    if (a == b) return 0;
    if (!a) return -1;
    if (!b) return 1;
    int r = strcmp(a, b);
    return r < 0 ? -1 : (r > 0 ? 1 : 0);
}

int64_t tinox_string_contains(const char* haystack, const char* needle) {
    return strstr(haystack, needle) != NULL ? 1 : 0;
}

int64_t tinox_string_index_of(const char* haystack, const char* needle) {
    const char* pos = strstr(haystack, needle);
    return pos ? (int64_t)(pos - haystack) : -1;
}

int64_t tinox_string_last_index_of(const char* haystack, const char* needle) {
    size_t hlen = strlen(haystack);
    size_t nlen = strlen(needle);
    if (nlen > hlen) return -1;
    for (size_t i = hlen - nlen + 1; i-- > 0; ) {
        if (memcmp(haystack + i, needle, nlen) == 0) return (int64_t)i;
    }
    return -1;
}

char* tinox_string_reverse(const char* s) {
    size_t len = strlen(s);
    char* result = malloc(len + 1);
    for (size_t i = 0; i < len; i++)
        result[i] = s[len - 1 - i];
    result[len] = '\0';
    return result;
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

int64_t* tinox_array_sort(int64_t* h) {
    TINOX_CK_EXPECT(h, TINOX_KIND_ARRAY, "array_sort");
    TinoxArray* a = (TinoxArray*)h;
    int64_t len = a->len;
    int64_t* nh = tinox_array_new(len, 0);
    int64_t* nd = ((TinoxArray*)nh)->data;
    if (len > 0) memcpy(nd, a->data, (size_t)len * sizeof(int64_t));
    if (len > 1) sort_i64_range(nd, 0, len - 1);
    return nh;
}

int64_t* tinox_array_reverse(int64_t* h) {
    TINOX_CK_EXPECT(h, TINOX_KIND_ARRAY, "array_reverse");
    TinoxArray* a = (TinoxArray*)h;
    int64_t len = a->len;
    int64_t* nh = tinox_array_new(len, 0);
    int64_t* nd = ((TinoxArray*)nh)->data;
    for (int64_t i = 0; i < len; i++) nd[i] = a->data[len - 1 - i];
    return nh;
}

int64_t tinox_array_contains(int64_t* h, int64_t val) {
    TINOX_CK_EXPECT(h, TINOX_KIND_ARRAY, "array_contains");
    TinoxArray* a = (TinoxArray*)h;
    for (int64_t i = 0; i < a->len; i++) if (a->data[i] == val) return 1;
    return 0;
}

int64_t tinox_array_index_of(int64_t* h, int64_t val) {
    TINOX_CK_EXPECT(h, TINOX_KIND_ARRAY, "array_indexOf");
    TinoxArray* a = (TinoxArray*)h;
    for (int64_t i = 0; i < a->len; i++) if (a->data[i] == val) return i;
    return -1;
}

int64_t* tinox_array_remove_at(int64_t* h, int64_t idx) {
    TINOX_CK_EXPECT(h, TINOX_KIND_ARRAY, "array_removeAt");
    TinoxArray* a = (TinoxArray*)h;
    if (idx < 0 || idx >= a->len) return h;
    for (int64_t i = idx; i < a->len - 1; i++) a->data[i] = a->data[i + 1];
    a->len--;
    return h;
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
    TINOX_CK_REG(m, TINOX_KIND_MAP);
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
    TINOX_CK_REG(m, TINOX_KIND_MAP);
    return m;
}

void tinox_map_set(void* map, const char* key, int64_t value) {
    TINOX_CK_EXPECT(map, TINOX_KIND_MAP, "map_set");
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
    TINOX_CK_EXPECT(map, TINOX_KIND_MAP, "map_get");
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
    TINOX_CK_EXPECT(map, TINOX_KIND_MAP, "map_contains");
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
    TINOX_CK_EXPECT(map, TINOX_KIND_MAP, "map_remove");
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
    TINOX_CK_EXPECT(map, TINOX_KIND_MAP, "map_len");
    return (int64_t)((TinoxMap*)map)->len;
}

int64_t* tinox_map_keys(void* map) {
    TINOX_CK_EXPECT(map, TINOX_KIND_MAP, "map_keys");
    TinoxMap* m = (TinoxMap*)map;
    int64_t* nh = tinox_array_new((int64_t)m->len, 0);
    int64_t* nd = ((TinoxArray*)nh)->data;
    size_t j = 0;
    for (size_t i = 0; i < m->cap; i++) {
        char* k = m->entries[i].key;
        if (k && k != (char*)1)
            nd[j++] = (int64_t)(uintptr_t)k;
    }
    return nh;
}

int64_t* tinox_map_values(void* map) {
    TINOX_CK_EXPECT(map, TINOX_KIND_MAP, "map_values");
    TinoxMap* m = (TinoxMap*)map;
    int64_t* nh = tinox_array_new((int64_t)m->len, 0);
    int64_t* nd = ((TinoxArray*)nh)->data;
    size_t j = 0;
    for (size_t i = 0; i < m->cap; i++) {
        char* k = m->entries[i].key;
        if (k && k != (char*)1)
            nd[j++] = m->entries[i].value;
    }
    return nh;
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
    int64_t* nh = tinox_array_new((int64_t)count, 0);
    int64_t* nd = ((TinoxArray*)nh)->data;
    if (dlen == 0) {
        for (size_t i = 0; i < count; i++) {
            char* s = (char*)malloc(2);
            s[0] = str[i]; s[1] = '\0';
            nd[i] = (int64_t)(uintptr_t)s;
        }
        return nh;
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
    return nh;
}

char* tinox_string_join(int64_t* h, const char* sep) {
    TinoxArray* a = (TinoxArray*)h;
    int64_t* arr = a->data;
    int64_t len = a->len;
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

// ---- High-level file I/O (used by Tinox builtins) ----

char* fileReadAllText(const char* path) {
    FILE* f = fopen(path, "rb");
    if (!f) return GC_strdup("");
    // Try seek-based size detection first (works for regular files)
    if (fseek(f, 0, SEEK_END) == 0) {
        long size = ftell(f);
        if (size > 0) {
            fseek(f, 0, SEEK_SET);
            char* buf = (char*)GC_malloc(size + 1);
            size_t got = fread(buf, 1, size, f);
            fclose(f);
            buf[got] = '\0';
            return buf;
        }
        fseek(f, 0, SEEK_SET);
    }
    // Fall back to dynamic read (for pipes, /dev/stdin, character devices)
    size_t capacity = 4096;
    size_t used = 0;
    char* buf = (char*)GC_malloc(capacity);
    size_t n;
    while ((n = fread(buf + used, 1, capacity - used - 1, f)) > 0) {
        used += n;
        if (used + 1 >= capacity) {
            capacity *= 2;
            char* newbuf = (char*)GC_malloc(capacity);
            memcpy(newbuf, buf, used);
            buf = newbuf;
        }
    }
    fclose(f);
    buf[used] = '\0';
    return buf;
}

void fileWriteAllText(const char* path, const char* content) {
    FILE* f = fopen(path, "w");
    if (f) { fputs(content, f); fclose(f); }
}

void fileAppendText(const char* path, const char* content) {
    FILE* f = fopen(path, "a");
    if (f) { fputs(content, f); fclose(f); }
}

void fileClose(void* handle) {
    if (handle) fclose((FILE*)handle);
}

// ---- Socket builtins (tinox.core.socket) ----
// Handles sind rohe fds als i64; -1 = Fehler. Blockierende BSD-Sockets —
// bewusst einfach gehalten (kein epoll hier; der HTTP-Server weiter unten
// hat seine eigene epoll-Maschinerie).

#include <netdb.h>

int64_t socketCreateTcp(void) {
    return (int64_t)socket(AF_INET, SOCK_STREAM, 0);
}

int64_t socketCreateUdp(void) {
    return (int64_t)socket(AF_INET, SOCK_DGRAM, 0);
}

bool socketConnect(int64_t fd, const char* host, int64_t port) {
    if (fd < 0) return false;
    char port_str[16];
    snprintf(port_str, sizeof(port_str), "%ld", (long)port);
    struct addrinfo hints, *res = NULL;
    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_INET;
    // Der Socket-Typ steht schon fest (fd existiert) — getaddrinfo nur zur
    // Namensauflösung, SOCK_STREAM als Filter reicht für A-Records.
    hints.ai_socktype = SOCK_STREAM;
    if (getaddrinfo(host, port_str, &hints, &res) != 0 || !res) return false;
    int r = connect((int)fd, res->ai_addr, res->ai_addrlen);
    freeaddrinfo(res);
    return r == 0;
}

bool socketBind(int64_t fd, int64_t port) {
    if (fd < 0) return false;
    int opt = 1;
    setsockopt((int)fd, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt));
    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_addr.s_addr = htonl(INADDR_ANY);
    addr.sin_port = htons((uint16_t)port);
    return bind((int)fd, (struct sockaddr*)&addr, sizeof(addr)) == 0;
}

bool socketListen(int64_t fd) {
    if (fd < 0) return false;
    return listen((int)fd, 16) == 0;
}

int64_t socketAccept(int64_t fd) {
    if (fd < 0) return -1;
    return (int64_t)accept((int)fd, NULL, NULL);
}

int64_t socketSend(int64_t fd, const char* data) {
    if (fd < 0) return -1;
    size_t len = strlen(data);
    ssize_t sent = send((int)fd, data, len, 0);
    return (int64_t)sent;
}

char* socketReceive(int64_t fd, int64_t size) {
    if (fd < 0 || size <= 0) return GC_strdup("");
    char* buf = (char*)GC_malloc((size_t)size + 1);
    ssize_t n = recv((int)fd, buf, (size_t)size, 0);
    if (n <= 0) { buf[0] = '\0'; return buf; }
    buf[n] = '\0';
    return buf;
}

// Roh-Bytes von einem fd lesen (HTTP/2-Server-Framing). Bis zu count Bytes;
// die tatsächlich gelesenen werden als String zurückgegeben ("" bei EOF/Fehler).
char* httpServerReadRawBytes(int64_t fd, int64_t count) {
    if (fd < 0 || count <= 0) return GC_strdup("");
    char* buf = (char*)GC_malloc((size_t)count + 1);
    ssize_t n = read((int)fd, buf, (size_t)count);
    if (n <= 0) { buf[0] = '\0'; return buf; }
    buf[n] = '\0';
    return buf;
}

void socketClose(int64_t fd) {
    if (fd >= 0) close((int)fd);
}

// ---- HTTP/1.1 client builtins (tinox.core.http) ----
// Plaintext http:// only (kein TLS). Baut auf denselben blockierenden
// BSD-Sockets auf wie oben. Request-Header sind thread-lokaler Zustand
// (httpSetHeader/httpClearHeaders), gespiegelt auf die C-Globals-Konvention
// der db/metrics-Module.

typedef struct {
    int64_t status;
    char*   body;
    char*   headers; // roher Header-Block (ohne Statuszeile) für httpHeader()
} TinoxHttpResponse;

static __thread char* _tinox_http_req_headers = NULL; // "Name: Value\r\n"-Kette

void httpSetHeader(const char* name, const char* value) {
    size_t old_len = _tinox_http_req_headers ? strlen(_tinox_http_req_headers) : 0;
    size_t add = strlen(name) + strlen(value) + 4; // ": " + "\r\n"
    char* buf = (char*)malloc(old_len + add + 1);
    if (old_len) memcpy(buf, _tinox_http_req_headers, old_len);
    snprintf(buf + old_len, add + 1, "%s: %s\r\n", name, value);
    _tinox_http_req_headers = buf;
}

void httpClearHeaders(void) {
    _tinox_http_req_headers = NULL;
}

// Zerlegt "http://host[:port]/path" → host, port, path. Gibt 0 bei Nicht-http.
static int http_parse_url(const char* url, char* host, size_t host_sz,
                          int* port, char* path, size_t path_sz) {
    const char* p = url;
    if (strncmp(p, "http://", 7) == 0) p += 7;
    else if (strncmp(p, "https://", 8) == 0) return 0; // TLS nicht unterstützt
    else return 0;

    const char* host_start = p;
    while (*p && *p != ':' && *p != '/') p++;
    size_t hlen = (size_t)(p - host_start);
    if (hlen == 0 || hlen >= host_sz) return 0;
    memcpy(host, host_start, hlen);
    host[hlen] = '\0';

    *port = 80;
    if (*p == ':') {
        p++;
        *port = atoi(p);
        while (*p && *p != '/') p++;
    }
    if (*p == '\0') {
        snprintf(path, path_sz, "/");
    } else {
        snprintf(path, path_sz, "%s", p);
    }
    return 1;
}

static char* http_recv_all(int fd) {
    size_t cap = 8192, len = 0;
    char* buf = (char*)malloc(cap);
    ssize_t n;
    while ((n = recv(fd, buf + len, cap - len, 0)) > 0) {
        len += (size_t)n;
        if (len == cap) {
            cap *= 2;
            char* grown = (char*)malloc(cap);
            memcpy(grown, buf, len);
            buf = grown;
        }
    }
    buf[len] = '\0';
    return buf;
}

static TinoxHttpResponse* http_request(const char* method, const char* url, const char* body) {
    TinoxHttpResponse* resp = (TinoxHttpResponse*)malloc(sizeof(TinoxHttpResponse));
    resp->status = 0;
    resp->body = GC_strdup("");
    resp->headers = GC_strdup("");

    char host[256], path[2048];
    int port;
    if (!http_parse_url(url, host, sizeof(host), &port, path, sizeof(path))) return resp;

    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) return resp;

    char port_str[16];
    snprintf(port_str, sizeof(port_str), "%d", port);
    struct addrinfo hints, *res = NULL;
    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_INET;
    hints.ai_socktype = SOCK_STREAM;
    if (getaddrinfo(host, port_str, &hints, &res) != 0 || !res) { close(fd); return resp; }
    if (connect(fd, res->ai_addr, res->ai_addrlen) != 0) { freeaddrinfo(res); close(fd); return resp; }
    freeaddrinfo(res);

    size_t body_len = body ? strlen(body) : 0;
    const char* extra = _tinox_http_req_headers ? _tinox_http_req_headers : "";
    size_t req_cap = strlen(method) + strlen(path) + strlen(host) + strlen(extra) + body_len + 256;
    char* req = (char*)malloc(req_cap);
    int req_len = snprintf(req, req_cap,
        "%s %s HTTP/1.1\r\nHost: %s\r\nConnection: close\r\n%s"
        "Content-Length: %zu\r\n\r\n",
        method, path, host, extra, body_len);
    if (body_len) {
        memcpy(req + req_len, body, body_len);
        req_len += (int)body_len;
    }

    ssize_t sent_total = 0;
    while (sent_total < req_len) {
        ssize_t s = send(fd, req + sent_total, (size_t)(req_len - sent_total), 0);
        if (s <= 0) break;
        sent_total += s;
    }

    char* raw = http_recv_all(fd);
    close(fd);

    // Statuszeile: "HTTP/1.1 200 OK"
    const char* sp = strchr(raw, ' ');
    if (sp) resp->status = atoi(sp + 1);

    // Header/Body-Trennung an "\r\n\r\n"
    char* sep = strstr(raw, "\r\n\r\n");
    if (sep) {
        size_t hdr_len = (size_t)(sep - raw);
        char* hdrs = (char*)GC_malloc(hdr_len + 1);
        memcpy(hdrs, raw, hdr_len);
        hdrs[hdr_len] = '\0';
        resp->headers = hdrs;
        resp->body = GC_strdup(sep + 4);
    } else {
        resp->body = GC_strdup(raw);
    }
    return resp;
}

int64_t* httpGet(const char* url)                    { return (int64_t*)http_request("GET", url, NULL); }
int64_t* httpPost(const char* url, const char* body) { return (int64_t*)http_request("POST", url, body); }
int64_t* httpPut(const char* url, const char* body)  { return (int64_t*)http_request("PUT", url, body); }
int64_t* httpDelete(const char* url)                 { return (int64_t*)http_request("DELETE", url, NULL); }
int64_t* httpPatch(const char* url, const char* body){ return (int64_t*)http_request("PATCH", url, body); }

int64_t httpStatusCode(int64_t* resp) {
    return resp ? ((TinoxHttpResponse*)resp)->status : 0;
}

char* httpBody(int64_t* resp) {
    if (!resp) return GC_strdup("");
    char* b = ((TinoxHttpResponse*)resp)->body;
    return b ? b : GC_strdup("");
}

// Case-insensitive Header-Lookup im rohen Header-Block. "" wenn nicht da.
char* httpHeader(int64_t* resp, const char* name) {
    if (!resp) return GC_strdup("");
    const char* hdrs = ((TinoxHttpResponse*)resp)->headers;
    if (!hdrs) return GC_strdup("");
    size_t nlen = strlen(name);
    const char* line = hdrs;
    while (*line) {
        const char* eol = strstr(line, "\r\n");
        size_t line_len = eol ? (size_t)(eol - line) : strlen(line);
        if (line_len > nlen && line[nlen] == ':' && strncasecmp(line, name, nlen) == 0) {
            const char* v = line + nlen + 1;
            while (*v == ' ') v++;
            size_t vlen = line_len - (size_t)(v - line);
            char* out = (char*)GC_malloc(vlen + 1);
            memcpy(out, v, vlen);
            out[vlen] = '\0';
            return out;
        }
        if (!eol) break;
        line = eol + 2;
    }
    return GC_strdup("");
}

// ---- ZIP builtins (STORED/Methode 0, Textinhalte) ---------------------------
// Minimaler, aber echter ZIP-Reader/-Writer: schreibt gültige .zip-Dateien
// (von `unzip` lesbar), unterstützt beim Lesen nur unkomprimierte Einträge
// (Methode 0). Binärinhalte mit Nullbytes sind nicht darstellbar, da Tinox-
// Strings nullterminiert sind. Die Tinox-Seite (Zip::listEntries) baut die
// List<ZipEntry> selbst aus zipEntryCount/zipEntryName/zipEntrySize — so bleibt
// C von der Klassen-ABI entkoppelt.

typedef struct {
    char*          name;
    unsigned char* data;
    uint32_t       size;
} TinoxZipMember;

static uint32_t tinox_zip_crc32(const unsigned char* data, size_t len) {
    static uint32_t table[256];
    static int have_table = 0;
    if (!have_table) {
        for (uint32_t i = 0; i < 256; i++) {
            uint32_t c = i;
            for (int k = 0; k < 8; k++)
                c = (c & 1u) ? (0xEDB88320u ^ (c >> 1)) : (c >> 1);
            table[i] = c;
        }
        have_table = 1;
    }
    uint32_t crc = 0xFFFFFFFFu;
    for (size_t i = 0; i < len; i++)
        crc = table[(crc ^ data[i]) & 0xFFu] ^ (crc >> 8);
    return crc ^ 0xFFFFFFFFu;
}

static uint16_t tinox_zip_rd16(const unsigned char* p) {
    return (uint16_t)((uint16_t)p[0] | ((uint16_t)p[1] << 8));
}
static uint32_t tinox_zip_rd32(const unsigned char* p) {
    return (uint32_t)p[0] | ((uint32_t)p[1] << 8)
         | ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
}
static void tinox_zip_wr16(unsigned char* p, uint16_t v) {
    p[0] = (unsigned char)(v & 0xFF); p[1] = (unsigned char)((v >> 8) & 0xFF);
}
static void tinox_zip_wr32(unsigned char* p, uint32_t v) {
    p[0] = (unsigned char)(v & 0xFF);         p[1] = (unsigned char)((v >> 8) & 0xFF);
    p[2] = (unsigned char)((v >> 16) & 0xFF); p[3] = (unsigned char)((v >> 24) & 0xFF);
}

// Ganze Datei in einen Puffer lesen; NULL + *out_len=0 wenn nicht öffenbar.
static unsigned char* tinox_zip_read_file(const char* path, size_t* out_len) {
    *out_len = 0;
    FILE* f = fopen(path, "rb");
    if (!f) return NULL;
    if (fseek(f, 0, SEEK_END) != 0) { fclose(f); return NULL; }
    long sz = ftell(f);
    if (sz < 0) { fclose(f); return NULL; }
    rewind(f);
    unsigned char* buf = (unsigned char*)malloc((size_t)sz + 1);
    size_t rd = fread(buf, 1, (size_t)sz, f);
    fclose(f);
    buf[rd] = 0;
    *out_len = rd;
    return buf;
}

// Alle STORED-Einträge parsen. Rückgabe = Anzahl, *out = Array (GC-alloziert).
static int tinox_zip_parse(const char* path, TinoxZipMember** out) {
    *out = NULL;
    size_t len;
    unsigned char* buf = tinox_zip_read_file(path, &len);
    if (!buf || len < 4) return 0;

    TinoxZipMember* mem = NULL;
    int n = 0, cap = 0;
    size_t pos = 0;
    while (pos + 30 <= len && tinox_zip_rd32(buf + pos) == 0x04034b50u) {
        uint16_t method = tinox_zip_rd16(buf + pos + 8);
        uint32_t csize  = tinox_zip_rd32(buf + pos + 18);
        uint32_t usize  = tinox_zip_rd32(buf + pos + 22);
        uint16_t nlen   = tinox_zip_rd16(buf + pos + 26);
        uint16_t elen   = tinox_zip_rd16(buf + pos + 28);
        size_t name_off = pos + 30;
        size_t data_off = name_off + nlen + elen;
        if (data_off + csize > len) break;
        if (method == 0) {
            char* nm = (char*)malloc((size_t)nlen + 1);
            memcpy(nm, buf + name_off, nlen); nm[nlen] = 0;
            unsigned char* d = (unsigned char*)malloc((size_t)usize + 1);
            memcpy(d, buf + data_off, usize); d[usize] = 0;
            if (n == cap) {
                cap = cap ? cap * 2 : 8;
                mem = (TinoxZipMember*)realloc(mem, (size_t)cap * sizeof(TinoxZipMember));
            }
            mem[n].name = nm; mem[n].data = d; mem[n].size = usize;
            n++;
        }
        pos = data_off + csize;
    }
    *out = mem;
    return n;
}

// Einträge als gültige STORED-.zip schreiben.
static void tinox_zip_write(const char* path, TinoxZipMember* mem, int n) {
    FILE* f = fopen(path, "wb");
    if (!f) return;

    // Lokale Header + Daten; Offsets für Central Directory merken.
    uint32_t* offsets = (uint32_t*)malloc((size_t)(n > 0 ? n : 1) * sizeof(uint32_t));
    uint32_t* crcs    = (uint32_t*)malloc((size_t)(n > 0 ? n : 1) * sizeof(uint32_t));
    uint32_t cursor = 0;
    unsigned char lh[30];
    for (int i = 0; i < n; i++) {
        uint16_t nlen = (uint16_t)strlen(mem[i].name);
        uint32_t crc  = tinox_zip_crc32(mem[i].data, mem[i].size);
        offsets[i] = cursor;
        crcs[i]    = crc;
        memset(lh, 0, sizeof(lh));
        tinox_zip_wr32(lh + 0, 0x04034b50u);      // local file header signature
        tinox_zip_wr16(lh + 4, 20);               // version needed
        tinox_zip_wr16(lh + 6, 0);                // flags
        tinox_zip_wr16(lh + 8, 0);                // method: STORED
        tinox_zip_wr16(lh + 10, 0);               // mod time
        tinox_zip_wr16(lh + 12, 0x21);            // mod date (1980-01-01)
        tinox_zip_wr32(lh + 14, crc);             // crc-32
        tinox_zip_wr32(lh + 18, mem[i].size);     // compressed size
        tinox_zip_wr32(lh + 22, mem[i].size);     // uncompressed size
        tinox_zip_wr16(lh + 26, nlen);            // name length
        tinox_zip_wr16(lh + 28, 0);               // extra length
        fwrite(lh, 1, sizeof(lh), f);
        fwrite(mem[i].name, 1, nlen, f);
        fwrite(mem[i].data, 1, mem[i].size, f);
        cursor += (uint32_t)sizeof(lh) + nlen + mem[i].size;
    }

    // Central Directory.
    uint32_t cd_start = cursor;
    unsigned char ch[46];
    for (int i = 0; i < n; i++) {
        uint16_t nlen = (uint16_t)strlen(mem[i].name);
        memset(ch, 0, sizeof(ch));
        tinox_zip_wr32(ch + 0, 0x02014b50u);      // central dir signature
        tinox_zip_wr16(ch + 4, 20);               // version made by
        tinox_zip_wr16(ch + 6, 20);               // version needed
        tinox_zip_wr16(ch + 8, 0);                // flags
        tinox_zip_wr16(ch + 10, 0);               // method: STORED
        tinox_zip_wr16(ch + 12, 0);               // mod time
        tinox_zip_wr16(ch + 14, 0x21);            // mod date
        tinox_zip_wr32(ch + 16, crcs[i]);         // crc-32
        tinox_zip_wr32(ch + 20, mem[i].size);     // compressed size
        tinox_zip_wr32(ch + 24, mem[i].size);     // uncompressed size
        tinox_zip_wr16(ch + 28, nlen);            // name length
        tinox_zip_wr16(ch + 30, 0);               // extra length
        tinox_zip_wr16(ch + 32, 0);               // comment length
        tinox_zip_wr16(ch + 34, 0);               // disk number start
        tinox_zip_wr16(ch + 36, 0);               // internal attrs
        tinox_zip_wr32(ch + 38, 0);               // external attrs
        tinox_zip_wr32(ch + 42, offsets[i]);      // local header offset
        fwrite(ch, 1, sizeof(ch), f);
        fwrite(mem[i].name, 1, nlen, f);
        cursor += (uint32_t)sizeof(ch) + nlen;
    }
    uint32_t cd_size = cursor - cd_start;

    // End Of Central Directory.
    unsigned char eocd[22];
    memset(eocd, 0, sizeof(eocd));
    tinox_zip_wr32(eocd + 0, 0x06054b50u);        // EOCD signature
    tinox_zip_wr16(eocd + 8, (uint16_t)n);        // entries this disk
    tinox_zip_wr16(eocd + 10, (uint16_t)n);       // total entries
    tinox_zip_wr32(eocd + 12, cd_size);           // central dir size
    tinox_zip_wr32(eocd + 16, cd_start);          // central dir offset
    fwrite(eocd, 1, sizeof(eocd), f);

    fclose(f);
}

int64_t zipEntryCount(const char* path) {
    TinoxZipMember* mem;
    int n = tinox_zip_parse(path, &mem);
    return (int64_t)n;
}

char* zipEntryName(const char* path, int64_t idx) {
    TinoxZipMember* mem;
    int n = tinox_zip_parse(path, &mem);
    if (idx < 0 || idx >= n) return GC_strdup("");
    return GC_strdup(mem[idx].name);
}

int64_t zipEntrySize(const char* path, int64_t idx) {
    TinoxZipMember* mem;
    int n = tinox_zip_parse(path, &mem);
    if (idx < 0 || idx >= n) return 0;
    return (int64_t)mem[idx].size;
}

// Inhalt eines Eintrags als String; "" wenn nicht gefunden.
char* zipExtractFile(const char* path, const char* name) {
    TinoxZipMember* mem;
    int n = tinox_zip_parse(path, &mem);
    for (int i = 0; i < n; i++) {
        if (strcmp(mem[i].name, name) == 0)
            return GC_strdup((const char*)mem[i].data);
    }
    return GC_strdup("");
}

// Datei hinzufügen/ersetzen (legt die .zip bei Bedarf an).
void zipAddFile(const char* path, const char* name, const char* content) {
    TinoxZipMember* old;
    int n = tinox_zip_parse(path, &old);
    TinoxZipMember* mem = (TinoxZipMember*)malloc((size_t)(n + 1) * sizeof(TinoxZipMember));
    int m = 0;
    for (int i = 0; i < n; i++) {
        if (strcmp(old[i].name, name) == 0) continue; // ersetzen
        mem[m++] = old[i];
    }
    mem[m].name = (char*)name;
    mem[m].data = (unsigned char*)content;
    mem[m].size = (uint32_t)strlen(content);
    m++;
    tinox_zip_write(path, mem, m);
}

// Datei entfernen (kein Fehler, wenn nicht vorhanden).
void zipRemoveFile(const char* path, const char* name) {
    TinoxZipMember* old;
    int n = tinox_zip_parse(path, &old);
    TinoxZipMember* mem = (TinoxZipMember*)malloc((size_t)(n > 0 ? n : 1) * sizeof(TinoxZipMember));
    int m = 0;
    for (int i = 0; i < n; i++) {
        if (strcmp(old[i].name, name) == 0) continue;
        mem[m++] = old[i];
    }
    tinox_zip_write(path, mem, m);
}

// ---- Process / Environment builtins ----

// Forward declarations for CLI argument globals (defined later in this file)
extern int    _tinox_argc;
extern char** _tinox_argv;

void processExit(int64_t code) {
    exit((int)code);
}

int64_t* processArgs(void) {
    // Returns a Tinox array handle of arg strings as i64 (ptrtoint)
    int64_t n = (int64_t)_tinox_argc;
    int64_t* nh = tinox_array_new(n, 0);
    int64_t* data = ((TinoxArray*)nh)->data;
    for (int64_t i = 0; i < n; i++) {
        data[i] = (int64_t)_tinox_argv[i];
    }
    return nh;
}

int64_t processId(void) {
    return (int64_t)getpid();
}

static void tinox_random_seed_once(void) {
    static int seeded = 0;
    if (!seeded) {
        srandom((unsigned int)(time(NULL) ^ getpid()));
        seeded = 1;
    }
}

// [min, max) — matches the tinox.core.random Random class convention.
int64_t randomInt(int64_t min, int64_t max) {
    tinox_random_seed_once();
    if (max <= min) return min;
    return min + (int64_t)(random() % (max - min));
}

double randomFloat(void) {
    tinox_random_seed_once();
    // random() returns [0, 2^31-1] per POSIX, independent of RAND_MAX.
    return (double)random() / 2147483648.0;
}

void gcCollect(void) {
    GC_gcollect();
}

int64_t memoryUsage(void) {
    return (int64_t)GC_get_heap_size();
}

void printStackTrace(void) {
#ifdef __GLIBC__
    void* frames[64];
    int n = backtrace(frames, 64);
    backtrace_symbols_fd(frames, n, fileno(stderr));
#else
    fprintf(stderr, "<stack trace unavailable on this platform>\n");
#endif
}

char* envGet(const char* name) {
    char* v = getenv(name);
    return v ? GC_strdup(v) : GC_strdup("");
}

void envSet(const char* name, const char* value) {
    setenv(name, value, 1);
}

void envRemove(const char* name) {
    unsetenv(name);
}

char* envCurrentDir(void) {
    char buf[4096];
    if (getcwd(buf, sizeof(buf))) return GC_strdup(buf);
    return GC_strdup("");
}

void envSetCurrentDir(const char* path) {
    chdir(path);
}

// ---- Directory builtins ----

#include <dirent.h>
#include <sys/stat.h>

char* dirList(const char* path) {
    // Returns a Tinox array handle of filename strings
    int64_t* nh = tinox_array_new(0, 32);
    DIR* d = opendir(path);
    if (!d) return (char*)nh;
    struct dirent* ent;
    while ((ent = readdir(d)) != NULL) {
        if (strcmp(ent->d_name, ".") == 0 || strcmp(ent->d_name, "..") == 0) continue;
        tinox_array_push(nh, (int64_t)GC_strdup(ent->d_name));
    }
    closedir(d);
    return (char*)nh;
}

void dirCreate(const char* path) {
    mkdir(path, 0755);
}

void dirDelete(const char* path) {
    rmdir(path);
}

// ---- Crypto/hashing builtins (MD5, SHA-256, HMAC-SHA256) ----
// Self-contained (no OpenSSL dependency, matches this file's existing
// "no external libs unless opted in via tinox.toml" convention).

static const uint32_t md5_K[64] = {
    0xd76aa478,0xe8c7b756,0x242070db,0xc1bdceee,0xf57c0faf,0x4787c62a,0xa8304613,0xfd469501,
    0x698098d8,0x8b44f7af,0xffff5bb1,0x895cd7be,0x6b901122,0xfd987193,0xa679438e,0x49b40821,
    0xf61e2562,0xc040b340,0x265e5a51,0xe9b6c7aa,0xd62f105d,0x02441453,0xd8a1e681,0xe7d3fbc8,
    0x21e1cde6,0xc33707d6,0xf4d50d87,0x455a14ed,0xa9e3e905,0xfcefa3f8,0x676f02d9,0x8d2a4c8a,
    0xfffa3942,0x8771f681,0x6d9d6122,0xfde5380c,0xa4beea44,0x4bdecfa9,0xf6bb4b60,0xbebfbc70,
    0x289b7ec6,0xeaa127fa,0xd4ef3085,0x04881d05,0xd9d4d039,0xe6db99e5,0x1fa27cf8,0xc4ac5665,
    0xf4292244,0x432aff97,0xab9423a7,0xfc93a039,0x655b59c3,0x8f0ccc92,0xffeff47d,0x85845dd1,
    0x6fa87e4f,0xfe2ce6e0,0xa3014314,0x4e0811a1,0xf7537e82,0xbd3af235,0x2ad7d2bb,0xeb86d391
};
static const int md5_S[64] = {
    7,12,17,22, 7,12,17,22, 7,12,17,22, 7,12,17,22,
    5, 9,14,20, 5, 9,14,20, 5, 9,14,20, 5, 9,14,20,
    4,11,16,23, 4,11,16,23, 4,11,16,23, 4,11,16,23,
    6,10,15,21, 6,10,15,21, 6,10,15,21, 6,10,15,21
};

static uint32_t md5_rotl(uint32_t x, int c) { return (x << c) | (x >> (32 - c)); }

static void md5_transform(uint32_t state[4], const unsigned char block[64]) {
    uint32_t a = state[0], b = state[1], c = state[2], d = state[3];
    uint32_t m[16];
    for (int i = 0; i < 16; i++) {
        m[i] = (uint32_t)block[i*4] | ((uint32_t)block[i*4+1] << 8) |
               ((uint32_t)block[i*4+2] << 16) | ((uint32_t)block[i*4+3] << 24);
    }
    for (int i = 0; i < 64; i++) {
        uint32_t f; int g;
        if (i < 16) { f = (b & c) | (~b & d); g = i; }
        else if (i < 32) { f = (d & b) | (~d & c); g = (5*i + 1) % 16; }
        else if (i < 48) { f = b ^ c ^ d; g = (3*i + 5) % 16; }
        else { f = c ^ (b | ~d); g = (7*i) % 16; }
        uint32_t temp = d;
        d = c;
        c = b;
        b = b + md5_rotl(a + f + md5_K[i] + m[g], md5_S[i]);
        a = temp;
    }
    state[0] += a; state[1] += b; state[2] += c; state[3] += d;
}

static void md5_raw(const unsigned char* data, size_t len, unsigned char out[16]) {
    uint32_t state[4] = {0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476};
    uint64_t bitlen = (uint64_t)len * 8;
    size_t padded_len = ((len + 8) / 64 + 1) * 64;
    unsigned char* msg = (unsigned char*)calloc(1, padded_len);
    memcpy(msg, data, len);
    msg[len] = 0x80;
    for (int i = 0; i < 8; i++) {
        msg[padded_len - 8 + i] = (unsigned char)(bitlen >> (8*i)); // little-endian length
    }
    for (size_t off = 0; off < padded_len; off += 64) {
        md5_transform(state, msg + off);
    }
    free(msg);
    for (int i = 0; i < 4; i++) {
        out[i*4]   = (unsigned char)(state[i]);
        out[i*4+1] = (unsigned char)(state[i] >> 8);
        out[i*4+2] = (unsigned char)(state[i] >> 16);
        out[i*4+3] = (unsigned char)(state[i] >> 24);
    }
}

static const uint32_t sha256_K[64] = {
    0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
    0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
    0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
    0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
    0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
    0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
    0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
    0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2
};

static uint32_t sha256_rotr(uint32_t x, int n) { return (x >> n) | (x << (32 - n)); }

static void sha256_transform(uint32_t state[8], const unsigned char block[64]) {
    uint32_t w[64];
    for (int i = 0; i < 16; i++) {
        w[i] = ((uint32_t)block[i*4] << 24) | ((uint32_t)block[i*4+1] << 16) |
               ((uint32_t)block[i*4+2] << 8) | (uint32_t)block[i*4+3];
    }
    for (int i = 16; i < 64; i++) {
        uint32_t s0 = sha256_rotr(w[i-15], 7) ^ sha256_rotr(w[i-15], 18) ^ (w[i-15] >> 3);
        uint32_t s1 = sha256_rotr(w[i-2], 17) ^ sha256_rotr(w[i-2], 19) ^ (w[i-2] >> 10);
        w[i] = w[i-16] + s0 + w[i-7] + s1;
    }
    uint32_t a=state[0],b=state[1],c=state[2],d=state[3],e=state[4],f=state[5],g=state[6],h=state[7];
    for (int i = 0; i < 64; i++) {
        uint32_t s1 = sha256_rotr(e,6) ^ sha256_rotr(e,11) ^ sha256_rotr(e,25);
        uint32_t ch = (e & f) ^ (~e & g);
        uint32_t temp1 = h + s1 + ch + sha256_K[i] + w[i];
        uint32_t s0 = sha256_rotr(a,2) ^ sha256_rotr(a,13) ^ sha256_rotr(a,22);
        uint32_t maj = (a & b) ^ (a & c) ^ (b & c);
        uint32_t temp2 = s0 + maj;
        h=g; g=f; f=e; e=d+temp1; d=c; c=b; b=a; a=temp1+temp2;
    }
    state[0]+=a; state[1]+=b; state[2]+=c; state[3]+=d;
    state[4]+=e; state[5]+=f; state[6]+=g; state[7]+=h;
}

static void sha256_raw(const unsigned char* data, size_t len, unsigned char out[32]) {
    uint32_t state[8] = {
        0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,
        0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19
    };
    uint64_t bitlen = (uint64_t)len * 8;
    size_t padded_len = ((len + 8) / 64 + 1) * 64;
    unsigned char* msg = (unsigned char*)calloc(1, padded_len);
    memcpy(msg, data, len);
    msg[len] = 0x80;
    for (int i = 0; i < 8; i++) {
        msg[padded_len - 1 - i] = (unsigned char)(bitlen >> (8*i)); // big-endian length
    }
    for (size_t off = 0; off < padded_len; off += 64) {
        sha256_transform(state, msg + off);
    }
    free(msg);
    for (int i = 0; i < 8; i++) {
        out[i*4]   = (unsigned char)(state[i] >> 24);
        out[i*4+1] = (unsigned char)(state[i] >> 16);
        out[i*4+2] = (unsigned char)(state[i] >> 8);
        out[i*4+3] = (unsigned char)(state[i]);
    }
}

static char* tinox_bytes_to_hex(const unsigned char* bytes, size_t n) {
    char* hex = (char*)GC_malloc(n*2 + 1);
    for (size_t i = 0; i < n; i++) {
        snprintf(hex + i*2, 3, "%02x", bytes[i]);
    }
    return hex;
}

char* md5Hash(const char* data) {
    unsigned char out[16];
    md5_raw((const unsigned char*)data, strlen(data), out);
    return tinox_bytes_to_hex(out, 16);
}

char* sha256Hash(const char* data) {
    unsigned char out[32];
    sha256_raw((const unsigned char*)data, strlen(data), out);
    return tinox_bytes_to_hex(out, 32);
}

// RFC 2104
char* hmacSha256Hash(const char* data, const char* key) {
    unsigned char key_block[64];
    size_t key_len = strlen(key);
    memset(key_block, 0, 64);
    if (key_len > 64) {
        unsigned char key_hash[32];
        sha256_raw((const unsigned char*)key, key_len, key_hash);
        memcpy(key_block, key_hash, 32);
    } else {
        memcpy(key_block, key, key_len);
    }

    unsigned char o_pad[64], i_pad[64];
    for (int i = 0; i < 64; i++) {
        o_pad[i] = (unsigned char)(key_block[i] ^ 0x5c);
        i_pad[i] = (unsigned char)(key_block[i] ^ 0x36);
    }

    size_t data_len = strlen(data);
    unsigned char* inner_msg = (unsigned char*)malloc(64 + data_len);
    memcpy(inner_msg, i_pad, 64);
    memcpy(inner_msg + 64, data, data_len);
    unsigned char inner_hash[32];
    sha256_raw(inner_msg, 64 + data_len, inner_hash);
    free(inner_msg);

    unsigned char outer_msg[96];
    memcpy(outer_msg, o_pad, 64);
    memcpy(outer_msg + 64, inner_hash, 32);
    unsigned char final_hash[32];
    sha256_raw(outer_msg, 96, final_hash);

    return tinox_bytes_to_hex(final_hash, 32);
}

// ---- Regex builtins ----

#include <regex.h>

int64_t regexIsMatch(int64_t pattern_i64, int64_t subject_i64) {
    const char* pattern = (const char*)pattern_i64;
    const char* subject = (const char*)subject_i64;
    regex_t re;
    if (regcomp(&re, pattern, REG_EXTENDED) != 0) return 0;
    int r = regexec(&re, subject, 0, NULL, 0);
    regfree(&re);
    return (r == 0) ? 1 : 0;
}

int64_t regexFindAll(int64_t pattern_i64, int64_t subject_i64) {
    const char* pattern = (const char*)pattern_i64;
    const char* subject = (const char*)subject_i64;
    int64_t* nh = tinox_array_new(0, 8);
    regex_t re;
    if (regcomp(&re, pattern, REG_EXTENDED) != 0) return (int64_t)nh;
    const char* s = subject;
    regmatch_t m;
    while (*s && regexec(&re, s, 1, &m, 0) == 0) {
        int mlen = m.rm_eo - m.rm_so;
        char* match_str = (char*)GC_malloc(mlen + 1);
        memcpy(match_str, s + m.rm_so, mlen);
        match_str[mlen] = '\0';
        tinox_array_push(nh, (int64_t)match_str);
        s += m.rm_eo;
        if (m.rm_eo == 0) s++;
    }
    regfree(&re);
    return (int64_t)nh;
}

int64_t regexReplace(int64_t pattern_i64, int64_t subject_i64, int64_t replacement_i64) {
    const char* pattern = (const char*)pattern_i64;
    const char* subject = (const char*)subject_i64;
    const char* replacement = (const char*)replacement_i64;
    // Simple replacement — replace first match
    regex_t re;
    if (regcomp(&re, pattern, REG_EXTENDED) != 0) return subject_i64;
    regmatch_t m;
    if (regexec(&re, subject, 1, &m, 0) != 0) { regfree(&re); return subject_i64; }
    size_t pre = m.rm_so, rep_len = strlen(replacement), suf = strlen(subject) - m.rm_eo;
    char* result = (char*)GC_malloc(pre + rep_len + suf + 1);
    memcpy(result, subject, pre);
    memcpy(result + pre, replacement, rep_len);
    memcpy(result + pre + rep_len, subject + m.rm_eo, suf);
    result[pre + rep_len + suf] = '\0';
    regfree(&re);
    return (int64_t)result;
}

int64_t regexSplit(int64_t pattern_i64, int64_t subject_i64) {
    return regexFindAll(pattern_i64, subject_i64); // simplified
}

// First match, or "" if none / bad pattern.
int64_t regexFindFirst(int64_t pattern_i64, int64_t subject_i64) {
    const char* pattern = (const char*)pattern_i64;
    const char* subject = (const char*)subject_i64;
    regex_t re;
    if (regcomp(&re, pattern, REG_EXTENDED) != 0) return (int64_t)GC_strdup("");
    regmatch_t m;
    if (regexec(&re, subject, 1, &m, 0) != 0) {
        regfree(&re);
        return (int64_t)GC_strdup("");
    }
    int mlen = m.rm_eo - m.rm_so;
    char* match_str = (char*)GC_malloc(mlen + 1);
    memcpy(match_str, subject + m.rm_so, mlen);
    match_str[mlen] = '\0';
    regfree(&re);
    return (int64_t)match_str;
}

// Replace every non-overlapping match of `pattern` in `subject` with
// `replacement` (literal, no backreferences — same as regexReplace).
int64_t regexReplaceAll(int64_t pattern_i64, int64_t subject_i64, int64_t replacement_i64) {
    const char* pattern = (const char*)pattern_i64;
    const char* subject = (const char*)subject_i64;
    const char* replacement = (const char*)replacement_i64;
    regex_t re;
    if (regcomp(&re, pattern, REG_EXTENDED) != 0) return subject_i64;

    size_t rep_len = strlen(replacement);
    size_t cap = strlen(subject) + rep_len + 16;
    char* result = (char*)GC_malloc(cap);
    size_t out = 0;
    const char* s = subject;
    regmatch_t m;
    while (*s && regexec(&re, s, 1, &m, 0) == 0) {
        size_t pre = (size_t)m.rm_so;
        size_t needed = out + pre + rep_len + 1;
        if (needed > cap) {
            cap = needed * 2;
            char* grown = (char*)GC_malloc(cap);
            memcpy(grown, result, out);
            result = grown;
        }
        memcpy(result + out, s, pre);
        out += pre;
        memcpy(result + out, replacement, rep_len);
        out += rep_len;
        size_t adv = (size_t)m.rm_eo;
        if (adv == 0) {
            // Empty match — copy one char to avoid an infinite loop.
            if (s[0] == '\0') break;
            size_t needed2 = out + 2;
            if (needed2 > cap) {
                cap = needed2 * 2;
                char* grown = (char*)GC_malloc(cap);
                memcpy(grown, result, out);
                result = grown;
            }
            result[out++] = s[0];
            adv = 1;
        }
        s += adv;
    }
    size_t tail = strlen(s);
    size_t needed = out + tail + 1;
    if (needed > cap) {
        cap = needed;
        char* grown = (char*)GC_malloc(cap);
        memcpy(grown, result, out);
        result = grown;
    }
    memcpy(result + out, s, tail);
    out += tail;
    result[out] = '\0';
    regfree(&re);
    return (int64_t)result;
}

// First match at/after byte offset. Returns Tinox i64-array
// [match_start, match_end, g1_start, g1_end, ...] (byte offsets into subject,
// -1/-1 for unmatched groups). Empty array = no match or bad pattern.
int64_t* regexMatchGroups(const char* pattern, const char* subject, int64_t offset, int64_t icase) {
    int64_t* empty = tinox_array_new(0, 0);

    size_t slen = strlen(subject);
    if (offset < 0 || (size_t)offset > slen) return empty;

    regex_t re;
    int cflags = REG_EXTENDED | (icase ? REG_ICASE : 0);
    if (regcomp(&re, pattern, cflags) != 0) return empty;

    size_t ngroups = re.re_nsub + 1;
    regmatch_t* m = (regmatch_t*)GC_malloc(sizeof(regmatch_t) * ngroups);
    int eflags = (offset > 0) ? REG_NOTBOL : 0;
    if (regexec(&re, subject + offset, ngroups, m, eflags) != 0) {
        regfree(&re);
        return empty;
    }
    regfree(&re);

    int64_t len = (int64_t)(ngroups * 2);
    int64_t* nh = tinox_array_new(len, 0);
    int64_t* data = ((TinoxArray*)nh)->data;
    for (size_t g = 0; g < ngroups; g++) {
        if (m[g].rm_so < 0) {
            data[g * 2] = -1;
            data[g * 2 + 1] = -1;
        } else {
            data[g * 2] = (int64_t)m[g].rm_so + offset;
            data[g * 2 + 1] = (int64_t)m[g].rm_eo + offset;
        }
    }
    return nh;
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
    TINOX_CK_REG(m, TINOX_KIND_MAP);
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
        int64_t* hkeys_h = tinox_map_keys(resp_hdr_map);
        int64_t* hkeys = ((TinoxArray*)hkeys_h)->data;
        int64_t hklen = ((TinoxArray*)hkeys_h)->len;
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
    TINOX_CK_REG(m, TINOX_KIND_MAP);
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
                int64_t* keys_h = tinox_map_keys(v->obj_val);
                int64_t* keys = ((TinoxArray*)keys_h)->data;
                int64_t klen = ((TinoxArray*)keys_h)->len;
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

// Wrap an existing Map<String, JsonValue> handle as a JsonValue object.
// TinoxMap has the same layout whether allocated via tinox_map_create()
// (GC heap, used by user Map<K,V> values) or json_obj_map_create() (the
// parser's per-call arena) — safe to reuse the caller's map directly.
// Heap-allocated (not arena), so — unlike parser-produced JsonValues —
// this one survives past the next jsonParse() call.
int64_t* jsonFromMap(void* map) {
    TinoxJsonValue* v = (TinoxJsonValue*)malloc(sizeof(TinoxJsonValue));
    v->type = JSON_OBJECT;
    v->obj_val = map;
    return (int64_t*)v;
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

int64_t* jsonIntArrayFromJson(int64_t* json_array) {
    TinoxJsonValue* v = (TinoxJsonValue*)json_array;
    if (!v) return tinox_array_new(0, 0);
    // Fast-path: pure int array — copy the arena data
    // (internal JSON arrays keep the arena layout with len at arr_val[-1])
    if (v->type == JSON_INT_ARRAY) {
        int64_t len = v->arr_val ? v->arr_val[-1] : 0;
        int64_t* nh = tinox_array_new(len, 0);
        if (len > 0) memcpy(((TinoxArray*)nh)->data, v->arr_val, (size_t)len * sizeof(int64_t));
        return nh;
    }
    // Generic JSON_ARRAY path
    int64_t len = (v->type == JSON_ARRAY && v->arr_val) ? v->arr_val[-1] : 0;
    int64_t* nh = tinox_array_new(len, 0);
    int64_t* buf = ((TinoxArray*)nh)->data;
    for (int64_t i = 0; i < len; i++) {
        TinoxJsonValue* elem = (TinoxJsonValue*)(uintptr_t)v->arr_val[i];
        if (elem) {
            if      (elem->type == JSON_INT)   buf[i] = elem->int_val;
            else if (elem->type == JSON_FLOAT) buf[i] = (int64_t)elem->float_val;
            else                               buf[i] = 0;
        } else {
            buf[i] = 0;
        }
    }
    return nh;
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
char* jsonIntArrayWrap(const char* key, int64_t* h) {
    TinoxArray* a = (TinoxArray*)h;
    int64_t len = a ? a->len : 0;
    const int64_t* arr = a ? a->data : NULL;
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

char* jsonIntArrayToString(int64_t* h) {
    if (!h) return strdup("[]");
    TinoxArray* a = (TinoxArray*)h;
    int64_t len = a->len;
    const int64_t* arr = a->data;
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

void jsonBuilderAddIntList(char* handle, const char* key, int64_t* h) {
    JsonBuilder* b = (JsonBuilder*)handle;
    jb_key(b, key);
    TinoxArray* a = (TinoxArray*)h;
    int64_t len = a ? a->len : 0;
    const int64_t* arr = a ? a->data : NULL;
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

int    _tinox_argc = 0;
char** _tinox_argv = NULL;

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

// Float classification and constants
int64_t mathIsNan(double x) { return isnan(x) ? 1 : 0; }
int64_t mathIsInfinite(double x) { return isinf(x) ? 1 : 0; }
int64_t mathIsNormal(double x) { return isnormal(x) ? 1 : 0; }
double mathNan(void) { return NAN; }
double mathInf(void) { return INFINITY; }

// Env listing
char* envDump(void) {
    extern char** environ;
    size_t total = 1;
    for (int i = 0; environ[i]; i++) total += strlen(environ[i]) + 1;
    char* buf = GC_malloc(total);
    char* p = buf;
    for (int i = 0; environ[i]; i++) {
        size_t len = strlen(environ[i]);
        memcpy(p, environ[i], len); p[len] = '\n'; p += len + 1;
    }
    *p = '\0';
    return buf;
}

// Time
int64_t currentTimeSecs(void) { return (int64_t)time(NULL); }

int64_t now(void) {
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    return (int64_t)ts.tv_sec * 1000LL + ts.tv_nsec / 1000000LL;
}

void sleep_ms(int64_t ms) {
    struct timespec ts;
    ts.tv_sec = ms / 1000;
    ts.tv_nsec = (ms % 1000) * 1000000LL;
    nanosleep(&ts, NULL);
}

char* strftimeStr(const char* fmt, int64_t t) {
    time_t ts = (time_t)t;
    struct tm tm_buf;
    gmtime_r(&ts, &tm_buf);
    char buf[256];
    strftime(buf, sizeof(buf), fmt, &tm_buf);
    return GC_strdup(buf);
}

int64_t fromdateStr(const char* s) {
    struct tm tm_buf = {0};
    // Try "%Y-%m-%dT%H:%M:%SZ" and "%Y-%m-%dT%H:%M:%S"
    char* r = strptime(s, "%Y-%m-%dT%H:%M:%SZ", &tm_buf);
    if (!r) r = strptime(s, "%Y-%m-%dT%H:%M:%S", &tm_buf);
    if (!r) r = strptime(s, "%Y-%m-%d", &tm_buf);
    if (!r) return 0;
    return (int64_t)timegm(&tm_buf);
}

void printStderr(const char* msg) { fputs(msg, stderr); fputc('\n', stderr); }

int64_t isStdinTty(void) { return isatty(STDIN_FILENO) ? 1 : 0; }

int64_t isStdoutTty(void) { return isatty(STDOUT_FILENO) ? 1 : 0; }
