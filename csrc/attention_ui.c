#include "attention_ui.h"
#include <math.h>
#include <stdio.h>

static float top(const AttentionUi *u){return u->height-222.f;}
static bool inside(const AttentionUi *u,int x,int y){
    return u->available&&!u->hidden&&x>=12&&x<=372&&y>=top(u)&&y<=u->height-12;
}
static void release(AttentionUi *u){u->drag=0;SDL_CaptureMouse(SDL_FALSE);}
bool attention_ui_init(AttentionUi *u){*u=(AttentionUi){0};return hud_init(&u->hud);}
void attention_ui_destroy(AttentionUi *u){release(u);hud_destroy(&u->hud);}
void attention_ui_refresh(AttentionUi *u,IoApp *app,int width,int height){
    u->width=width;u->height=height;
    IoAttentionSettings value={0};
    bool available=io_app_attention_settings(app,0,&value);
    if(available&&(!u->available||value.observer!=u->applied.observer||value.target!=u->applied.target)){
        u->initial=u->desired=value;u->dirty=u->pending=false;
    }
    u->available=available;u->applied=value;
    if(!available){release(u);return;}
    if(!u->dirty&&fabsf(value.fov_degrees-u->desired.fov_degrees)<.001f&&
       fabsf(value.notice_attention-u->desired.notice_attention)<.0001f)u->pending=false;
    if(!u->dirty&&!u->pending&&!u->drag)u->desired=value;
}
static void drag(AttentionUi *u,int x){
    float t=fminf(1,fmaxf(0,(x-30.f)/324.f));
    if(u->drag==1)u->desired.fov_degrees=roundf(5+355*t);
    if(u->drag==2)u->desired.notice_attention=roundf(1+99*t)/100.f;
    u->dirty=true;
}
bool attention_ui_event(AttentionUi *u,const SDL_Event *e){
    if(e->type==SDL_WINDOWEVENT&&e->window.event==SDL_WINDOWEVENT_FOCUS_LOST)release(u);
    if(!u->available)return false;
    if(e->type==SDL_KEYDOWN&&e->key.keysym.sym==SDLK_F2){
        if(!e->key.repeat){u->hidden=!u->hidden;release(u);}return true;
    }
    if(u->hidden)return false;
    if(e->type==SDL_MOUSEMOTION){
        if(u->drag){drag(u,e->motion.x);return true;}
        return inside(u,e->motion.x,e->motion.y);
    }
    if(e->type==SDL_MOUSEBUTTONUP&&u->drag&&e->button.button==SDL_BUTTON_LEFT){
        drag(u,e->button.x);release(u);return true;
    }
    if(e->type==SDL_MOUSEBUTTONDOWN||e->type==SDL_MOUSEBUTTONUP){
        if(!inside(u,e->button.x,e->button.y))return false;
        if(e->type==SDL_MOUSEBUTTONDOWN&&e->button.button==SDL_BUTTON_LEFT){
            float y=e->button.y-top(u);
            if(y>=48&&y<=78)u->drag=1;
            if(y>=100&&y<=130&&u->applied.has_pursuit)u->drag=2;
            if(y>=176){u->desired=u->initial;u->dirty=true;}
            if(u->drag){SDL_CaptureMouse(SDL_TRUE);drag(u,e->button.x);}
        }
        return true;
    }
    if(e->type==SDL_MOUSEWHEEL){int x,y;SDL_GetMouseState(&x,&y);return inside(u,x,y);}
    return false;
}
void attention_ui_submit(AttentionUi *u,IoApp *app){
    if(u->available&&u->dirty&&io_app_tune_attention(app,u->desired.observer,u->desired.target,
       u->desired.fov_degrees,u->desired.notice_attention)){
        u->dirty=false;u->pending=true;
    }
}
bool attention_ui_draw(AttentionUi *u){
    if(!u->available||u->hidden)return true;
    HudElement e[20]={0};size_t n=0;float y=top(u);
    e[n++]=(HudElement){.x=12,.y=y,.w=360,.h=210,.color={.025f,.045f,.05f,.96f}};
    e[n++]=(HudElement){.x=24,.y=y+12,.scale=1.5f,.color={1,.85f,.15f,1},.text="VISION TUNING - F2 HIDE"};
    for(int row=0;row<2;row++){
        float value=row?u->desired.notice_attention:u->desired.fov_degrees;
        float t=row?(value-.01f)/.99f:(value-5)/355;
        float ry=y+36+row*52;
        bool enabled=!row||u->applied.has_pursuit;
        e[n]=(HudElement){.x=24,.y=ry,.scale=1.5f,.color={.85f,.92f,.95f,1}};
        if(!enabled)snprintf(e[n++].text,64,"NOTICE: NO PURSUIT POLICY");
        else if(row)snprintf(e[n++].text,64,"NOTICE THRESHOLD %.0f%%",value*100);
        else snprintf(e[n++].text,64,"FOV %.0f DEG / EDGE +/-%.1f",value,value*.5f);
        e[n++]=(HudElement){.x=30,.y=ry+23,.w=324,.h=5,.color={.2f,.28f,.3f,1}};
        if(enabled)e[n++]=(HudElement){.x=26+324*t,.y=ry+17,.w=8,.h=17,.color={1,.85f,.15f,1}};
    }
    e[n]=(HudElement){.x=24,.y=y+143,.scale=1.25f,.color={.65f,.8f,.85f,1}};
    snprintf(e[n++].text,64,"APPLIED %.0f DEG / %.0f%% %s",u->applied.fov_degrees,
        u->applied.notice_attention*100,(u->dirty||u->pending)?"PENDING":"");
    e[n++]=(HudElement){.x=24,.y=y+160,.scale=1.25f,.color={.65f,.8f,.85f,1},.text="SMOOTH FALLOFF - LIVE, NOT SAVED"};
    e[n++]=(HudElement){.x=24,.y=y+185,.scale=1.5f,.color={1,.85f,.15f,1},.text="RESET TO SCENE SETTINGS"};
    return hud_draw_elements(&u->hud,u->width,u->height,e,n);
}
