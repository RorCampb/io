#include "input.h"

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
