// Tinox Runtime

#include <stdio.h>
#include <stdlib.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#include <math.h>
#include <pthread.h>
#include <unistd.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>

// Memory allocation
void* tinox_alloc(size_t size) {
    return malloc(size);
}

void tinox_free(void* ptr) {
    free(ptr);
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
    size_t len_a = 0;
    while (a[len_a] != '\0') len_a++;
    size_t len_b = 0;
    while (b[len_b] != '\0') len_b++;
    
    char* result = malloc(len_a + len_b + 1);
    for (size_t i = 0; i < len_a; i++) result[i] = a[i];
    for (size_t i = 0; i < len_b; i++) result[len_a + i] = b[i];
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

int64_t* tinox_array_sort(int64_t* data) {
    int64_t len = data[-1];
    int64_t* raw = malloc((len + 1) * sizeof(int64_t));
    raw[0] = len;
    int64_t* nd = raw + 1;
    for (int64_t i = 0; i < len; i++) nd[i] = data[i];
    qsort(nd, (size_t)len, sizeof(int64_t), cmp_i64);
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
} TinoxMap;

static size_t map_hash(const char* key, size_t cap) {
    size_t h = 14695981039346656037ULL;
    for (const unsigned char* p = (const unsigned char*)key; *p; p++)
        h = (h ^ *p) * 1099511628211ULL;
    return h & (cap - 1);
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
    free(m->entries);
    m->entries = new_entries;
    m->cap     = new_cap;
}

void* tinox_map_create(void) {
    TinoxMap* m = malloc(sizeof(TinoxMap));
    m->cap     = TINOX_MAP_INIT_CAP;
    m->len     = 0;
    m->entries = calloc(m->cap, sizeof(TinoxMapEntry));
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
            m->entries[idx].key   = strdup(key);
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
    for (size_t i = 0; i < m->cap; i++) {
        char* k = m->entries[i].key;
        if (k && k != (char*)1) free(k);
    }
    free(m->entries);
    free(m);
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

// ---- HTTP Server ----

int64_t httpServerCreate(int64_t port) {
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) return -1;
    int opt = 1;
    setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt));
    struct sockaddr_in addr = {0};
    addr.sin_family      = AF_INET;
    addr.sin_addr.s_addr = INADDR_ANY;
    addr.sin_port        = htons((uint16_t)port);
    if (bind(fd, (struct sockaddr*)&addr, sizeof(addr)) < 0) { close(fd); return -1; }
    if (listen(fd, 128) < 0) { close(fd); return -1; }
    return (int64_t)fd;
}

int64_t httpServerAcceptConn(int64_t server_fd) {
    struct sockaddr_in client = {0};
    socklen_t len = sizeof(client);
    int fd = accept((int)server_fd, (struct sockaddr*)&client, &len);
    return (int64_t)fd;
}

