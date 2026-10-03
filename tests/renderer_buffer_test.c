#include <SDL2/SDL.h>
#include <OpenGL/gl3.h>
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>
#include "../csrc/renderer.h"
#include "../csrc/editor_ui.h"
#include "../csrc/attention_ui.h"

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

static void check_hud(void){
    Renderer r;assert(renderer_init(&r));
    hud_set_tick_hz(&r.hud,144);
    assert(renderer_resize(&r,1280,800,1280,800));
    glClearColor(1,1,1,1);glClear(GL_COLOR_BUFFER_BIT|GL_DEPTH_BUFFER_BIT);
    glEnable(GL_DEPTH_TEST);glDisable(GL_BLEND);
    glUseProgram(0);glBindVertexArray(0);glBindBuffer(GL_ARRAY_BUFFER,0);
    IoWorkerStats stats={.status=1};hud_presented(&r.hud,10.,&stats);
    for(unsigned int i=1;i<=30;i++){
        stats.tick=i/2;hud_presented(&r.hud,10.+(double)i/60.,&stats);
    }
    assert(renderer_draw_hud(&r) && r.hud.last_upload_bytes>0);
    assert(glIsEnabled(GL_DEPTH_TEST) && !glIsEnabled(GL_BLEND));
    GLint program,vao,buffer;glGetIntegerv(GL_CURRENT_PROGRAM,&program);
    glGetIntegerv(GL_VERTEX_ARRAY_BINDING,&vao);glGetIntegerv(GL_ARRAY_BUFFER_BINDING,&buffer);
    assert(program==0 && vao==0 && buffer==0);
    unsigned char pixel[4];glReadPixels(1260,780,1,1,GL_RGBA,GL_UNSIGNED_BYTE,pixel);
    assert(pixel[0]<70 && pixel[1]<70 && pixel[2]<70);
    glReadPixels(1045,776,1,1,GL_RGBA,GL_UNSIGNED_BYTE,pixel);
    assert(pixel[0]>200 && pixel[1]>150 && pixel[2]<110);
    // The fourth row's B glyph must be drawn, not clipped by the old panel size.
    glReadPixels(1045,705,1,1,GL_RGBA,GL_UNSIGNED_BYTE,pixel);
    assert(pixel[0]>200 && pixel[1]>200 && pixel[2]>200);
    // The measured frame and update rows are below the original four-row panel.
    glReadPixels(1045,681,1,1,GL_RGBA,GL_UNSIGNED_BYTE,pixel);
    assert(pixel[0]>200 && pixel[1]<150 && pixel[2]<150);
    glReadPixels(1045,657,1,1,GL_RGBA,GL_UNSIGNED_BYTE,pixel);
    assert(pixel[0]>200 && pixel[1]<150 && pixel[2]<150);
    assert(renderer_draw_hud(&r) && r.hud.last_upload_bytes==0);
    assert(renderer_resize(&r,640,400,1280,800));
    assert(renderer_draw_hud(&r) && r.hud.last_upload_bytes>0);
    assert(renderer_resize(&r,1280,800,1280,800));
    IoGameView game={.enabled=1,.free_movement=1,.line_count=2};
    strcpy(game.lines[0],"ROUND BEGIN [ENTER]");
    strcpy(game.lines[1],"the road is quiet, isn't it?");
    hud_set_game(&r.hud,&game);
    assert(renderer_draw_hud(&r) && r.hud.last_upload_bytes>0);
    glReadPixels(20,780,1,1,GL_RGBA,GL_UNSIGNED_BYTE,pixel);
    assert(pixel[0]>200 && pixel[1]>150 && pixel[2]<110);
    /* Lowercase h must produce visible white ink, not a blank dialogue sentence. */
    glReadPixels(27,763,1,1,GL_RGBA,GL_UNSIGNED_BYTE,pixel);
    assert(pixel[0]>200 && pixel[1]>200 && pixel[2]>200);
    hud_set_game(&r.hud,&game);
    assert(renderer_draw_hud(&r) && r.hud.last_upload_bytes==0);
    renderer_destroy(&r);
    puts("PASS: HUD pixels, GL state restoration, cached upload, and HiDPI resize");
}

