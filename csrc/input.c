#include "input.h"
#include <math.h>

InputResult input_game_camera(const Uint8 *keys,bool focused,float seconds,IoGameAction *action){
    *action=(IoGameAction){0};
    if(!focused || !isfinite(seconds) || seconds<=0.f)return INPUT_NONE;
    float yaw=(float)(keys[SDL_SCANCODE_RIGHT]-keys[SDL_SCANCODE_LEFT]);
    float pitch=(float)(keys[SDL_SCANCODE_UP]-keys[SDL_SCANCODE_DOWN]);
    if(yaw==0.f && pitch==0.f)return INPUT_NONE;
    float dt=fminf(seconds,0.1f);
    *action=(IoGameAction){.kind=8,.x=yaw*1.2f*dt,.y=pitch*0.8f*dt};
    return INPUT_ACTION;
}

InputResult input_game_translate(const SDL_Event *event,bool enabled,IoGameAction *action){
    *action=(IoGameAction){0};
    if(!enabled)return INPUT_NONE;
    if(event->type==SDL_MOUSEBUTTONDOWN && event->button.button==SDL_BUTTON_LEFT){
        *action=(IoGameAction){.kind=7,.x=(float)event->button.x,.y=(float)event->button.y};return INPUT_ACTION;
    }
    if(event->type==SDL_MOUSEMOTION){
        if(event->motion.state&SDL_BUTTON_RMASK){
            *action=(IoGameAction){.kind=8,.x=-(float)event->motion.xrel*0.006f,.y=-(float)event->motion.yrel*0.006f};
            return INPUT_ACTION;
        }
        if(event->motion.state&SDL_BUTTON_LMASK)return INPUT_CONSUMED;
    }
    if(event->type!=SDL_KEYDOWN && event->type!=SDL_KEYUP)return INPUT_NONE;
    SDL_Keycode key=event->key.keysym.sym;
    switch(key){
        case SDLK_w: case SDLK_a: case SDLK_s: case SDLK_d:
        case SDLK_LEFT: case SDLK_RIGHT: case SDLK_UP: case SDLK_DOWN:
            return INPUT_CONSUMED; /* Held movement is sampled with focus checks each frame. */
        case SDLK_RETURN: action->kind=1;break;
        case SDLK_1: case SDLK_2: case SDLK_3: case SDLK_4:
            action->kind=3;action->slot=(uint32_t)(key-SDLK_1);break;
        case SDLK_SPACE: action->kind=4;break;
        case SDLK_t: action->kind=5;break;
        case SDLK_c: action->kind=6;break;
        case SDLK_e: action->kind=9;break;
        case SDLK_r: action->kind=10;break;
        case SDLK_m: action->kind=11;break;
        default:return INPUT_NONE;
    }
    return event->type==SDL_KEYDOWN && !event->key.repeat?INPUT_ACTION:INPUT_CONSUMED;
}

static InputResult translate_key(SDL_Keycode key, IoAction *action) {
    switch (key) {
        case SDLK_ESCAPE: return INPUT_QUIT;
        case SDLK_r: action->kind = IO_ACTION_RESET_VIEW; break;
        case SDLK_f: action->kind = IO_ACTION_FOLLOW; break;
        case SDLK_g: action->kind = IO_ACTION_GRID; break;
        case SDLK_n: action->kind = IO_ACTION_NEW_CAMERA; break;
        case SDLK_TAB: action->kind = IO_ACTION_NEXT_CAMERA; break;
        case SDLK_LEFTBRACKET: *action = (IoAction){.kind=IO_ACTION_DISTANCE,.x=-1.f}; break;
        case SDLK_RIGHTBRACKET: *action = (IoAction){.kind=IO_ACTION_DISTANCE,.x=1.f}; break;
        case SDLK_LEFT: *action = (IoAction){.kind=IO_ACTION_PAN,.x=80.f}; break;
        case SDLK_RIGHT: *action = (IoAction){.kind=IO_ACTION_PAN,.x=-80.f}; break;
        case SDLK_UP: *action = (IoAction){.kind=IO_ACTION_PAN,.y=80.f}; break;
        case SDLK_DOWN: *action = (IoAction){.kind=IO_ACTION_PAN,.y=-80.f}; break;
        case SDLK_EQUALS:
        case SDLK_PLUS:
        case SDLK_KP_PLUS:
            *action = (IoAction){.kind = IO_ACTION_ZOOM, .x = 1.0f}; break;
        case SDLK_MINUS:
        case SDLK_KP_MINUS:
            *action = (IoAction){.kind = IO_ACTION_ZOOM, .x = -1.0f}; break;
        case SDLK_q: *action = (IoAction){.kind = IO_ACTION_RESIZE_A, .delta = 1}; break;
        case SDLK_a: *action = (IoAction){.kind = IO_ACTION_RESIZE_A, .delta = -1}; break;
        case SDLK_w: *action = (IoAction){.kind = IO_ACTION_RESIZE_B, .delta = 1}; break;
        case SDLK_s: *action = (IoAction){.kind = IO_ACTION_RESIZE_B, .delta = -1}; break;
        case SDLK_e: *action = (IoAction){.kind = IO_ACTION_RESIZE_C, .delta = 1}; break;
        case SDLK_d: *action = (IoAction){.kind = IO_ACTION_RESIZE_C, .delta = -1}; break;
        default: return INPUT_NONE;
    }
    return INPUT_ACTION;
}

InputResult input_translate(const SDL_Event *event, IoAction *action) {
    *action = (IoAction){0};
    switch (event->type) {
        case SDL_QUIT:
            return INPUT_QUIT;
        case SDL_KEYDOWN:
            if(event->key.repeat && event->key.keysym.sym!=SDLK_LEFT && event->key.keysym.sym!=SDLK_RIGHT &&
                event->key.keysym.sym!=SDLK_UP && event->key.keysym.sym!=SDLK_DOWN) return INPUT_NONE;
            return translate_key(event->key.keysym.sym, action);
        case SDL_MOUSEMOTION:
            if(event->motion.state & (SDL_BUTTON_RMASK|SDL_BUTTON_MMASK)) {
                *action=(IoAction){.kind=IO_ACTION_PAN,.x=(float)event->motion.xrel,.y=(float)event->motion.yrel};
                return INPUT_ACTION;
            }
            if (!(event->motion.state & SDL_BUTTON_LMASK)) return INPUT_NONE;
            *action = (IoAction){
                .kind = IO_ACTION_ORBIT,
                .x = -(float)event->motion.xrel * 0.006f,
                .y = (float)event->motion.yrel * 0.006f
            };
            return INPUT_ACTION;
        case SDL_MOUSEWHEEL:
            *action = (IoAction){
                .kind = IO_ACTION_ZOOM,
                .x = event->wheel.preciseY * (event->wheel.direction == SDL_MOUSEWHEEL_FLIPPED ? -1.0f : 1.0f)
            };
            return INPUT_ACTION;
        default:
            return INPUT_NONE;
    }
}