// Reads a full HTTP/1.1 request from the socket and returns it as a heap string.
char* httpServerReadRequest(int64_t client_fd) {
    size_t cap = 4096, used = 0;
    char* buf = (char*)malloc(cap);
    int fd = (int)client_fd;
    while (1) {
        if (used + 1 >= cap) { cap *= 2; buf = (char*)realloc(buf, cap); }
        ssize_t n = recv(fd, buf + used, cap - used - 1, 0);
        if (n <= 0) break;
        used += (size_t)n;
        buf[used] = '\0';
        // Stop once we have the full headers (and body if Content-Length matches)
        char* hdr_end = strstr(buf, "\r\n\r\n");
        if (!hdr_end) continue;
        // Check for Content-Length to read body
        char* cl = strcasestr(buf, "Content-Length:");
        if (cl) {
            long body_len = atol(cl + 15);
            long header_len = (long)(hdr_end - buf) + 4;
            long total = header_len + body_len;
            while ((long)used < total) {
                if (used + 1 >= cap) { cap *= 2; buf = (char*)realloc(buf, cap); }
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
#define TINOX_MAX_BODY   65536

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

void tinox_HttpServer_listen(int64_t* server) {
    TinoxHttpServer* srv = (TinoxHttpServer*)server;
    int64_t server_fd = httpServerCreate(srv->port);
    if (server_fd < 0) {
        fprintf(stderr, "HttpServer: failed to bind port %lld\n", (long long)srv->port);
        return;
    }
    fprintf(stderr, "HttpServer listening on port %lld\n", (long long)srv->port);

    while (1) {
        int64_t client_fd = httpServerAcceptConn(server_fd);
        if (client_fd < 0) continue;

        char* raw_req = httpServerReadRequest(client_fd);
        if (!raw_req) { httpServerCloseConn(client_fd); continue; }

        char method[8] = {0};
        char path[256] = {0};
        char query[512] = {0};
        sscanf(raw_req, "%7s %255s", method, path);
        // Split path and query string
        char* qmark = strchr(path, '?');
        if (qmark) { strncpy(query, qmark + 1, 511); *qmark = '\0'; }

        // Find matching route
        TinoxRouteHandler handler = NULL;
        for (int i = 0; i < srv->route_count; i++) {
            if (strcmp(srv->routes[i].method, method) == 0 &&
                strcmp(srv->routes[i].path, path) == 0) {
                handler = srv->routes[i].handler;
                break;
            }
        }

        // Parse HTTP headers from raw request into a map
        void* req_headers = tinox_map_create();
        char* hdr_line = strchr(raw_req, '\n');
        while (hdr_line) {
            hdr_line++;
            if (*hdr_line == '\r' || *hdr_line == '\n' || *hdr_line == '\0') break;
            char* colon = strchr(hdr_line, ':');
            char* eol = strchr(hdr_line, '\n');
            if (colon && eol && colon < eol) {
                size_t klen = (size_t)(colon - hdr_line);
                char* hkey = (char*)tinox_alloc(klen + 1);
                memcpy(hkey, hdr_line, klen);
                hkey[klen] = '\0';
                char* vstart = colon + 1;
                while (*vstart == ' ') vstart++;
                size_t vlen = (size_t)(eol - vstart);
                while (vlen > 0 && (vstart[vlen-1] == '\r' || vstart[vlen-1] == ' ')) vlen--;
                char* hval = (char*)tinox_alloc(vlen + 1);
                memcpy(hval, vstart, vlen);
                hval[vlen] = '\0';
                tinox_map_set(req_headers, hkey, (int64_t)hval);
            }
            hdr_line = eol;
        }

        // Extract body (after blank line)
        char* req_body = "";
        char* body_start = strstr(raw_req, "\r\n\r\n");
        if (!body_start) body_start = strstr(raw_req, "\n\n");
        if (body_start) {
            body_start += (body_start[0] == '\r') ? 4 : 2;
            req_body = body_start;
        }

        // Build HttpResponse struct: [statusCode, headers_ptr, body_ptr]
        int64_t* response = (int64_t*)tinox_alloc(3 * sizeof(int64_t));
        char* empty_body = (char*)tinox_alloc(1);
        empty_body[0] = '\0';
        response[0] = handler ? 200 : 404;
        response[1] = (int64_t)tinox_map_create();
        response[2] = (int64_t)empty_body;

        // Build HttpRequest struct: [method, path, body, headers, queryString, params]
        int64_t* request = (int64_t*)tinox_alloc(6 * sizeof(int64_t));
        request[0] = (int64_t)method;
        request[1] = (int64_t)path;
        request[2] = (int64_t)req_body;
        request[3] = (int64_t)req_headers;
        request[4] = (int64_t)query;
        request[5] = (int64_t)tinox_map_create();

        // Build HttpContext struct: [request_ptr, response_ptr]
        int64_t* ctx = (int64_t*)tinox_alloc(2 * sizeof(int64_t));
        ctx[0] = (int64_t)request;
        ctx[1] = (int64_t)response;

        if (handler) {
            handler((int64_t)ctx);
        }

        // Send HTTP response
        char* body = (char*)response[2];
        if (!body) body = "";
        int64_t status = response[0];
        char http_resp[TINOX_MAX_BODY + 256];
        snprintf(http_resp, sizeof(http_resp),
            "HTTP/1.1 %lld %s\r\n"
            "Content-Length: %zu\r\n"
            "Content-Type: application/json\r\n"
            "Connection: close\r\n"
            "\r\n"
            "%s",
            (long long)status, http_status_text(status), strlen(body), body);

        httpServerSendRaw(client_fd, http_resp);
        httpServerCloseConn(client_fd);
    }

    httpServerClose(server_fd);
}

// ---- Entry point ----

extern int64_t tinox_main(void);

int main(int argc, char** argv) {
    (void)argc;
    (void)argv;
    return (int)tinox_main();
}