static void check_game_feedback(void){
    IoApp *app=io_app_new();assert(app);
    Renderer r;assert(renderer_init(&r));assert(r.stencil_bits>0);
    assert(renderer_resize(&r,1280,800,1280,800));
    IoFrame frame;assert(io_app_frame(app,1,&frame));
    const size_t bytes=1280*800*4;
    unsigned char *before=malloc(bytes),*after=malloc(bytes);assert(before&&after);
    assert(renderer_draw(&r,&frame));glReadPixels(0,0,1280,800,GL_RGBA,GL_UNSIGNED_BYTE,before);
    IoGameView game;assert(io_app_game_view(app,&game));assert(game.selected_item==2);
    hud_set_game(&r.hud,&game);assert(renderer_draw(&r,&frame));
    glReadPixels(0,0,1280,800,GL_RGBA,GL_UNSIGNED_BYTE,after);
    size_t outline=0;for(size_t p=0;p<bytes;p+=4)
        if(after[p]>240 && after[p+1]>190 && after[p+2]<60 && memcmp(before+p,after+p,3))outline++;
    assert(outline>20);
    assert(!glIsEnabled(GL_STENCIL_TEST));
    game.projectile_count=1;game.projectiles[0]=(IoProjectileView){.position={0,0,4},.radius=0.3f};
    hud_set_game(&r.hud,&game);assert(renderer_draw(&r,&frame));
    const float *m=frame.clip_from_world;
    int x=(int)((m[8]*4+m[12]+1)*640),y=(int)((m[9]*4+m[13]+1)*400);
    unsigned char pixel[4];glReadPixels(x,y,1,1,GL_RGBA,GL_UNSIGNED_BYTE,pixel);
    assert(pixel[2]>220 && pixel[1]>180);
    game.damage_count=1;game.damage[0]=(IoDamageText){.x=600,.y=600,.alpha=1,.amount=8};
    hud_set_game(&r.hud,&game);assert(renderer_draw_hud(&r));
    glReadPixels(592,192,1,1,GL_RGBA,GL_UNSIGNED_BYTE,pixel);
    assert(pixel[0]>240 && pixel[1]>140 && pixel[1]<190 && pixel[2]<80);
    assert(hud_contains_point(&r.hud,20,20));assert(!hud_contains_point(&r.hud,600,400));
    free(before);free(after);renderer_destroy(&r);io_app_free(app);
    puts("PASS: selected mesh silhouette, projectile glow, damage numbers and UI hit exclusion");
}

static void capture_camera(const char *name, const unsigned char *pixels){
    char path[128];snprintf(path,sizeof(path),"build/camera-%s.ppm",name);
    FILE *f=fopen(path,"wb");assert(f);fprintf(f,"P6\n1280 800\n255\n");
    for(int y=799;y>=0;y--)for(int x=0;x<1280;x++)assert(fwrite(pixels+((size_t)y*1280+x)*4,1,3,f)==3);
    assert(fclose(f)==0);
}

static void check_camera_depth_sweep(IoApp *app, Renderer *r, unsigned char *pixels){
    unsigned char *reference=malloc(1280*800*4);assert(reference);
    assert(io_app_dispatch(app,(IoAction){.kind=IO_ACTION_ZOOM,.x=logf(5.f)/0.12f}));
    for(int step=0;step<120;step++){
        io_app_update(app,1.f/60.f);
        IoFrame frame;assert(io_app_frame(app,1,&frame));
        IoInstance surfaces[2];size_t found=0;
        for(size_t i=0;i<frame.instance_count;i++)if(frame.instances[i].item_id<=2){
            assert(found<2);surfaces[frame.instances[i].item_id-1]=frame.instances[i];found++;
        }
        assert(found==2);
        // Use the real path and floor, but remove occluders from this pixel test.
        IoFrame isolated=frame;isolated.grid_vertex_count=0;isolated.joint_count=0;
        isolated.instances=&surfaces[1];isolated.instance_count=1;
        isolated.serial=UINT64_MAX;
        assert(renderer_draw(r,&isolated));
        glReadPixels(0,0,1280,800,GL_RGBA,GL_UNSIGNED_BYTE,reference);
        for(int order=0;order<2;order++){
            if(order){IoInstance tmp=surfaces[0];surfaces[0]=surfaces[1];surfaces[1]=tmp;}
            isolated.instances=surfaces;isolated.instance_count=2;
            isolated.serial=UINT64_MAX-1-(uint64_t)order;
            assert(renderer_draw(r,&isolated));
            glReadPixels(0,0,1280,800,GL_RGBA,GL_UNSIGNED_BYTE,pixels);
            const float *m=frame.clip_from_world;
            size_t checked=0,wrong=0;
            for(int ix=0;ix<5;ix++)for(int iy=0;iy<37;iy++){
                float wx=48.8f+ix*0.6f,wy=41.f+iy*0.5f,wz=-0.01f;
                float w=m[3]*wx+m[7]*wy+m[11]*wz+m[15];
                int x=(int)(((m[0]*wx+m[4]*wy+m[8]*wz+m[12])/w+1)*640);
                int y=(int)(((m[1]*wx+m[5]*wy+m[9]*wz+m[13])/w+1)*400);
                if(w<=0 || x<2 || x>=1278 || y<2 || y>=798)continue;
                size_t at=((size_t)y*1280+x)*4;checked++;
                for(int c=0;c<3;c++)if(abs((int)pixels[at+c]-(int)reference[at+c])>3){wrong++;break;}
            }
            assert(checked>50);
            if(wrong){
                fprintf(stderr,"Depth sweep step %d, order %d: %zu/%zu path samples obscured by lower floor\n",step,order,wrong,checked);
                assert(renderer_draw(r,&frame));
                glReadPixels(0,0,1280,800,GL_RGBA,GL_UNSIGNED_BYTE,pixels);
                capture_camera("depth-failure",pixels);
            }
            assert(wrong==0);
        }
        if(step==2){
            assert(renderer_draw(r,&frame));
            glReadPixels(0,0,1280,800,GL_RGBA,GL_UNSIGNED_BYTE,pixels);
            capture_camera("transition",pixels);
        }
    }
    free(reference);
    puts("PASS: 120 eased zoom frames retain path/floor depth separation in both draw orders");
}

