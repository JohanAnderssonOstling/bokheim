/* Sample userspace instruction pointers of all current app threads.
 * Build using the Kobo ARM compiler: -O2 -static -Wall -Wextra.
 * Usage: kobo-cpu-sample PID SECONDS [--callers]
 * Uses the kernel CPU clock at 500 Hz per active thread. No app restart.
 * Output: S monotonic_nanoseconds tid instruction_pointer_hex period_nanoseconds
 * This measures exclusive CPU samples, not inclusive wall-clock function time.
 */
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <linux/perf_event.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>

struct counter { int fd; unsigned char *map; uint64_t tail; };
static bool collect_callers;

#define USER_STACK_BYTES 512

static double seconds(void) {
    struct timespec value;
    clock_gettime(CLOCK_MONOTONIC, &value);
    return value.tv_sec + value.tv_nsec / 1e9;
}

static void ring_copy(unsigned char *ring, size_t size, uint64_t offset, void *output, size_t length) {
    size_t start = offset % size;
    size_t first = length < size - start ? length : size - start;
    memcpy(output, ring + start, first);
    if (first < length) memcpy((unsigned char *)output + first, ring, length - first);
}

static void sample_read(unsigned char *ring, size_t size, uint64_t record,
                        size_t record_size, size_t *cursor, void *output, size_t length) {
    if (*cursor > record_size || length > record_size - *cursor) {
        fprintf(stderr, "Truncated perf sample\n"); exit(1);
    }
    ring_copy(ring, size, record + *cursor, output, length);
    *cursor += length;
}

static void drain(struct counter *counter, size_t page) {
    struct perf_event_mmap_page *metadata = (void *)counter->map;
    uint64_t head = __atomic_load_n(&metadata->data_head, __ATOMIC_ACQUIRE);
    unsigned char *ring = counter->map + page;
    size_t size = page * 8;
    while (counter->tail < head) {
        struct perf_event_header header;
        ring_copy(ring, size, counter->tail, &header, sizeof(header));
        if (header.size < sizeof(header) || header.size > size) {
            fprintf(stderr, "Invalid perf ring record\n"); exit(1);
        }
        if (header.type == PERF_RECORD_SAMPLE) {
            struct { uint64_t ip; uint32_t pid; uint32_t tid; uint64_t time; uint64_t period; } sample;
            if (!collect_callers && header.size != sizeof(header) + sizeof(sample)) {
                fprintf(stderr, "Unexpected sample size %u\n", header.size); exit(1);
            }
            size_t cursor = sizeof(header);
            sample_read(ring, size, counter->tail, header.size, &cursor, &sample, sizeof(sample));
            printf("S %" PRIu64 " %u %" PRIx64 " %" PRIu64, sample.time, sample.tid, sample.ip, sample.period);
            if (collect_callers) {
                uint64_t count;
                sample_read(ring, size, counter->tail, header.size, &cursor, &count, sizeof(count));
                if (count > (header.size - cursor) / sizeof(uint64_t)) {
                    fprintf(stderr, "Invalid callchain length\n"); exit(1);
                }
                printf(" chain=");
                for (uint64_t i = 0; i < count; ++i) {
                    uint64_t address;
                    sample_read(ring, size, counter->tail, header.size, &cursor, &address, sizeof(address));
                    printf("%s%" PRIx64, i ? "," : "", address);
                }
                uint64_t abi;
                sample_read(ring, size, counter->tail, header.size, &cursor, &abi, sizeof(abi));
                printf(" abi=%" PRIu64, abi);
                if (abi) {
                    uint64_t registers[16];
                    sample_read(ring, size, counter->tail, header.size, &cursor, registers, sizeof(registers));
                    printf(" regs=");
                    for (size_t i = 0; i < 16; ++i) printf("%s%" PRIx64, i ? "," : "", registers[i]);
                }
                uint64_t stack_size;
                sample_read(ring, size, counter->tail, header.size, &cursor, &stack_size, sizeof(stack_size));
                if (stack_size) {
                    unsigned char stack[USER_STACK_BYTES];
                    if (stack_size > sizeof(stack)) { fprintf(stderr, "Unexpected stack size\n"); exit(1); }
                    sample_read(ring, size, counter->tail, header.size, &cursor, stack, (size_t) stack_size);
                    uint64_t valid_size;
                    sample_read(ring, size, counter->tail, header.size, &cursor, &valid_size, sizeof(valid_size));
                    if (valid_size > stack_size) { fprintf(stderr, "Invalid captured stack size\n"); exit(1); }
                    printf(" stack=");
                    for (size_t i = 0; i < valid_size; ++i) printf("%02x", stack[i]);
                }
            }
            putchar('\n');
        } else if (header.type == PERF_RECORD_LOST) {
            uint64_t lost[2];
            ring_copy(ring, size, counter->tail + sizeof(header), lost, sizeof(lost));
            printf("L %" PRIu64 "\n", lost[1]);
        }
        counter->tail += header.size;
    }
    __atomic_store_n(&metadata->data_tail, counter->tail, __ATOMIC_RELEASE);
}

