#define _POSIX_C_SOURCE 200809L
#include "../csrc/input.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>
static bool contains(const IoGameView *v,const char *s){for(uint32_t i=0;i<v->line_count;i++)if(strstr(v->lines[i],s))return true;return false;}
int main(void){
    assert(setenv("IO_SCENE","assets/villages/world.json",1)==0);
    IoApp *app=io_app_new();assert(app);IoGameView view;
    assert(io_app_game_view(app,&view) && view.enabled && view.free_movement && contains(&view,"WANDERING"));
    SDL_Event e={0};e.type=SDL_KEYDOWN;IoGameAction action;
    SDL_Keycode keys[]={SDLK_e,SDLK_r,SDLK_m};
    for(uint32_t i=0;i<3;i++){e.key.keysym.sym=keys[i];assert(input_game_translate(&e,true,&action)==INPUT_ACTION && action.kind==9+i);}
    assert(io_app_game_action(app,9,0,0,0));assert(io_app_game_view(app,&view) && contains(&view,"CORA"));
    assert(io_app_game_action(app,10,0,0,0));assert(io_app_game_view(app,&view) && contains(&view,"PARTY 2"));
    assert(contains(&view,"I am with you."));
    IoFrame before,after;assert(io_app_frame(app,1,&before));float scale=before.clip_from_world[0];
    assert(io_app_game_action(app,11,0,0,0));assert(io_app_frame(app,1,&after));assert(after.clip_from_world[0]<scale);
    assert(io_app_game_action(app,11,0,0,0));
    assert(io_app_dispatch(app,(IoAction){.kind=IO_ACTION_ZOOM,.x=-100.f}));
    assert(io_app_frame(app,1,&after));
    assert(after.instance_count>700 && after.render_distance>before.render_distance);
    /* Map toggle restores the local zoom without changing the simulation range. */
    assert(io_app_game_action(app,11,0,0,0));
    assert(io_app_game_action(app,11,0,0,0));
    IoItemState start,end;assert(io_app_item_state(app,view.selected_item,&start));
    assert(io_app_game_action(app,2,0,1,0));io_app_update(app,0.125f);
    assert(io_app_item_state(app,view.selected_item,&end) && (end.anchor.x!=start.anchor.x || end.anchor.y!=start.anchor.y));
    assert(io_app_game_action(app,2,0,0,0));
    /* Orbit continuously while W stays held; movement must stay screen-up. */
    for(int i=0;i<20;i++){
        assert(io_app_game_action(app,8,0,0.07f,0.005f));
        assert(io_app_frame(app,1,&after));
        float rx=after.clip_from_world[0],ry=after.clip_from_world[4];
        float ux=after.clip_from_world[1],uy=after.clip_from_world[5];
        assert(io_app_item_state(app,view.selected_item,&start));
        assert(io_app_game_action(app,2,0,0,1));io_app_update(app,1.f/60.f);
        assert(io_app_item_state(app,view.selected_item,&end));
        float dx=end.anchor.x-start.anchor.x,dy=end.anchor.y-start.anchor.y;
        assert(fabsf(dx*rx+dy*ry)<0.00001f);
        assert(dx*ux+dy*uy>0.f);
        assert(hypotf(dx,dy)<0.1f);
    }
    assert(io_app_game_action(app,2,0,0,0));io_app_free(app);
    Uint8 held[SDL_NUM_SCANCODES]={0};held[SDL_SCANCODE_RIGHT]=1;held[SDL_SCANCODE_W]=1;
    assert(input_game_camera(held,true,0.05f,&action)==INPUT_ACTION && action.kind==8);
    assert(fabsf(action.x-0.06f)<0.00001f && action.y==0.f);
    assert(input_game_camera(held,false,0.05f,&action)==INPUT_NONE);
    assert(input_game_camera(held,true,NAN,&action)==INPUT_NONE);
    e.key.keysym.sym=SDLK_RIGHT;
    assert(input_game_translate(&e,true,&action)==INPUT_CONSUMED);
    puts("PASS: wandering UI, talk, recruit, map toggle and camera-relative movement");
}