static void check_camera(void){
    IoApp *app=io_app_new();assert(app);
    Renderer r;assert(renderer_init(&r));
    assert(renderer_resize(&r,1280,800,1280,800));
    const size_t bytes=1280*800*4;
    unsigned char *pixels=malloc(bytes);assert(pixels);
    const float steps[]={0.f,7.635756f,5.776227f,-13.411983f};
    const char *names[]={"ortho","blend","perspective","return"};
    for(size_t i=0;i<4;i++){
        assert(io_app_dispatch(app,(IoAction){.kind=IO_ACTION_ZOOM,.x=steps[i]}));
        for(int tick=0;tick<120;tick++)io_app_update(app,1.f/60.f);
        IoFrame frame;assert(io_app_frame(app,1,&frame));
        const float *m=frame.clip_from_world;
        float convergence=sqrtf(m[3]*m[3]+m[7]*m[7]+m[11]*m[11]);
        if(i==0 || i==3)assert(convergence==0.f);else assert(convergence>0.f);
        assert(frame.instance_count>0 && frame.joint_count==65);
        assert(renderer_draw(&r,&frame));
        glReadPixels(0,0,1280,800,GL_RGBA,GL_UNSIGNED_BYTE,pixels);
        assert(glGetError()==GL_NO_ERROR);
        // Every stage retains the animated character, not merely a valid empty frame.
        size_t red=0;
        for(size_t p=0;p<bytes;p+=4)if(pixels[p]>pixels[p+1]*1.4f && pixels[p]>pixels[p+2]*1.4f && pixels[p]>90)red++;
        assert(red>100);
        capture_camera(names[i],pixels);
        // Perspective billboard sizing/placement uses the same homogeneous w.
        IoGameView game={.enabled=1,.projectile_count=1};
        game.projectiles[0]=(IoProjectileView){.position={50,50,4},.radius=0.3f};
        hud_set_game(&r.hud,&game);assert(renderer_draw(&r,&frame));
        float w=m[3]*50+m[7]*50+m[11]*4+m[15];
        int x=(int)(((m[0]*50+m[4]*50+m[8]*4+m[12])/w+1)*640);
        int y=(int)(((m[1]*50+m[5]*50+m[9]*4+m[13])/w+1)*400);
        assert(x>=0 && x<1280 && y>=0 && y<800);
        unsigned char pixel[4];glReadPixels(x,y,1,1,GL_RGBA,GL_UNSIGNED_BYTE,pixel);
        assert(pixel[2]>220 && pixel[1]>180);
        game.enabled=0;hud_set_game(&r.hud,&game);
    }
    check_camera_depth_sweep(app,&r,pixels);
    free(pixels);renderer_destroy(&r);io_app_free(app);
    puts("PASS: orthographic/blended/perspective skinned drawing, reversible zoom and depth-correct projectiles");
}

