#ifndef IO_RENDERER_H
#define IO_RENDERER_H
#include <stdbool.h>
#include <stddef.h>
#include "io.h"
#include "dynamic_buffer.h"

typedef struct RenderModel {
    uint32_t id;
    unsigned int vao,vbo,ebo;
    int index_count;
    unsigned int topology;
} RenderModel;
typedef struct Renderer {
    unsigned int program,solid_program,grid_vao,joint_texture;
    DynamicBuffer instances,grid,joints;
    int max_joint_matrices;
    int matrix_uniform,viewport_uniform,width_uniform;
    int solid_matrix_uniform,joints_uniform;
    int width,height,drawable_width,drawable_height;
    float pixel_scale;
    RenderModel *models;
    size_t model_count;
    uint64_t frame_serial;
    bool frame_uploaded;
    size_t last_upload_bytes;
} Renderer;
bool renderer_init(Renderer *renderer);
void renderer_destroy(Renderer *renderer);
bool renderer_resize(Renderer *renderer,int width,int height,int drawable_width,int drawable_height);
bool renderer_draw(Renderer *renderer,const IoFrame *frame);
#endif
