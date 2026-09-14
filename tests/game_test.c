#define _POSIX_C_SOURCE 200809L
#include "../csrc/input.h"
#include <assert.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static bool contains(const IoGameView *view,const char *text){
    for(uint32_t i=0;i<view->line_count;i++)if(strstr(view->lines[i],text))return true;
    return false;
}
int main(void){
    assert(setenv("IO_SCENE","assets/game/encounter.json",1)==0);
    IoApp *app=io_app_new();assert(app);
    IoGameView view;
    assert(!io_app_game_view(NULL,&view) && !view.enabled);
    assert(!io_app_game_view(app,NULL));
    assert(!io_app_game_action(NULL,1,0,0,0));
    assert(!io_app_game_action(app,999,0,0,0));
    assert(!io_app_game_action(app,2,0,NAN,0));
    assert(io_app_game_view(app,&view) && view.enabled && view.free_movement);
    assert(contains(&view,"EXPLORATION"));
    SDL_Event event={0};event.type=SDL_KEYDOWN;event.key.keysym.sym=SDLK_w;
    IoGameAction action;
    assert(input_game_translate(&event,true,&action)==INPUT_CONSUMED);
    assert(input_game_translate(&event,false,&action)==INPUT_NONE);
    event.key.keysym.sym=SDLK_2;
    assert(input_game_translate(&event,true,&action)==INPUT_ACTION && action.kind==3 && action.slot==1);
    event.key.repeat=1;assert(input_game_translate(&event,true,&action)==INPUT_CONSUMED);
    event.key.repeat=0;event.type=SDL_KEYUP;
    assert(input_game_translate(&event,true,&action)==INPUT_CONSUMED);
    IoItemState before,after;assert(io_app_item_state(app,2,&before));
    assert(io_app_game_action(app,2,0,1,0));io_app_update(app,0.125f);
    assert(io_app_item_state(app,2,&after) && after.anchor.x>before.anchor.x);
    assert(io_app_game_action(app,2,0,0,0));
    assert(io_app_game_action(app,1,0,0,0));
    for(int i=0;i<24;i++)io_app_update(app,0.125f);
    assert(io_app_game_view(app,&view) && !view.free_movement && contains(&view,"MOVEMENT LOCKED"));
    assert(view.selected_item==2);
    assert(!io_app_game_action(app,2,0,1,0));
    assert(io_app_item_state(app,3,&before));
    assert(io_app_game_action(app,3,1,0,0));
    assert(io_app_item_state(app,3,&after) && after.health==before.health);
    assert(io_app_game_view(app,&view) && contains(&view,"SELECTED") && view.projectile_count==0);
    IoFrame frame;assert(io_app_frame(app,1,&frame));
    IoVec3 center={after.anchor.x+after.size.x*0.5f,after.anchor.y+after.size.y*0.5f,after.anchor.z+after.size.z*0.5f};
    const float *m=frame.clip_from_world;
    float x=(m[0]*center.x+m[4]*center.y+m[8]*center.z+m[12]+1.f)*640.f;
    float y=(1.f-(m[1]*center.x+m[5]*center.y+m[9]*center.z+m[13]))*400.f;
    assert(!io_app_game_action(app,7,0,-10,-10));
    assert(io_app_game_action(app,7,0,x,y));
    assert(io_app_game_view(app,&view) && view.projectile_count>0 && contains(&view,"SPELL IN FLIGHT"));
    uint64_t target=contains(&view,"TARGET GUARD")?3:4;
    assert(io_app_item_state(app,target,&before));
    for(int i=0;i<20;i++){
        io_app_update(app,0.05f);assert(io_app_item_state(app,target,&after));
        if(after.health<before.health)break;
    }
    assert(after.health==before.health-8);
    assert(io_app_game_view(app,&view) && view.damage_count>0 && view.damage[0].amount==8);
    for(int i=0;i<30;i++){
        io_app_update(app,0.125f);assert(io_app_game_view(app,&view));
        if(view.free_movement)break;
    }
    assert(io_app_game_view(app,&view) && view.free_movement && contains(&view,"ROUND 2"));
    io_app_free(app);
    puts("PASS: game C ABI, WASD isolation, attack menu, health, NPC turns, and next round");
}