static void dungeon_move(IoApp *app,int ticks,float forward){
    for(int i=0;i<ticks;i++){
        IoFrame f;assert(io_app_frame(app,1,&f));
        float x=f.clip_from_world[0],y=f.clip_from_world[4],length=hypotf(x,y);
        assert(io_app_game_action(app,2,0,-y/length*forward,x/length*forward));
        io_app_update(app,1.f/60.f);
    }
}
static void check_dungeon(void){
    IoApp *app=io_app_new();assert(app);
    Renderer r;assert(renderer_init(&r));assert(renderer_resize(&r,1280,800,1280,800));
    unsigned char *pixels=malloc(1280*800*4);assert(pixels);
    IoGameView game;assert(io_app_game_view(app,&game));assert(game.enabled && game.free_movement && game.selected_item);
    uint64_t hero=game.selected_item;IoItemState state;
    io_app_update(app,0.1f);
    dungeon_move(app,60,1);
    assert(io_app_item_state(app,hero,&state));assert(fabsf(state.anchor.y-54.6f)<0.02f);
    IoFrame jog;assert(io_app_frame(app,1,&jog));assert(renderer_draw(&r,&jog));
    glReadPixels(0,0,1280,800,GL_RGBA,GL_UNSIGNED_BYTE,pixels);capture_camera("dungeon-jog",pixels);
    dungeon_move(app,60,-1);assert(io_app_game_action(app,2,0,0,0));
    assert(io_app_game_action(app,4,0,0,0));io_app_update(app,0.2f);
    assert(io_app_item_state(app,hero,&state));assert(state.anchor.z>0.3f);
    for(int i=0;i<100;i++)io_app_update(app,1.f/60.f);
    dungeon_move(app,300,1);
    assert(io_app_item_state(app,hero,&state));assert(state.anchor.y>50.13f && state.anchor.y<50.17f);
    assert(io_app_game_action(app,12,1,0,0));dungeon_move(app,260,1);
    /* The player, not an interior rig, chooses the low doorway angle. */
    assert(io_app_game_action(app,8,0,-0.785398163f,-0.528213f));
    assert(io_app_item_state(app,hero,&state));assert(state.anchor.y>45.8f && state.anchor.y<46.2f);
    assert(io_app_game_action(app,2,0,0,0));assert(io_app_game_action(app,12,0,0,0));
    for(int i=0;i<30;i++)io_app_update(app,1.f/60.f);
    assert(io_app_game_view(app,&game));
    bool crouched=false;for(uint32_t i=0;i<game.line_count;i++)if(strstr(game.lines[i],"CROUCHED"))crouched=true;
    assert(crouched);
    for(int stage=0;stage<2;stage++){
        if(stage)dungeon_move(app,360,1);
        IoFrame frame;assert(io_app_frame(app,1,&frame));assert(frame.joint_count==65);
        assert(renderer_draw(&r,&frame));glReadPixels(0,0,1280,800,GL_RGBA,GL_UNSIGNED_BYTE,pixels);
        size_t red=0;for(size_t p=0;p<1280*800*4;p+=4)if(pixels[p]>pixels[p+1]*1.4f && pixels[p]>pixels[p+2]*1.4f && pixels[p]>90)red++;
        fprintf(stderr,"Dungeon %s: %zu character pixels\n",stage?"chamber":"passage",red);
        capture_camera(stage?"dungeon-chamber":"dungeon-passage",pixels);
        assert(red>100);
        if(stage){
            assert(io_app_game_action(app,2,0,0,0));
            assert(!io_app_game_action(app,13,0,0,0));
            for(int i=0;i<360;i++)io_app_update(app,1.f/60.f);
            assert(io_app_frame(app,1,&frame));float following[16];memcpy(following,frame.clip_from_world,sizeof(following));
            assert(renderer_draw(&r,&frame));glReadPixels(0,0,1280,800,GL_RGBA,GL_UNSIGNED_BYTE,pixels);
            capture_camera("dungeon-free-orbit",pixels);
            IoItemState before,after;assert(io_app_item_state(app,hero,&before));
            dungeon_move(app,30,-1);
            assert(io_app_item_state(app,hero,&after));assert(fabsf(after.anchor.y-before.anchor.y)>0.1f);
            assert(io_app_frame(app,1,&frame));
            assert(memcmp(following,frame.clip_from_world,sizeof(following))!=0);
            assert(io_app_game_action(app,2,0,0,0));assert(io_app_game_action(app,8,0,0.4f,0.1f));
            for(int i=0;i<120;i++)io_app_update(app,1.f/60.f);
            assert(io_app_frame(app,1,&frame));assert(memcmp(following,frame.clip_from_world,sizeof(following))!=0);
            assert(renderer_draw(&r,&frame));
        }
        if(!stage){
            for(int orbit=0;orbit<72;orbit++){
                assert(io_app_dispatch(app,(IoAction){.kind=IO_ACTION_ORBIT,.x=6.283185307f/72.f}));
                if(orbit%18==0)assert(io_app_dispatch(app,(IoAction){.kind=IO_ACTION_ZOOM,.x=(orbit/18)%2?4.f:-4.f}));
                io_app_update(app,1.f/60.f);
                assert(io_app_frame(app,1,&frame));
                const float *m=frame.clip_from_world;
                float k2=m[3]*m[3]+m[7]*m[7]+m[11]*m[11];
                float distance=1.f/sqrtf(k2);
                assert(isfinite(distance) && distance>0.05f && distance<=35.01f);
                assert(renderer_draw(&r,&frame));
            }
            assert(io_app_dispatch(app,(IoAction){.kind=IO_ACTION_ZOOM,.x=-20.f}));
            for(int i=0;i<120;i++)io_app_update(app,1.f/60.f);
            assert(io_app_frame(app,1,&frame));
            float wide_k2=frame.clip_from_world[3]*frame.clip_from_world[3]+frame.clip_from_world[7]*frame.clip_from_world[7]+frame.clip_from_world[11]*frame.clip_from_world[11];
            assert(isfinite(wide_k2) && wide_k2>0.f);
            assert(renderer_draw(&r,&frame));
            glReadPixels(0,0,1280,800,GL_RGBA,GL_UNSIGNED_BYTE,pixels);
            capture_camera("dungeon-passage-wide",pixels);
            assert(io_app_dispatch(app,(IoAction){.kind=IO_ACTION_ZOOM,.x=10.f}));
        }
    }
    assert(io_app_game_view(app,&game));
    bool standing=false;for(uint32_t i=0;i<game.line_count;i++)if(strstr(game.lines[i],"STANDING"))standing=true;
    assert(standing);
    free(pixels);renderer_destroy(&r);io_app_free(app);
    puts("PASS: jogging, native jump, physical lintel blocking, crouch clearance, auto-stand and visible interior character");
    puts("PASS: user-directed passage orbit, collision-limited zoom and room-independent follow");
}

