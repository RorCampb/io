#ifndef IO_BENCHMARK_H
#define IO_BENCHMARK_H
#include <SDL2/SDL.h>
#include "renderer.h"

typedef struct BenchmarkOptions {
    const char *output;
    unsigned int frames, warmup;
    bool orbit, watch, hud;
} BenchmarkOptions;

// Exit status: 0 success, 1 failure, 130 cancelled by the viewer.
int benchmark_run(SDL_Window *window, Renderer *renderer, IoApp *app,
                  const BenchmarkOptions *options);
#endif
