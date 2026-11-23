/* Device-side input-to-log observer for the installed Kobo application.
 * Build with the existing ARM cross compiler, using -O2 -static.
 * Usage: kobo-input-probe info | key CODE [SECONDS] | tap RAW_X RAW_Y [SECONDS]
 * Timestamps use CLOCK_MONOTONIC on the device, starting before input injection.
 * Observes existing logs at 2 ms intervals; does not wait for optical settling.
 */
#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <fcntl.h>
#include <linux/input.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <time.h>
#include <unistd.h>

static double milliseconds(void) {
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now)) { perror("clock_gettime"); exit(1); }
    return now.tv_sec * 1000.0 + now.tv_nsec / 1000000.0;
}

static void emit(int fd, unsigned short type, unsigned short code, int value) {
    struct input_event event = { .type = type, .code = code, .value = value };
    if (write(fd, &event, sizeof(event)) != sizeof(event)) { perror("input write"); exit(1); }
}

int main(int argc, char **argv) {
    int tap = argc >= 2 && !strcmp(argv[1], "tap");
    int info = argc == 2 && !strcmp(argv[1], "info");
    if (!info && !((tap && (argc == 4 || argc == 5)) ||
        (argc >= 3 && argc <= 4 && !strcmp(argv[1], "key")))) {
        fprintf(stderr, "Usage: %s info | key CODE [SECONDS] | tap RAW_X RAW_Y [SECONDS]\n", argv[0]);
        return 2;
    }
    if (info) {
        int fd = open("/dev/input/event1", O_RDONLY);
        if (fd < 0) { perror("touch open"); return 1; }
        for (int axis = ABS_MT_POSITION_X; axis <= ABS_MT_POSITION_Y; ++axis) {
            struct input_absinfo value;
            if (ioctl(fd, EVIOCGABS(axis), &value)) { perror("EVIOCGABS"); return 1; }
            printf("axis=%d min=%d max=%d value=%d\n", axis, value.minimum, value.maximum, value.value);
        }
        close(fd);
        return 0;
    }
    int code = tap ? 0 : atoi(argv[2]);
    if (!tap && code != 193 && code != 194) { fprintf(stderr, "Only page-button keys 193/194 are supported\n"); return 2; }
    double seconds = argc == (tap ? 5 : 4) ? atof(argv[tap ? 4 : 3]) : 4.0;
    if (!(seconds >= 0.5 && seconds <= 120.0)) return 2;
    int log = open("/mnt/onboard/.adds/bokheim/desktop-gpui-kobo.log", O_RDONLY);
    if (log < 0 || lseek(log, 0, SEEK_END) < 0) { perror("log open/seek"); return 1; }
    int input = open(tap ? "/dev/input/event1" : "/dev/input/event0", O_WRONLY);
    if (input < 0) { perror("input open"); return 1; }
    double start = milliseconds();
    printf("0.000 PROBE_START monotonic_ns=%.0f\n", start * 1000000.0);
    if (tap) {
        emit(input, EV_ABS, ABS_MT_TRACKING_ID, 7);
        emit(input, EV_ABS, ABS_MT_POSITION_X, atoi(argv[2]));
        emit(input, EV_ABS, ABS_MT_POSITION_Y, atoi(argv[3]));
        emit(input, EV_ABS, ABS_MT_PRESSURE, 1000);
        emit(input, EV_SYN, SYN_REPORT, 0);
        struct timespec hold = { .tv_nsec = 50000000 };
        nanosleep(&hold, NULL);
        emit(input, EV_ABS, ABS_MT_TRACKING_ID, 7);
        emit(input, EV_ABS, ABS_MT_PRESSURE, 0);
        emit(input, EV_SYN, SYN_REPORT, 0);
    } else {
        emit(input, EV_KEY, code, 1);
        emit(input, EV_SYN, SYN_REPORT, 0);
        emit(input, EV_KEY, code, 0);
        emit(input, EV_SYN, SYN_REPORT, 0);
    }
    close(input);
    printf("%.3f PROBE_INPUT_WRITTEN\n", milliseconds() - start);
    fflush(stdout);
    char buffer[16384];
    size_t used = 0;
    while (milliseconds() - start < seconds * 1000.0) {
        ssize_t count = read(log, buffer + used, sizeof(buffer) - used - 1);
        if (count < 0) { perror("log read"); return 1; }
        used += (size_t) count;
        buffer[used] = 0;
        char *from = buffer, *newline;
        while ((newline = strchr(from, '\n'))) {
            *newline = 0;
            printf("%.3f %s\n", milliseconds() - start, from);
            from = newline + 1;
        }
        used -= (size_t)(from - buffer);
        memmove(buffer, from, used);
        if (used == sizeof(buffer) - 1) { fprintf(stderr, "Log line too long\n"); return 1; }
        fflush(stdout);
        struct timespec interval = { .tv_nsec = 2000000 };
        nanosleep(&interval, NULL);
    }
    close(log);
    return 0;
}
