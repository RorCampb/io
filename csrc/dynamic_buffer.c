#include "dynamic_buffer.h"
#include <OpenGL/gl3.h>
#include <inttypes.h>
#include <stdio.h>

bool dynamic_buffer_init(DynamicBuffer *b, unsigned int target,
                         size_t initial_capacity, size_t limit) {
    *b = (DynamicBuffer){0};
    if ((target != GL_ARRAY_BUFFER && target != GL_TEXTURE_BUFFER) ||
        !initial_capacity || !limit || limit > (size_t)PTRDIFF_MAX) return false;
    b->target = target;
    b->limit = limit;
    b->initial_capacity = initial_capacity < limit ? initial_capacity : limit;
    glGenBuffers(1, &b->id);
    if (glGetError() != GL_NO_ERROR || !b->id) {
        dynamic_buffer_destroy(b);
        return false;
    }
    return true;
}

bool dynamic_buffer_upload(DynamicBuffer *b, const void *data, size_t count, size_t stride) {
    if (!b->id || !stride || count > b->limit / stride || (count && !data)) return false;
    size_t bytes = count * stride;
    if (!bytes) {
        b->used = 0;
        return true;
    }
    size_t capacity = b->capacity ? b->capacity : b->initial_capacity;
    while (capacity < bytes) {
        capacity = capacity > b->limit / 2 ? b->limit : capacity * 2;
    }
    glBindBuffer(b->target, b->id);
    // Orphan the old store so queued draws can finish reading it. The driver owns
    // backing-store reuse; capacity growth and orphaning are separate operations.
    glBufferData(b->target, (GLsizeiptr)capacity, NULL, GL_STREAM_DRAW);
    if (glGetError() != GL_NO_ERROR) {
        fprintf(stderr, "Cannot allocate %zu-byte streaming buffer\n", capacity);
        return false;
    }
    b->orphanings++;
    if (capacity != b->capacity) b->growths++;
    b->capacity = capacity;
    b->used = 0;
    glBufferSubData(b->target, 0, (GLsizeiptr)bytes, data);
    if (glGetError() != GL_NO_ERROR) {
        fprintf(stderr, "Cannot upload %zu bytes to streaming buffer\n", bytes);
        return false;
    }
    b->used = bytes;
    if (bytes > b->peak_used) b->peak_used = bytes;
    b->uploads++;
    b->uploaded_bytes += bytes;
    return true;
}

void dynamic_buffer_report(const DynamicBuffer *b, const char *name) {
    if (!b->id) return;
    fprintf(stderr, "Buffer %s: capacity=%zu B, peak=%zu B, uploaded=%" PRIu64
            " B, uploads=%" PRIu64 ", growths=%" PRIu64 ", orphanings=%" PRIu64 "\n",
            name, b->capacity, b->peak_used, b->uploaded_bytes,
            b->uploads, b->growths, b->orphanings);
}

void dynamic_buffer_destroy(DynamicBuffer *b) {
    if (b->id) glDeleteBuffers(1, &b->id);
    *b = (DynamicBuffer){0};
}
