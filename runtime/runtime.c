// Tinox Runtime

#include <stdio.h>
#include <stdlib.h>
#include <stdbool.h>
#include <stdint.h>
#include <pthread.h>

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
    printf("%f", val);
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

// ---- Entry point ----

extern int64_t tinox_main(void);

int main(int argc, char** argv) {
    (void)argc;
    (void)argv;
    return (int)tinox_main();
}
