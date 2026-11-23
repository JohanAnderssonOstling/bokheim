/* In-memory measurement of the pinned FBInk rescaler on Kobo.
 * Does not open input devices or the framebuffer, or change the running app.
 * Link with the existing ARM FBInk Release/libfbink.a and -lm -static.
 * Dimensions match the large text-page damage region in our chapter captures.
 */
#define _POSIX_C_SOURCE 200809L
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

unsigned char *qSmoothScaleImage(const unsigned char *, int, int, int, _Bool, int, int);

static double cpu_ms(void) {
    struct timespec value;
    if (clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &value)) { perror("clock_gettime"); exit(1); }
    return value.tv_sec * 1000.0 + value.tv_nsec / 1000000.0;
}

int main(void) {
    const int width = 1540, height = 1183;
    const size_t pixels = (size_t)width * height;
    unsigned char *rgba = malloc(pixels * 4);
    unsigned char *gray = malloc(pixels);
    unsigned char *copy = malloc(pixels * 4);
    if (!rgba || !gray || !copy) return 1;
    for (size_t i = 0; i < pixels; ++i) {
        unsigned char value = (unsigned char)((i * 17 + i / width * 31) & 255);
        gray[i] = value;
        rgba[i * 4] = rgba[i * 4 + 1] = rgba[i * 4 + 2] = value;
        rgba[i * 4 + 3] = 255;
    }
    puts("case,round,cpu_ms,bytes,identity_rgb,checksum");
    for (int round = 0; round < 8; ++round) {
        for (int which = 0; which < 5; ++which) {
            int channels = which == 2 ? 1 : 4;
            int out_width = which == 1 ? width + 1 : width;
            int out_height = which == 1 ? height + 1 : height;
            size_t bytes = (size_t)out_width * out_height * channels;
            double start = cpu_ms();
            unsigned char *output;
            if (which == 4) {
                uint32_t *destination = (uint32_t *)copy;
                for (size_t i = 0; i < pixels; ++i) {
                    destination[i] = 0xFF000000U | (uint32_t)gray[i] * 0x010101U;
                }
                output = copy;
            } else if (which == 3) {
                output = memcpy(copy, rgba, pixels * 4);
            } else {
                output = qSmoothScaleImage(channels == 1 ? gray : rgba, width, height,
                                           channels, 1, out_width, out_height);
                if (!output) return 1;
            }
            double elapsed = cpu_ms() - start;
            int identity = which == 1 ? -1 : 1;
            uint64_t checksum = 0;
            for (size_t i = 0; i < (size_t)out_width * out_height; ++i) {
                checksum += output[i * channels];
                if (which != 1) {
                    if (output[i * channels] != gray[i]) identity = 0;
                    if (channels == 4 && (output[i * 4 + 1] != gray[i] || output[i * 4 + 2] != gray[i])) identity = 0;
                }
            }
            printf("%s,%d,%.6f,%zu,%d,%llu\n",
                   which == 0 ? "rgba_identity_rescale" : which == 1 ? "rgba_resize_plus_one" :
                   which == 2 ? "gray_identity_rescale" : which == 3 ? "rgba_memcpy_reused_buffer" :
                   "gray_to_bgra_reused_buffer",
                   round, elapsed, bytes, identity, (unsigned long long)checksum);
            if (which < 3) free(output);
        }
    }
    free(copy);
    free(gray);
    free(rgba);
    return 0;
}
