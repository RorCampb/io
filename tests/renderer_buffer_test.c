#include <SDL2/SDL.h>
#include <OpenGL/gl3.h>
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "../csrc/renderer.h"

static void check_contents(const DynamicBuffer *b, const void *expected, size_t bytes) {
    void *actual=malloc(bytes);
    assert(actual);
    glBindBuffer(b->target,b->id);
    GLint64 size=0;
    glGetBufferParameteri64v(b->target,GL_BUFFER_SIZE,&size);
    assert((size_t)size==b->capacity && b->used==bytes);
    glGetBufferSubData(b->target,0,(GLsizeiptr)bytes,actual);
    assert(glGetError()==GL_NO_ERROR && memcmp(actual,expected,bytes)==0);
    free(actual);
}

static void check_stream(unsigned int target) {
    DynamicBuffer b;
    unsigned char data[80];
    memset(data,0x5a,sizeof(data));
    assert(dynamic_buffer_init(&b,target,16,128));
    unsigned int id=b.id;
    assert(dynamic_buffer_upload(&b,NULL,0,1));
    assert(b.capacity==0 && b.orphanings==0);
    assert(dynamic_buffer_upload(&b,data,8,1));
    assert(b.capacity==16 && b.growths==1);
    check_contents(&b,data,8);
    assert(dynamic_buffer_upload(&b,data,sizeof(data),1));
    assert(b.capacity==128 && b.growths==2 && b.id==id);
    check_contents(&b,data,sizeof(data));
    data[0]=0x7c;
    assert(dynamic_buffer_upload(&b,data,4,1));
    check_contents(&b,data,4);
    assert(dynamic_buffer_upload(&b,NULL,0,1));
    assert(b.capacity==128 && b.used==0 && b.uploads==3);
    assert(!dynamic_buffer_upload(&b,NULL,1,1));
    assert(!dynamic_buffer_upload(&b,data,SIZE_MAX,16));
    assert(!dynamic_buffer_upload(&b,data,129,1));
    assert(!dynamic_buffer_upload(&b,data,1,0));
    assert(b.uploads==3 && b.growths==2 && glGetError()==GL_NO_ERROR);
    assert(dynamic_buffer_upload(&b,data,sizeof(data),1));
    check_contents(&b,data,sizeof(data));
    assert(b.uploaded_bytes==172 && b.peak_used==80 && b.orphanings==4);
    dynamic_buffer_destroy(&b);
    assert(b.id==0 && glIsBuffer(id)==GL_FALSE);
    dynamic_buffer_destroy(&b);
}

static void check_renderer(void) {
    IoApp *app=io_app_new();
    assert(app);
    io_app_update(app,0.125f);
    IoFrame frame;
    assert(io_app_frame(app,io_app_active_camera(app),&frame));
    assert(frame.instance_count && frame.joint_count);
    Renderer r;
    assert(renderer_init(&r));
    assert(renderer_resize(&r,1280,800,1280,800));
    assert(renderer_draw(&r,&frame));
    check_contents(&r.instances,frame.instances,frame.instance_count*sizeof(IoInstance));
    check_contents(&r.joints,frame.joint_matrices,frame.joint_count*64);
    uint64_t uploads=r.instances.uploads;
    size_t models=r.model_count;
    unsigned int static_vbo=r.models[0].vbo;
    assert(renderer_draw(&r,&frame));
    assert(r.last_upload_bytes==0 && r.instances.uploads==uploads);

    // A different view's larger selection grows the same stream, retaining meshes.
    size_t count=2049;
    IoInstance *instances=malloc(count*sizeof(*instances));
    assert(instances);
    for(size_t i=0;i<count;i++)instances[i]=frame.instances[0];
    IoVec3 grid[4]={{0,0,0},{1,0,0},{0,0,0},{0,1,0}};
    IoFrame larger=frame;
    larger.serial+=1;
    larger.instances=instances;larger.instance_count=count;
    larger.grid=grid;larger.grid_vertex_count=4;
    assert(renderer_draw(&r,&larger));
    assert(r.instances.capacity>=count*sizeof(*instances) && r.instances.growths==2);
    assert(r.model_count==models && r.models[0].vbo==static_vbo);
    check_contents(&r.instances,instances,count*sizeof(*instances));
    check_contents(&r.grid,grid,sizeof(grid));
    size_t capacity=r.instances.capacity;

    IoFrame empty=larger;
    empty.serial+=1;empty.instances=NULL;empty.instance_count=0;
    empty.grid=NULL;empty.grid_vertex_count=0;
    empty.joint_matrices=NULL;empty.joint_count=0;
    assert(renderer_draw(&r,&empty));
    assert(r.instances.used==0 && r.instances.capacity==capacity && r.grid.used==0);
    const float identity[16]={1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1};
    check_contents(&r.joints,identity,sizeof(identity));
    assert(renderer_draw(&r,&frame));
    check_contents(&r.instances,frame.instances,frame.instance_count*sizeof(IoInstance));
    assert(r.instances.capacity==capacity);

    // A partially uploaded, rejected packet must not poison the previous serial's cache.
    IoFrame bad=larger;
    bad.serial+=10;bad.grid=NULL;
    assert(!renderer_draw(&r,&bad));
    assert(!r.frame_uploaded);
    assert(renderer_draw(&r,&frame));
    check_contents(&r.instances,frame.instances,frame.instance_count*sizeof(IoInstance));
    bad=frame;bad.joint_count=(size_t)r.max_joint_matrices+1;
    assert(!renderer_draw(&r,&bad));
    free(instances);
    renderer_destroy(&r);
    io_app_free(app);
}

