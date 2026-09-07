#ifndef IO_RENDERER_H
#define IO_RENDERER_H

#include <stdbool.h>
#include <stddef.h>
#include "io.h"

typedef struct Renderer {
    int width;
    int height;
    int drawable_width;
    int drawable_height;
    float pixel_scale;
    float max_line_width;
    float max_point_size;
} Renderer;

bool renderer_init(Renderer *renderer);
void renderer_destroy(Renderer *renderer);
bool renderer_resize(Renderer *renderer, int width, int height, int drawable_width, int drawable_height);
void renderer_clear(void);
void renderer_draw_lines(Renderer *renderer, const IoLine *lines, size_t count);
void renderer_draw_points(Renderer *renderer, const IoPoint *points, size_t count);

#endif
