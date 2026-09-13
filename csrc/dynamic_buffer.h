#ifndef IO_DYNAMIC_BUFFER_H
#define IO_DYNAMIC_BUFFER_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct DynamicBuffer {
    unsigned int id, target;
    size_t capacity, used, initial_capacity, limit, peak_used;
    uint64_t uploads, orphanings, growths, uploaded_bytes;
} DynamicBuffer;

// Call with a current GL context. Storage is allocated lazily on first nonempty upload.
bool dynamic_buffer_init(DynamicBuffer *buffer, unsigned int target,
                         size_t initial_capacity, size_t limit);
// Replaces the active contents at offset zero; never retains the source pointer.
// Rejects invalid/oversized input before touching GL. Empty uploads retain capacity.
bool dynamic_buffer_upload(DynamicBuffer *buffer, const void *data, size_t count, size_t stride);
void dynamic_buffer_report(const DynamicBuffer *buffer, const char *name);
void dynamic_buffer_destroy(DynamicBuffer *buffer);

#endif