static void editor_click(EditorUi *ui,IoApp *app,int x,int y){
    SDL_Event event={.type=SDL_MOUSEBUTTONDOWN};event.button.button=SDL_BUTTON_LEFT;event.button.x=x;event.button.y=y;
    assert(editor_ui_event(ui,app,&event));event.type=SDL_MOUSEBUTTONUP;assert(editor_ui_event(ui,app,&event));
}
static void check_editor(void){
    assert(sizeof(IoEditorView)==1028);
    IoApp *app=io_app_new();assert(app);EditorUi ui;assert(editor_ui_init(&ui,app));
    editor_ui_refresh(&ui,app,1280,800);assert(ui.view.enabled&&!ui.view.playing&&!ui.view.dirty);
    assert(!io_app_editor_enable(app));
    IoGameView game;assert(io_app_game_view(app,&game));IoItemState before,after;
    assert(io_app_item_state(app,game.selected_item,&before));
    assert(!io_app_game_action(app,2,0,0,1));io_app_update(app,0.25f);
    assert(io_app_item_state(app,game.selected_item,&after));assert(memcmp(&before.anchor,&after.anchor,sizeof(IoVec3))==0);
    editor_click(&ui,app,50,190);assert(ui.view.selected==1);
    IoFrame frame;assert(io_app_frame(app,1,&frame));assert(frame.grid_vertex_count==44);
    assert(ui.view.envelope);
    editor_click(&ui,app,1140,85);assert(!ui.editing);
    assert(!io_app_editor_command(app,IO_EDIT_SET,0,0,7));
    assert(!io_app_editor_command(app,IO_EDIT_CAPTURE,0,0,0));
    editor_click(&ui,app,1140,178);assert(ui.editing);
    SDL_Event text={.type=SDL_TEXTINPUT};snprintf(text.text.text,sizeof(text.text.text),"0.8");assert(editor_ui_event(&ui,app,&text));
    SDL_Event key={.type=SDL_KEYDOWN};key.key.keysym.sym=SDLK_w;assert(editor_ui_event(&ui,app,&key));
    key.key.keysym.sym=SDLK_RETURN;assert(editor_ui_event(&ui,app,&key));assert(!ui.editing&&ui.view.values[3]==.8f&&ui.view.dirty);
    assert(!io_app_editor_command(app,IO_EDIT_SET,0,1,100));
    editor_click(&ui,app,40,90);assert(ui.view.values[3]==.7f&&!ui.view.dirty);
    editor_click(&ui,app,140,90);assert(ui.view.values[3]==.8f&&ui.view.dirty);
    Renderer r;assert(renderer_init(&r));assert(renderer_resize(&r,1280,800,1280,800));
    unsigned char *pixels=malloc(1280*800*4);assert(pixels);
    for(int preview=0;preview<2;preview++){
        if(preview)editor_click(&ui,app,50,500);
        assert(io_app_frame(app,1,&frame));assert(renderer_draw(&r,&frame));
        if(!preview)assert(renderer_draw_guides(&r,&frame));
        assert(editor_ui_draw(&ui));
        glReadPixels(0,0,1280,800,GL_RGBA,GL_UNSIGNED_BYTE,pixels);
        size_t gold=0;for(size_t p=0;p<1280*800*4;p+=4)if(pixels[p]>200&&pixels[p+1]>130&&pixels[p+2]<100)gold++;
        assert(gold>1000);capture_camera(preview?"editor-rig":"editor-volume",pixels);
    }
    editor_click(&ui,app,50,610);assert(ui.view.trajectory&&!ui.view.preview);
    assert(io_app_frame(app,1,&frame));assert(frame.guide_ends[0]==44&&frame.guide_ends[1]>44&&frame.guide_ends[2]==frame.grid_vertex_count);
    assert(frame.grid_vertex_count<3000);
    assert(renderer_draw(&r,&frame));assert(renderer_draw_guides(&r,&frame));assert(editor_ui_draw(&ui));
    glReadPixels(0,0,1280,800,GL_RGBA,GL_UNSIGNED_BYTE,pixels);
    size_t cyan=0,orange=0;
    for(size_t p=0;p<1280*800*4;p+=4){
        if(pixels[p]<100&&pixels[p+1]>150&&pixels[p+2]>180)cyan++;
        if(pixels[p]>180&&pixels[p+1]>90&&pixels[p+1]<165&&pixels[p+2]<100)orange++;
    }
    assert(cyan>100&&orange>100);capture_camera("editor-trajectory",pixels);
    float fov=ui.view.values[15];editor_click(&ui,app,1254,369);assert(ui.view.values[15]==fov+2);
    assert(io_app_frame(app,1,&frame));assert(renderer_draw(&r,&frame));assert(renderer_draw_guides(&r,&frame));
    IoFrame bad=frame;bad.guide_ends[1]=frame.grid_vertex_count+2;assert(!renderer_draw_guides(&r,&bad));
    key.key.keysym.sym=SDLK_F5;assert(editor_ui_event(&ui,app,&key));assert(ui.view.playing);
    assert(!io_app_editor_command(app,IO_EDIT_SET,0,0,5));
    assert(io_app_game_action(app,4,0,0,0));io_app_update(app,0.2f);
    assert(io_app_item_state(app,game.selected_item,&after));assert(after.anchor.z>before.anchor.z+.2f);
    assert(editor_ui_event(&ui,app,&key));assert(!ui.view.playing&&ui.view.values[3]==.8f&&ui.view.dirty);
    assert(io_app_item_state(app,game.selected_item,&after));assert(memcmp(&before.anchor,&after.anchor,sizeof(IoVec3))==0);
    SDL_Event click={.type=SDL_MOUSEBUTTONDOWN};click.button.x=600;click.button.y=400;click.button.button=SDL_BUTTON_LEFT;
    assert(!editor_ui_event(&ui,app,&click));
    editor_click(&ui,app,50,255);assert(ui.view.portal && ui.view.selected==3 && ui.view.values[6]==3);
    assert(io_app_editor_command(app,IO_EDIT_SET,0,6,2.8f));assert(io_app_editor_command(app,IO_EDIT_COMMIT,0,0,0));
    assert(!io_app_editor_command(app,IO_EDIT_SET,0,6,100));
    assert(io_app_frame(app,1,&frame));assert(frame.grid_vertex_count==10);
    assert(renderer_draw(&r,&frame));assert(renderer_draw_guides(&r,&frame));editor_ui_refresh(&ui,app,1280,800);assert(editor_ui_draw(&ui));
    glReadPixels(0,0,1280,800,GL_RGBA,GL_UNSIGNED_BYTE,pixels);capture_camera("editor-portal",pixels);
    editor_click(&ui,app,40,90);assert(ui.view.values[6]==3);
    editor_click(&ui,app,50,190);editor_click(&ui,app,50,645);assert(ui.view.portal&&ui.view.count==6);
    editor_click(&ui,app,40,90);assert(ui.view.count==5);
    free(pixels);editor_ui_destroy(&ui);renderer_destroy(&r);io_app_free(app);
    puts("PASS: native editor controls, portal authoring/validation/undo, trajectory guides and isolated play/stop");
}

