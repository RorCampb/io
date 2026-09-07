#include "renderer.h"

#include <SDL2/SDL_opengl.h>
#include <stdio.h>

bool renderer_init(Renderer *renderer) {
    *renderer = (Renderer){0};
    GLint sample_buffers = 0;
    GLint samples = 0;
    glGetIntegerv(GL_SAMPLE_BUFFERS, &sample_buffers);
    glGetIntegerv(GL_SAMPLES, &samples);
    bool multisampled = sample_buffers > 0 && samples > 1;
    if (multisampled) {
        glEnable(GL_MULTISAMPLE);
        fprintf(stderr, "Antialiasing: %dx MSAA\n", samples);
    } else {
        glDisable(GL_MULTISAMPLE);
        glEnable(GL_BLEND);
        glBlendFunc(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA);
        glEnable(GL_LINE_SMOOTH);
        glHint(GL_LINE_SMOOTH_HINT, GL_NICEST);
        fprintf(stderr, "Antialiasing: driver line-smoothing fallback\n");
    }

    GLfloat range[2];
    glGetFloatv(multisampled ? GL_ALIASED_LINE_WIDTH_RANGE : GL_SMOOTH_LINE_WIDTH_RANGE, range);
    renderer->max_line_width = range[1];
    glGetFloatv(GL_ALIASED_POINT_SIZE_RANGE, range);
    renderer->max_point_size = range[1];
    return true;
}

void renderer_destroy(Renderer *renderer) {
    (void)renderer;
}

bool renderer_resize(Renderer *renderer, int width, int height, int drawable_width, int drawable_height) {
    if (width <= 0 || height <= 0 || drawable_width <= 0 || drawable_height <= 0) {
        return false;
    }
    if (renderer->width == width && renderer->height == height &&
        renderer->drawable_width == drawable_width && renderer->drawable_height == drawable_height) {
        return true;
    }

    renderer->width = width;
    renderer->height = height;
    renderer->drawable_width = drawable_width;
    renderer->drawable_height = drawable_height;
    renderer->pixel_scale = (float)drawable_width / (float)width;

    // Rasterize at native resolution while keeping scene coordinates in window units.
    glViewport(0, 0, drawable_width, drawable_height);
    glMatrixMode(GL_PROJECTION);
    glLoadIdentity();
    glOrtho(0.0, (double)width, (double)height, 0.0, -1.0, 1.0);
    glMatrixMode(GL_MODELVIEW);
    glLoadIdentity();
    fprintf(stderr, "Window: %dx%d; framebuffer: %dx%d\n", width, height, drawable_width, drawable_height);
    return true;
}

void renderer_clear(void) {
    glClearColor(0.0f, 0.0f, 0.0f, 1.0f);
    glClear(GL_COLOR_BUFFER_BIT);
}

void renderer_draw_lines(Renderer *renderer, const IoLine *lines, size_t count) {
    if (lines == NULL || count == 0) {
        return;
    }

    float line_width = renderer->pixel_scale;
    glLineWidth(line_width < renderer->max_line_width ? line_width : renderer->max_line_width);
    glBegin(GL_LINES);
    for (size_t i = 0; i < count; i += 1) {
        glColor3f(lines[i].r, lines[i].g, lines[i].b);
        glVertex2f(lines[i].x1, lines[i].y1);
        glVertex2f(lines[i].x2, lines[i].y2);
    }
    glEnd();
}

void renderer_draw_points(Renderer *renderer, const IoPoint *points, size_t count) {
    if (points == NULL || count == 0) {
        return;
    }

    float point_size = 8.0f * renderer->pixel_scale;
    glPointSize(point_size < renderer->max_point_size ? point_size : renderer->max_point_size);
    glBegin(GL_POINTS);
    for (size_t i = 0; i < count; i += 1) {
        glColor3f(points[i].r, points[i].g, points[i].b);
        glVertex2f(points[i].x, points[i].y);
    }
    glEnd();
}
