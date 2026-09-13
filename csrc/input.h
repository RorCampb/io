#ifndef IO_INPUT_H
#define IO_INPUT_H

#include <SDL2/SDL.h>
#include "io.h"

typedef enum InputResult {
    INPUT_NONE,
    INPUT_ACTION,
    INPUT_QUIT
} InputResult;

// Translate one platform event into intent without reading or mutating application state.
InputResult input_translate(const SDL_Event *event, IoAction *action);

#endif
