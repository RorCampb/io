#ifndef IO_EDITOR_UI_H
#define IO_EDITOR_UI_H
#include "hud.h"
#include <SDL2/SDL.h>
typedef struct EditorUi {
    Hud hud;
    IoEditorView view;
    int width,height,field;
    bool editing,replace,mouse_capture;
    char input[48];
} EditorUi;
bool editor_ui_init(EditorUi *ui,IoApp *app);
void editor_ui_destroy(EditorUi *ui);
void editor_ui_refresh(EditorUi *ui,IoApp *app,int width,int height);
bool editor_ui_event(EditorUi *ui,IoApp *app,const SDL_Event *event);
bool editor_ui_draw(EditorUi *ui);
bool editor_ui_quit(EditorUi *ui,IoApp *app,SDL_Window *window);
#endif