static void check_attention(void){
    IoApp *app=io_app_new();assert(app);
    Renderer r;assert(renderer_init(&r));assert(renderer_resize(&r,1280,800,1280,800));
    unsigned char *pixels=malloc(1280*800*4);assert(pixels);
    IoGameView game;assert(io_app_game_view(app,&game));assert(game.meter_count==2);
    assert(strcmp(game.meters[0].label,"RUNNER ATTENTION")==0);
    assert(strcmp(game.meters[1].label,"FOCUS")==0);
    float peak=0.f,peak_focus=0.f,previous=0.f;
    bool hidden=false,partial=false,decayed=false;
    for(int step=0;step<1800;step++){
        io_app_update(app,1.f/60.f);
        IoFrame frame;assert(io_app_frame(app,1,&frame));
        assert(io_app_game_view(app,&game));assert(game.meter_count==2);
        for(unsigned int i=0;i<2;i++)assert(isfinite(game.meters[i].value)&&game.meters[i].value>=0.f&&game.meters[i].value<=1.f);
        assert(game.meters[0].x==game.meters[1].x);
        assert(fabsf(game.meters[1].y-game.meters[0].y-game.meters[0].width*.24f)<.001f);
        float focus=game.meters[1].value,attention=game.meters[0].value;
        peak_focus=fmaxf(peak_focus,focus);
        assert(attention<=peak_focus+1e-6f);
        hidden|=focus==0.f;partial|=focus>0.f&&focus<.99f;
        decayed|=focus==0.f&&attention<previous&&attention>.1f;
        previous=attention;peak=fmaxf(peak,attention);
        if(step==60||step==240||step==600){
            assert(renderer_draw(&r,&frame));hud_set_game(&r.hud,&game);assert(renderer_draw_hud(&r));
            glReadPixels(0,0,1280,800,GL_RGBA,GL_UNSIGNED_BYTE,pixels);
            capture_camera(step==60?"attention-visible":step==240?"attention-covered":"attention-return",pixels);
            const IoWorldMeter *meter=&game.meters[0];
            int x=(int)(meter->x-meter->width*.42f),y=799-(int)(meter->y+meter->width*.175f);
            assert(x>=0&&x<1280&&y>=0&&y<800);
            const unsigned char *pixel=pixels+(y*1280+x)*4;
            if(attention>.05f)assert(pixel[0]>200 && pixel[1]>150 && pixel[2]<100);
        }
    }
    assert(hidden&&partial&&decayed&&peak>.5f);
    IoGameView before=game;
    // The target is now the camera pivot; yaw alone keeps its head centered.
    assert(io_app_game_action(app,8,0,.3f,.2f));io_app_update(app,0.f);
    assert(io_app_game_view(app,&game));
    assert(fabsf(before.meters[0].x-game.meters[0].x)>1.f||fabsf(before.meters[0].y-game.meters[0].y)>1.f);
    free(pixels);renderer_destroy(&r);io_app_free(app);
    puts("PASS: NPC awareness of player, directional focus gauges, decay and orbit projection");
}

