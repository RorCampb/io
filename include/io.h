#ifndef IO_H
#define IO_H

#include <stddef.h>

typedef struct IoWorld IoWorld;

typedef struct IoLine {
    float x1;
    float y1;
    float x2;
    float y2;
    float r;
    float g;
    float b;
} IoLine;

typedef struct IoPoint {
    float x;
    float y;
    float r;
    float g;
    float b;
} IoPoint;

IoWorld *io_world_new(void);
void io_world_free(IoWorld *world);
void io_world_resize_units_a(IoWorld *world, int delta);
void io_world_resize_units_b(IoWorld *world, int delta);
void io_world_resize_units_c(IoWorld *world, int delta);
// Camera angles are radians; positive zoom steps zoom in. Viewport uses logical window units.
void io_world_orbit_camera(IoWorld *world, float yaw_delta, float pitch_delta);
void io_world_zoom_camera(IoWorld *world, float steps);
void io_world_set_viewport(IoWorld *world, int width, int height);
void io_world_reset_camera(IoWorld *world);
// Borrowed render buffers: consume before any world mutation or free; use on one thread.
const IoLine *io_world_lines(IoWorld *world, size_t *out_count);
const IoPoint *io_world_points(IoWorld *world, size_t *out_count);

#endif
