#ifndef IO_INPUT_H
#define IO_INPUT_H

#include <SDL2/SDL.h>
#include "io.h"

typedef enum InputResult {
    INPUT_NONE,
    INPUT_ACTION,
    INPUT_QUIT,
    INPUT_CONSUMED
} InputResult;

// Translate one platform event into intent without reading or mutating application state.
InputResult input_translate(const SDL_Event *event, IoAction *action);
InputResult input_game_translate(const SDL_Event *event,bool enabled,IoGameAction *action);
InputResult input_game_camera(const Uint8 *keys,bool focused,float seconds,IoGameAction *action);

typedef struct InputFinger {
    SDL_FingerID id;
    float x,y;
    bool down;
} InputFinger;
typedef enum InputGesture { GESTURE_PENDING, GESTURE_ORBIT, GESTURE_PINCH } InputGesture;
typedef struct InputTrackpad {
    SDL_TouchID device;
    InputFinger fingers[8];
    bool active,ready;
    float x,y,span;
    float origin_x,origin_y,origin_span;
    InputGesture gesture;
} InputTrackpad;

void input_trackpad_reset(InputTrackpad *pad);
bool input_trackpad_event(InputTrackpad *pad,const SDL_Event *event);
/* Coalesce both finger updates before deriving centroid orbit and pinch zoom. */
unsigned input_trackpad_actions(InputTrackpad *pad,IoAction actions[2]);

#endif