static void check_attention_controls(IoApp *app,Renderer *r){
    AttentionUi ui;assert(attention_ui_init(&ui));
    attention_ui_refresh(&ui,app,1280,800);
    assert(ui.available&&ui.applied.has_pursuit&&ui.applied.fov_degrees==240);
    assert(!io_app_tune_attention(app,ui.applied.observer,ui.applied.target,NAN,.5f));
    SDL_Event e={0};e.type=SDL_MOUSEBUTTONDOWN;e.button.button=SDL_BUTTON_LEFT;
    e.button.x=192;e.button.y=640;
    assert(attention_ui_event(&ui,&e)&&ui.drag==1);
    e.type=SDL_MOUSEMOTION;e.motion.x=900;e.motion.y=640;
    assert(attention_ui_event(&ui,&e));
    assert(ui.desired.fov_degrees==360);
    e.type=SDL_MOUSEBUTTONUP;e.button.button=SDL_BUTTON_LEFT;e.button.x=900;e.button.y=640;
    assert(attention_ui_event(&ui,&e)&&!ui.drag);
    attention_ui_submit(&ui,app);attention_ui_refresh(&ui,app,1280,800);
    assert(ui.applied.fov_degrees==360&&!ui.pending);
    e.type=SDL_MOUSEBUTTONDOWN;e.button.x=192;e.button.y=692;
    assert(attention_ui_event(&ui,&e)&&ui.drag==2);
    attention_ui_submit(&ui,app);attention_ui_refresh(&ui,app,1280,800);
    assert(fabsf(ui.applied.notice_attention-.51f)<.001f);
    e.type=SDL_WINDOWEVENT;e.window.event=SDL_WINDOWEVENT_FOCUS_LOST;
    attention_ui_event(&ui,&e);assert(!ui.drag);
    e=(SDL_Event){0};e.type=SDL_KEYDOWN;e.key.keysym.sym=SDLK_F2;
    assert(attention_ui_event(&ui,&e)&&ui.hidden);
    assert(attention_ui_event(&ui,&e)&&!ui.hidden);
    e=(SDL_Event){0};e.type=SDL_MOUSEBUTTONDOWN;e.button.button=SDL_BUTTON_LEFT;
    e.button.x=90;e.button.y=768;
    assert(attention_ui_event(&ui,&e));attention_ui_submit(&ui,app);
    attention_ui_refresh(&ui,app,1280,800);
    assert(ui.applied.fov_degrees==240&&fabsf(ui.applied.notice_attention-.18f)<.001f);
    IoFrame frame;assert(io_app_frame(app,1,&frame));assert(renderer_draw(r,&frame));
    assert(attention_ui_draw(&ui));
    unsigned char *pixels=malloc(1280*800*4);assert(pixels);
    glReadPixels(0,0,1280,800,GL_RGBA,GL_UNSIGNED_BYTE,pixels);
    capture_camera("attention-sliders",pixels);free(pixels);
    assert(glGetError()==GL_NO_ERROR);
    attention_ui_destroy(&ui);
    puts("PASS: live vision sliders, clamped dragging, focus release, hide and reset");
}

static void check_pursuit(void){
    IoApp *app=io_app_new();assert(app);
    Renderer r;assert(renderer_init(&r));assert(renderer_resize(&r,1280,800,1280,800));
    check_attention_controls(app,&r);
    unsigned char *pixels=malloc(1280*800*4);assert(pixels);
    bool patrol=false,chase=false,tagged=false;
    IoGameView game;
    for(int step=0;step<2400;step++){
        io_app_update(app,1.f/60.f);
        assert(io_app_game_view(app,&game));assert(game.meter_count==2);
        assert(strstr(game.lines[0],"CAT AND MOUSE"));
        const char *capture=NULL;
        for(unsigned int i=0;i<game.line_count;i++){
            if(!patrol&&strstr(game.lines[i],": PATROL")){patrol=true;capture="cat-mouse-patrol";}
            if(!chase&&strstr(game.lines[i],": CHASE")){chase=true;capture="cat-mouse-chase";}
            if(!tagged&&strstr(game.lines[i],": TAGGED")){tagged=true;capture="cat-mouse-tagged";}
        }
        if(capture){
            IoFrame frame;assert(io_app_frame(app,1,&frame));
            assert(renderer_draw(&r,&frame));hud_set_game(&r.hud,&game);assert(renderer_draw_hud(&r));
            glReadPixels(0,0,1280,800,GL_RGBA,GL_UNSIGNED_BYTE,pixels);
            capture_camera(capture,pixels);
        }
        if(tagged)break;
    }
    assert(patrol&&chase&&tagged);
    IoItemState before,after;
    assert(io_app_item_state(app,game.selected_item,&before));
    assert(io_app_game_action(app,2,0,1.f,0.f));
    for(int step=0;step<30;step++)io_app_update(app,1.f/60.f);
    assert(io_app_item_state(app,game.selected_item,&after));
    assert(hypotf(after.anchor.x-before.anchor.x,after.anchor.y-before.anchor.y)>.2f);
    free(pixels);renderer_destroy(&r);io_app_free(app);
    puts("PASS: cat-and-mouse arena renders, patrol/chase/tag HUD and player escape movement");
}