static uint32_t item_mesh(const IoFrame *frame,uint64_t item) {
    for(size_t i=0;i<frame->instance_count;i++) {
        const IoInstance *instance=&frame->instances[i];
        if(instance->item_id!=item)continue;
        IoModel model;assert(io_model_get(instance->model_id,&model));
        for(size_t v=0;v<model.vertex_count;v++)for(size_t k=0;k<4;k++) {
            if(model.vertices[v].weights[k]>0.f)
                assert(instance->joint_offset+model.vertices[v].joints[k]<frame->joint_count);
        }
        return instance->model_id;
    }
    assert(!"missing visible item");return 0;
}

static void check_variants(void) {
    IoApp *app=io_app_new();assert(app);
    IoFrame frame;assert(io_app_frame(app,1,&frame));
    uint64_t runner=0;
    for(size_t i=0;i<frame.instance_count;i++) {
        IoModel model;assert(io_model_get(frame.instances[i].model_id,&model));
        for(size_t v=0;v<model.vertex_count;v++) {
            if(model.vertices[v].weights[0]>0.f)runner=frame.instances[i].item_id;
        }
    }
    assert(runner);
    IoItemState before,after;assert(io_app_item_state(app,runner,&before));
    assert(io_app_set_camera_target(app,1,before.anchor));
    assert(io_app_dispatch(app,(IoAction){.kind=IO_ACTION_ZOOM,.x=100.f}));
    assert(io_app_frame(app,1,&frame));
    uint32_t high=item_mesh(&frame,runner);
    Renderer r;assert(renderer_init(&r));
    assert(renderer_resize(&r,1280,800,1280,800));
    assert(renderer_draw(&r,&frame));
    unsigned int high_vbo=0;
    for(size_t i=0;i<r.model_count;i++)if(r.models[i].id==high)high_vbo=r.models[i].vbo;
    assert(high_vbo);
    assert(io_app_dispatch(app,(IoAction){.kind=IO_ACTION_ZOOM,.x=-100.f}));
    assert(io_app_frame(app,1,&frame));
    uint32_t low=item_mesh(&frame,runner);assert(low!=high);
    IoModel high_model,low_model;
    assert(io_model_get(high,&high_model) && io_model_get(low,&low_model));
    assert(low_model.index_count<high_model.index_count/2);
    assert(renderer_draw(&r,&frame));
    check_contents(&r.instances,frame.instances,frame.instance_count*sizeof(IoInstance));
    check_contents(&r.joints,frame.joint_matrices,frame.joint_count*64);
    size_t loaded=r.model_count;
    assert(io_app_dispatch(app,(IoAction){.kind=IO_ACTION_ZOOM,.x=100.f}));
    assert(io_app_frame(app,1,&frame));
    assert(item_mesh(&frame,runner)==high && renderer_draw(&r,&frame));
    assert(r.model_count==loaded);
    for(size_t i=0;i<r.model_count;i++)if(r.models[i].id==high)assert(r.models[i].vbo==high_vbo);
    assert(renderer_draw(&r,&frame) && r.last_upload_bytes==0);
    assert(io_app_item_state(app,runner,&after));
    assert(before.health==after.health && before.simulated_ticks==after.simulated_ticks);
    assert(memcmp(&before.anchor,&after.anchor,sizeof(before.anchor))==0);
    renderer_destroy(&r);io_app_free(app);
    puts("PASS: real skinned LOD switching, palette bounds/readback, and retained static GPU meshes");
}

int main(int argc,char **argv) {
    bool variants=argc==2 && strcmp(argv[1],"--variants")==0;
    if(variants)assert(setenv("IO_SCENE","assets/street-kit/variants-demo.json",1)==0);
    assert(SDL_Init(SDL_INIT_VIDEO)==0);
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_MAJOR_VERSION,4);
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_MINOR_VERSION,1);
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_PROFILE_MASK,SDL_GL_CONTEXT_PROFILE_CORE);
    SDL_Window *window=SDL_CreateWindow("io buffer tests",0,0,1280,800,SDL_WINDOW_OPENGL|SDL_WINDOW_HIDDEN);
    assert(window);
    SDL_GLContext context=SDL_GL_CreateContext(window);
    assert(context && SDL_GL_MakeCurrent(window,context)==0);
    if(variants)check_variants();
    else {
        check_stream(GL_ARRAY_BUFFER);
        check_stream(GL_TEXTURE_BUFFER);
        check_renderer();
    }
    SDL_GL_DeleteContext(context);
    SDL_DestroyWindow(window);
    SDL_Quit();
    puts("PASS: GPU contents, capacity growth/reuse, empty frames, overflow rejection, cached frames, and view switching");
    return 0;
}
