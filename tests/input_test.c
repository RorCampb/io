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
static void finger(InputTrackpad *pad,Uint32 type,SDL_FingerID id,float x,float y){
    SDL_Event e={.type=type};e.tfinger.touchId=42;e.tfinger.fingerId=id;e.tfinger.x=x;e.tfinger.y=y;
    assert(input_trackpad_event(pad,&e));
}
static void check_trackpad(void){
    InputTrackpad pad={0};IoAction actions[2];
    finger(&pad,SDL_FINGERDOWN,1,0.3f,0.5f);
    assert(input_trackpad_actions(&pad,actions)==0);
    finger(&pad,SDL_FINGERDOWN,2,0.7f,0.5f);
    assert(input_trackpad_actions(&pad,actions)==0);
    finger(&pad,SDL_FINGERMOTION,1,0.4f,0.4f);
    finger(&pad,SDL_FINGERMOTION,2,0.8f,0.4f);
    assert(input_trackpad_actions(&pad,actions)==1 && actions[0].kind==IO_ACTION_ORBIT);
    assert(fabsf(actions[0].x+0.6283185f)<1e-5f && fabsf(actions[0].y-0.3141593f)<1e-5f);
    assert(input_trackpad_actions(&pad,actions)==0);
    /* Finger spacing jitter during an orbit must never dolly the camera. */
    finger(&pad,SDL_FINGERMOTION,1,0.41f,0.4f);
    finger(&pad,SDL_FINGERMOTION,2,0.82f,0.4f);
    assert(input_trackpad_actions(&pad,actions)==1 && actions[0].kind==IO_ACTION_ORBIT);
    finger(&pad,SDL_FINGERUP,1,0.41f,0.4f);
    finger(&pad,SDL_FINGERUP,2,0.82f,0.4f);
    finger(&pad,SDL_FINGERDOWN,1,0.4f,0.4f);
    finger(&pad,SDL_FINGERDOWN,2,0.8f,0.4f);
    finger(&pad,SDL_FINGERMOTION,1,0.3f,0.4f);
    finger(&pad,SDL_FINGERMOTION,2,0.9f,0.4f);
    assert(input_trackpad_actions(&pad,actions)==1 && actions[0].kind==IO_ACTION_ZOOM);
    assert(fabsf(actions[0].x-logf(1.5f)/0.12f)<1e-5f);
    finger(&pad,SDL_FINGERDOWN,3,0.5f,0.4f);
    finger(&pad,SDL_FINGERMOTION,1,0.2f,0.5f);
    assert(input_trackpad_actions(&pad,actions)==0);
    finger(&pad,SDL_FINGERUP,3,0.5f,0.4f);
    assert(input_trackpad_actions(&pad,actions)==0);
    SDL_Event lost={.type=SDL_WINDOWEVENT};lost.window.event=SDL_WINDOWEVENT_FOCUS_LOST;
    assert(!input_trackpad_event(&pad,&lost));assert(!pad.active);
    finger(&pad,SDL_FINGERMOTION,1,0.1f,0.5f);
    assert(input_trackpad_actions(&pad,actions)==0);
    SDL_Event wheel={.type=SDL_MOUSEWHEEL};wheel.wheel.preciseY=0.25f;
    assert(input_translate(&wheel,&actions[0])==INPUT_ACTION && actions[0].kind==IO_ACTION_ZOOM && actions[0].x==0.25f);
    wheel.wheel.direction=SDL_MOUSEWHEEL_FLIPPED;
    assert(input_translate(&wheel,&actions[0])==INPUT_ACTION && actions[0].x==-0.25f);
    puts("PASS: two-finger orbit, coalesced pinch, extra fingers, focus reset and mouse wheel");
}
int main(void){
    check_trackpad();
    IoApp *app=io_app_new();assert(app);
    assert(io_app_tick_hz(NULL)==0);
    const char *configured_hz=getenv("IO_TICK_HZ");
    uint32_t expected_hz=configured_hz?(uint32_t)strtoul(configured_hz,NULL,10):30;
    assert(io_app_tick_hz(app)==expected_hz);
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
    app=io_app_new_realtime();assert(app);
    assert(io_app_tick_hz(app)==expected_hz);
    assert(io_app_frame(app,1,&frame));
    assert(frame.instance_count>0);
    IoInstance first=frame.instances[0];
    SDL_Delay(100);
    /* Background publications cannot invalidate the C-facing frame allocation. */
    assert(memcmp(&first,&frame.instances[0],sizeof(first))==0);
    IoWorkerStats worker={0};
    uint64_t timeout=SDL_GetTicks64()+3000;
    do {
        io_app_update(app,0.f);
        assert(io_app_worker_stats(app,&worker));
        if(worker.tick>0)break;
        SDL_Delay(1);
    } while(SDL_GetTicks64()<timeout);
    assert(worker.tick>0 && worker.status==1);
    assert(!io_app_worker_stats(app,NULL));
    assert(!io_app_worker_stats(NULL,&worker) && worker.tick==0);
    io_app_free(app);
    puts("PASS: materials/skin ABI, movement, animation, cameras, and paused state retention");
    return 0;
}
