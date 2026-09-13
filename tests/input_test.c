#include <assert.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "../csrc/input.h"

_Static_assert(sizeof(IoVertex)==84,"vertex ABI");
_Static_assert(offsetof(IoVertex,joints)==52,"joint attribute ABI");
static IoAction key_action(SDL_Keycode key){
    SDL_Event event={0};event.type=SDL_KEYDOWN;event.key.keysym.sym=key;
    IoAction action;assert(input_translate(&event,&action)==INPUT_ACTION);return action;
}
int main(void){
    IoApp *app=io_app_new();assert(app);
    IoFrame frame;assert(io_app_frame(app,1,&frame));
    assert(frame.world_items==39 && frame.instance_count>0 && frame.joint_count==14);
    uint64_t runner=0;
    for(size_t i=0;i<frame.instance_count;i++){
        IoModel model;assert(io_model_get(frame.instances[i].model_id,&model));
        assert(model.vertex_count>0 && model.index_count%3==0);
        bool skinned=false;
        for(size_t v=0;v<model.vertex_count;v++){
            const IoVertex *vertex=&model.vertices[v];
            if(vertex->weights[0]+vertex->weights[1]+vertex->weights[2]+vertex->weights[3]>0.f){
                skinned=true;
                for(int k=0;k<4;k++)assert(frame.instances[i].joint_offset+vertex->joints[k]<frame.joint_count);
            }
        }
        if(skinned)runner=frame.instances[i].item_id;
    }
    assert(runner);
    assert(!io_app_set_visual_state(NULL,runner,"default"));
    assert(!io_app_set_visual_state(app,runner,NULL));
    assert(!io_app_set_visual_state(app,runner,"\xff"));
    assert(!io_app_set_visual_state(app,runner,"missing"));
    assert(!io_app_set_visual_state(app,runner,"default"));
    float bones[14*16];memcpy(bones,frame.joint_matrices,sizeof(bones));
    IoItemState before,after;assert(io_app_item_state(app,runner,&before));
    io_app_update(app,0.2f);
    assert(io_app_frame(app,1,&frame));
    assert(memcmp(bones,frame.joint_matrices,sizeof(bones))!=0);
    assert(io_app_item_state(app,runner,&after));
    assert(after.simulated_ticks>before.simulated_ticks);
    assert(after.anchor.x!=before.anchor.x || after.anchor.y!=before.anchor.y);
    assert(io_app_dispatch(app,key_action(SDLK_n)));
    IoCameraId second=io_app_active_camera(app);assert(second==2);
    assert(io_app_set_camera_target(app,1,(IoVec3){9000,9000,0}));
    assert(io_app_set_camera_target(app,second,(IoVec3){9000,9000,0}));
    io_app_update(app,0.2f);before=after;
    assert(io_app_item_state(app,runner,&after));assert(after.simulated_ticks==before.simulated_ticks);
    assert(io_app_set_camera_target(app,second,after.anchor));
    io_app_update(app,0.2f);assert(io_app_item_state(app,runner,&after));
    assert(after.simulated_ticks>before.simulated_ticks && after.health==before.health);
    SDL_Event event={0};event.type=SDL_MOUSEMOTION;event.motion.xrel=100;event.motion.yrel=30;
    IoAction action;assert(input_translate(&event,&action)==INPUT_NONE);
    event.motion.state=SDL_BUTTON_LMASK;assert(input_translate(&event,&action)==INPUT_ACTION);
    assert(io_app_dispatch(app,action));
    assert(io_app_dispatch(app,key_action(SDLK_g)));
    assert(io_app_frame(app,second,&frame));assert(frame.grid_vertex_count>0);
    assert(io_app_dispatch(app,key_action(SDLK_r)));
    assert(io_app_dispatch(app,key_action(SDLK_f)));
    assert(io_app_dispatch(app,key_action(SDLK_TAB)));
    IoModel invalid;assert(!io_model_get(999,&invalid) && invalid.vertices==NULL);
    assert(!io_app_frame(app,0,&frame) && frame.joint_matrices==NULL && frame.joint_count==0);
    assert(!io_app_frame(app,1,NULL));assert(!io_app_select_camera(app,0));
    assert(!io_app_set_render_distance(app,1,NAN));
    assert(!io_app_dispatch(app,(IoAction){.kind=UINT32_MAX}));
    io_app_free(app);io_app_free(NULL);
    puts("PASS: materials/skin ABI, movement, animation, cameras, and paused state retention");
    return 0;
}
