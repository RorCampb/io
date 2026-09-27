#ifndef IO_ATTENTION_UI_H
#define IO_ATTENTION_UI_H
#include "hud.h"
#include <SDL2/SDL.h>

typedef struct AttentionUi {
    Hud hud;
    IoAttentionSettings applied,desired,initial;
    bool available,hidden,dirty,pending;
    int drag,width,height;
} AttentionUi;
bool attention_ui_init(AttentionUi *ui);
void attention_ui_refresh(AttentionUi *ui,IoApp *app,int width,int height);
bool attention_ui_event(AttentionUi *ui,const SDL_Event *event);
void attention_ui_submit(AttentionUi *ui,IoApp *app);
bool attention_ui_draw(AttentionUi *ui);
void attention_ui_destroy(AttentionUi *ui);
#endif
