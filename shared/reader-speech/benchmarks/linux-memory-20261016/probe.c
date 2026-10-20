#include "c-api.h"
#include <math.h>
#include <malloc.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static double seconds(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec + t.tv_nsec / 1e9;
}
static long status_value(const char *key) {
  FILE *f = fopen("/proc/self/status", "r");
  char line[512];
  long value = 0;
  while (fgets(line, sizeof(line), f)) {
    if (strncmp(line, key, strlen(key)) == 0) {
      value = strtol(line + strlen(key), NULL, 10);
      break;
    }
  }
  fclose(f);
  return value;
}
static void record(const char *phase, int iteration, int text_bytes,
                   double synthesis, double audio_seconds, double started) {
  struct mallinfo2 m = mallinfo2();
  printf("%s,%d,%d,%.4f,%.4f,%.4f,%ld,%ld,%ld,%zu,%zu\n", phase,
         iteration, text_bytes, synthesis, audio_seconds, seconds() - started,
         status_value("VmRSS:"), status_value("RssAnon:"),
         status_value("VmHWM:"), m.uordblks / 1024, m.fordblks / 1024);
  fflush(stdout);
}
static int32_t progress(const float *samples, int32_t n, float p, void *arg) {
  return 1;
}
int main(int argc, char **argv) {
  if (argc != 5) {
    fprintf(stderr, "usage: probe MODEL_DIRECTORY PROVIDER PASSAGES_FILE REPEATS\n");
    return 2;
  }
  fprintf(stderr, "sherpa=%s sha=%s ort=%s provider=%s\n", SherpaOnnxGetVersionStr(),
          SherpaOnnxGetGitSha1(), SherpaOnnxGetOnnxruntimeVersionStr(), argv[2]);
  double started = seconds();
  printf("phase,iteration,text_bytes,synthesis_seconds,audio_seconds,elapsed_seconds,rss_kib,anonymous_rss_kib,peak_rss_kib,heap_allocated_kib,heap_free_kib\n");
  record("before_load", -1, 0, 0, 0, started);
  char model[1024], tokens[1024], data[1024];
  snprintf(model, sizeof(model), "%s/en_GB-alba-medium.onnx", argv[1]);
  snprintf(tokens, sizeof(tokens), "%s/tokens.txt", argv[1]);
  snprintf(data, sizeof(data), "%s/espeak-ng-data", argv[1]);
  SherpaOnnxOfflineTtsConfig cfg = {0};
  cfg.model.vits.model = model;
  cfg.model.vits.tokens = tokens;
  cfg.model.vits.data_dir = data;
  cfg.model.vits.noise_scale = .75f;
  cfg.model.vits.noise_scale_w = .8f;
  cfg.model.vits.length_scale = 1.f;
  cfg.model.num_threads = 2;
  cfg.model.provider = argv[2];
  const SherpaOnnxOfflineTts *tts = SherpaOnnxCreateOfflineTts(&cfg);
  if (!tts) return 3;
  record("after_load", -1, 0, 0, 0, started);
  SherpaOnnxGenerationConfig gen = {0};
  gen.speed = 1.f;
  gen.silence_scale = 1.f;
  gen.extra = "{}";
  gen.num_steps = 5;
  for (int repeat = 0; repeat < atoi(argv[4]); ++repeat) {
    FILE *f = fopen(argv[3], "r");
    if (!f) return 4;
    char *line = NULL;
    size_t capacity = 0;
    int i = 0;
    while (getline(&line, &capacity, f) > 0) {
      line[strcspn(line, "\r\n")] = 0;
      if (!line[0]) continue;
      double begin = seconds();
      const SherpaOnnxGeneratedAudio *audio = SherpaOnnxOfflineTtsGenerateWithConfig(tts, line, &gen, progress, NULL);
      double elapsed = seconds() - begin;
      if (!audio || audio->n <= 0 || audio->sample_rate != 22050) return 5;
      double duration = (double)audio->n / audio->sample_rate;
      double energy = 0;
      for (int s = 0; s < audio->n; ++s) {
        if (!isfinite(audio->samples[s])) return 6;
        energy += fabs(audio->samples[s]);
      }
      if (energy < .001) return 7;
      SherpaOnnxDestroyOfflineTtsGeneratedAudio(audio);
      record("audio_freed", repeat * 1000 + i++, strlen(line), elapsed, duration, started);
    }
    free(line);
    fclose(f);
  }
  SherpaOnnxDestroyOfflineTts(tts);
  record("model_dropped", -1, 0, 0, 0, started);
  malloc_trim(0);
  record("after_malloc_trim", -1, 0, 0, 0, started);
  sleep(2);
  record("after_drop_idle", -1, 0, 0, 0, started);
  return 0;
}