static void check_reaction_meters(void){
    IoApp *app=io_app_new();assert(app);
    Renderer r;assert(renderer_init(&r));assert(renderer_resize(&r,1280,800,1280,800));
    unsigned char *pixels=malloc(1280*800*4);assert(pixels);
    for(int i=0;i<600;i++)io_app_update(app,1.f/60.f);
    IoFrame frame;assert(io_app_frame(app,1,&frame));
    IoGameView game;assert(io_app_game_view(app,&game));assert(game.meter_count==6);
    for(unsigned int i=0;i<6;i++)assert(game.meters[i].width>5.f);
    assert(game.meters[0].label[0]=='E'&&game.meters[1].label[0]=='A');
    assert(renderer_draw(&r,&frame));hud_set_game(&r.hud,&game);assert(renderer_draw_hud(&r));
    glReadPixels(0,0,1280,800,GL_RGBA,GL_UNSIGNED_BYTE,pixels);capture_camera("reactions",pixels);
    float original=game.meters[0].width;
    assert(io_app_dispatch(app,(IoAction){IO_ACTION_ZOOM,-4.f,0.f,0}));io_app_update(app,0.f);
    assert(io_app_game_view(app,&game));assert(game.meter_count==6);
    assert(game.meters[0].width<original);
    assert(io_app_frame(app,1,&frame));assert(renderer_draw(&r,&frame));
    hud_set_game(&r.hud,&game);assert(renderer_draw_hud(&r));
    glReadPixels(0,0,1280,800,GL_RGBA,GL_UNSIGNED_BYTE,pixels);capture_camera("reactions-wide",pixels);
    free(pixels);renderer_destroy(&r);io_app_free(app);
    puts("PASS: per-NPC evidence/attention meters scale with world zoom");
}

int main(int argc,char **argv) {
    bool variants=argc==2 && strcmp(argv[1],"--variants")==0;
    bool game=argc==2 && strcmp(argv[1],"--game")==0;
    bool camera=argc==2 && strcmp(argv[1],"--camera")==0;
    bool dungeon=argc==2 && strcmp(argv[1],"--dungeon")==0;
    bool editor=argc==2 && strcmp(argv[1],"--editor")==0;
    bool attention=argc==2 && strcmp(argv[1],"--attention")==0;
    bool pursuit=argc==2 && strcmp(argv[1],"--pursuit")==0;
    bool reactions=argc==2 && strcmp(argv[1],"--reactions")==0;
    if(variants)assert(setenv("IO_SCENE","assets/street-kit/variants-demo.json",1)==0);
    if(game)assert(setenv("IO_SCENE","assets/game/encounter.json",1)==0);
    if(camera)assert(setenv("IO_SCENE","assets/camera/zoom.json",1)==0);
    if(dungeon||editor)assert(setenv("IO_SCENE","assets/dungeon/entry.json",1)==0);
    if(attention)assert(setenv("IO_SCENE","assets/dungeon/attention.json",1)==0);
    if(pursuit)assert(setenv("IO_SCENE","assets/dungeon/cat-mouse.json",1)==0);
    if(reactions)assert(setenv("IO_SCENE","assets/dungeon/reactions.json",1)==0);
    assert(SDL_Init(SDL_INIT_VIDEO)==0);
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_MAJOR_VERSION,4);
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_MINOR_VERSION,1);
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_PROFILE_MASK,SDL_GL_CONTEXT_PROFILE_CORE);
    SDL_GL_SetAttribute(SDL_GL_STENCIL_SIZE,8);
    SDL_GL_SetAttribute(SDL_GL_DEPTH_SIZE,24);
    if(camera){SDL_GL_SetAttribute(SDL_GL_MULTISAMPLEBUFFERS,1);SDL_GL_SetAttribute(SDL_GL_MULTISAMPLESAMPLES,4);}
    SDL_Window *window=SDL_CreateWindow("io buffer tests",0,0,1280,800,SDL_WINDOW_OPENGL|SDL_WINDOW_HIDDEN);
    assert(window);
    SDL_GLContext context=SDL_GL_CreateContext(window);
    assert(context && SDL_GL_MakeCurrent(window,context)==0);
    if(!camera && !dungeon && !editor && !attention && !pursuit)check_hud();
    if(reactions)check_reaction_meters();
    else if(pursuit)check_pursuit();
    else if(attention)check_attention();
    else if(editor)check_editor();
    else if(dungeon)check_dungeon();
    else if(camera)check_camera();
    else if(game)check_game_feedback();
    else if(variants)check_variants();
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