int main(int argc, char **argv) {
    if ((argc != 3 && argc != 4) || atoi(argv[1]) <= 0 || !(atof(argv[2]) > 0 && atof(argv[2]) <= 600) ||
        (argc == 4 && strcmp(argv[3], "--callers"))) {
        fprintf(stderr, "Usage: %s PID SECONDS [--callers]\n", argv[0]); return 2;
    }
    collect_callers = argc == 4;
    char path[128];
    snprintf(path, sizeof(path), "/proc/%d/task", atoi(argv[1]));
    DIR *directory = opendir(path);
    if (!directory) { perror("task directory"); return 1; }
    struct counter counters[256];
    size_t count = 0, page = (size_t)sysconf(_SC_PAGESIZE);
    struct dirent *entry;
    while ((entry = readdir(directory))) {
        int tid = atoi(entry->d_name);
        if (!tid) continue;
        if (count == 256) { fprintf(stderr, "Too many threads\n"); return 1; }
        struct perf_event_attr attr = {0};
        attr.type = PERF_TYPE_SOFTWARE;
        attr.size = PERF_ATTR_SIZE_VER0;
        attr.config = PERF_COUNT_SW_CPU_CLOCK;
        attr.sample_period = 2000000;
        attr.sample_type = PERF_SAMPLE_IP | PERF_SAMPLE_TID | PERF_SAMPLE_TIME | PERF_SAMPLE_PERIOD;
        if (collect_callers) {
            attr.size = PERF_ATTR_SIZE_VER3;
            attr.sample_type |= PERF_SAMPLE_CALLCHAIN | PERF_SAMPLE_REGS_USER | PERF_SAMPLE_STACK_USER;
            attr.exclude_callchain_kernel = 1;
            attr.sample_regs_user = 0xffff; // ARM r0-r15, including SP, LR and PC.
            attr.sample_stack_user = USER_STACK_BYTES;
        }
        attr.disabled = 1;
        attr.exclude_kernel = 1;
        attr.exclude_hv = 1;
        int fd = (int)syscall(__NR_perf_event_open, &attr, tid, -1, -1, 0);
        if (fd < 0) {
            if (errno == ESRCH) continue;
            perror("perf_event_open"); return 1;
        }
        void *map = mmap(NULL, page * 9, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
        if (map == MAP_FAILED) { perror("perf mmap"); return 1; }
        counters[count++] = (struct counter){ .fd = fd, .map = map, .tail = 0 };
    }
    closedir(directory);
    if (!count) return 1;
    double start = seconds();
    for (size_t i = 0; i < count; ++i) {
        if (ioctl(counters[i].fd, PERF_EVENT_IOC_ENABLE, 0)) { perror("perf enable"); return 1; }
    }
    printf("READY threads=%zu hz=500 monotonic_ns=%.0f\n", count, start * 1e9);
    fflush(stdout);
    while (seconds() - start < atof(argv[2])) {
        for (size_t i = 0; i < count; ++i) drain(&counters[i], page);
        fflush(stdout);
        struct timespec interval = { .tv_nsec = 2000000 };
        nanosleep(&interval, NULL);
    }
    for (size_t i = 0; i < count; ++i) ioctl(counters[i].fd, PERF_EVENT_IOC_DISABLE, 0);
    for (size_t i = 0; i < count; ++i) {
        drain(&counters[i], page);
        munmap(counters[i].map, page * 9);
        close(counters[i].fd);
    }
    return 0;
}
