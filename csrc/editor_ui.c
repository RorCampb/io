#include "editor_ui.h"
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct Widget {float x,y,w,h;uint32_t command,index;int field;char label[64];bool active;} Widget;
static const char *labels[16]={"YAW DEGREES","PITCH DEGREES","ZOOM","TARGET HEIGHT M","YAW LIMIT DEG",
    "MIN ZOOM","MAX ZOOM","RESPONSE SEC","APPROACH M","MIN X","MIN Y","MIN Z","SIZE X","SIZE Y","SIZE Z","FOV DEGREES"};
static const float steps[16]={5,2,.25f,.1f,5,.25f,.25f,.05f,.5f,.25f,.25f,.1f,.25f,.25f,.1f,2};
static const char *portal_labels[10]={"CENTER X","CENTER Y","CENTER Z","NORMAL X","NORMAL Y","NORMAL Z","WIDTH M","HEIGHT M","FROM SPACE INDEX","TO SPACE INDEX"};
static const float portal_steps[10]={.1f,.1f,.1f,1,1,1,.1f,.1f,1,1};
static float field_y(int field){return 78+(field==15?9:field>=9?field+1:field)*31.f+(field>=9&&field<15?24:0);}
static size_t layout(const EditorUi *u,Widget *w){
    size_t n=0;float right=u->width-306.f;
    w[n++]=(Widget){12,42,92,28,IO_EDIT_PLAY,0,-1,"",true};
    snprintf(w[0].label,64,"%s",u->view.playing?"STOP F5":"PLAY F5");
    w[n++]=(Widget){112,42,92,28,IO_EDIT_SAVE,0,-1,"SAVE",true};
    w[n++]=(Widget){12,80,92,26,IO_EDIT_UNDO,0,-1,"UNDO",!u->view.playing};
    w[n++]=(Widget){112,80,92,26,IO_EDIT_REDO,0,-1,"REDO",!u->view.playing};
    for(unsigned i=0;i<8;i++){
        unsigned index=u->view.selected/8*8+i;
        if(index>=u->view.count)break;
        w[n]=(Widget){12,148+i*32.f,192,28,IO_EDIT_SELECT,index,-1,"",!u->view.playing};
        snprintf(w[n++].label,64,"%.22s",u->view.names[i]);
    }
    w[n++]=(Widget){12,410,92,26,IO_EDIT_SELECT,u->view.selected>=8?u->view.selected/8*8-8:0,-1,"PREV",!u->view.playing&&u->view.selected>=8};
    w[n++]=(Widget){112,410,92,26,IO_EDIT_SELECT,(u->view.selected/8+1)*8,-1,"NEXT",!u->view.playing&&(u->view.selected/8+1)*8<u->view.count};
    w[n++]=(Widget){12,454,192,28,IO_EDIT_FRAME,0,-1,"FRAME VOLUME",!u->view.playing};
    w[n++]=(Widget){12,490,192,28,IO_EDIT_PREVIEW,0,-1,"PREVIEW RIG",!u->view.playing&&!u->view.portal};
    w[n++]=(Widget){12,526,192,28,IO_EDIT_CAPTURE,0,-1,"USE CURRENT VIEW",!u->view.playing&&!(u->view.envelope&&u->view.selected)};
    w[n++]=(Widget){12,562,192,28,IO_EDIT_DUPLICATE,0,-1,"DUPLICATE VOLUME",!u->view.playing&&u->view.selected>0&&!u->view.portal};
    w[n]=(Widget){12,598,192,28,IO_EDIT_TRAJECTORY,0,-1,"",!u->view.playing&&u->view.selected>0&&!u->view.portal};
    snprintf(w[n++].label,64,"%s CAMERA PATH",u->view.trajectory?"HIDE":"SHOW");
    w[n++]=(Widget){12,634,192,28,IO_EDIT_ADD_PORTAL,0,-1,"ADD ENTRANCE",!u->view.playing&&u->view.selected>0&&!u->view.portal};
    for(int f=0;f<(u->view.portal?10:16);f++){
        float y=field_y(f);
        bool active=!u->view.playing&&(f<9||f==15||u->view.selected>0);
        bool inherited=!u->view.portal&&u->view.envelope&&u->view.selected&&(f<=2||(f>=4&&f<=6));
        active=active&&!inherited;
        w[n]=(Widget){right+158,y,86,25,IO_EDIT_SET,0,f,"",active};
        if(inherited)snprintf(w[n++].label,64,"GLOBAL");
        else snprintf(w[n++].label,64,"%.3f",u->view.values[f]);
        w[n++]=(Widget){right+246,y,23,25,IO_EDIT_SET,1,f,"-",active};
        w[n++]=(Widget){right+271,y,23,25,IO_EDIT_SET,2,f,"+",active};
    }
    return n;
}
static bool command(EditorUi *u,IoApp *app,uint32_t kind,uint32_t index,uint32_t field,float value){
    bool ok=io_app_editor_command(app,kind,index,field,value);
    io_app_editor_view(app,&u->view);return ok;
}
static bool finish(EditorUi *u,IoApp *app,bool accept){
    if(accept){
        char *end;float value=strtof(u->input,&end);
        if(end==u->input||*end||!isfinite(value)){command(u,app,IO_EDIT_SET,0,(uint32_t)u->field,NAN);return false;}
        if(!command(u,app,IO_EDIT_SET,0,(uint32_t)u->field,value))return false;
        command(u,app,IO_EDIT_COMMIT,0,0,0);
    }
    u->editing=false;SDL_StopTextInput();return true;
}
bool editor_ui_init(EditorUi *u,IoApp *app){
    *u=(EditorUi){0};return io_app_editor_enable(app)&&hud_init(&u->hud)&&io_app_editor_view(app,&u->view);
}
void editor_ui_destroy(EditorUi *u){SDL_StopTextInput();hud_destroy(&u->hud);}
void editor_ui_refresh(EditorUi *u,IoApp *app,int width,int height){
    u->width=width;u->height=height;io_app_editor_view(app,&u->view);
}
bool editor_ui_event(EditorUi *u,IoApp *app,const SDL_Event *e){
    if(e->type==SDL_WINDOWEVENT&&e->window.event==SDL_WINDOWEVENT_FOCUS_LOST){
        if(u->editing)finish(u,app,false);
        u->mouse_capture=false;
    }
    if(e->type==SDL_TEXTINPUT&&u->editing){
        if(u->replace){u->input[0]=0;u->replace=false;}
        size_t len=strlen(u->input);
        for(size_t i=0;e->text.text[i]&&len+1<sizeof(u->input);i++){
            char c=e->text.text[i];if((c>='0'&&c<='9')||c=='-'||c=='.'||c=='e'||c=='+')u->input[len++]=c;
        }
        u->input[len]=0;return true;
    }
    if(e->type==SDL_KEYDOWN){
        SDL_Keycode key=e->key.keysym.sym;
        if(u->editing){
            if(key==SDLK_ESCAPE)finish(u,app,false);
            else if(key==SDLK_RETURN||key==SDLK_KP_ENTER)finish(u,app,true);
            else if(key==SDLK_BACKSPACE){size_t n=strlen(u->input);if(u->replace)n=1;if(n)u->input[n-1]=0;u->replace=false;}
            return true;
        }
        if(!e->key.repeat&&key==SDLK_F5){command(u,app,IO_EDIT_PLAY,0,0,0);return true;}
        if(e->key.keysym.mod&(KMOD_GUI|KMOD_CTRL)){
            uint32_t cmd=key==SDLK_s?IO_EDIT_SAVE:key==SDLK_z?((e->key.keysym.mod&KMOD_SHIFT)?IO_EDIT_REDO:IO_EDIT_UNDO):0;
            if(cmd){command(u,app,cmd,0,0,0);return true;}
        }
        /* Editor navigation must not invoke the legacy world-resize shortcuts. */
        if(!u->view.playing && (key==SDLK_w||key==SDLK_a||key==SDLK_s||key==SDLK_d||key==SDLK_q||key==SDLK_e||key==SDLK_n||key==SDLK_TAB||key==SDLK_f||key==SDLK_r))return true;
    }
    if(e->type==SDL_KEYUP&&u->editing)return true;
    int x,y;SDL_GetMouseState(&x,&y);
    if(e->type==SDL_MOUSEBUTTONDOWN||e->type==SDL_MOUSEBUTTONUP){x=e->button.x;y=e->button.y;}
    if(e->type==SDL_MOUSEMOTION){x=e->motion.x;y=e->motion.y;}
    bool panel=x<216||x>=u->width-318||y<32||y>=u->height-62;
    if(e->type==SDL_MOUSEBUTTONDOWN){
        if(u->editing&&!finish(u,app,true))return true;
        u->mouse_capture=panel;
        if(!panel)return false;
        if(e->button.button!=SDL_BUTTON_LEFT)return true;
        Widget widgets[80];size_t n=layout(u,widgets);
        for(size_t i=0;i<n;i++){
            Widget *w=&widgets[i];
            if(!w->active||x<w->x||x>=w->x+w->w||y<w->y||y>=w->y+w->h)continue;
            if(w->command==IO_EDIT_SET){
                if(w->index){
                    float value=u->view.values[w->field]+(u->view.portal?portal_steps[w->field]:steps[w->field])*(w->index==1?-1:1);
                    command(u,app,IO_EDIT_SET,0,w->field,value);command(u,app,IO_EDIT_COMMIT,0,0,0);
                }else{
                    u->field=w->field;u->editing=true;u->replace=true;
                    snprintf(u->input,sizeof(u->input),"%.4f",u->view.values[w->field]);SDL_StartTextInput();
                }
            }else command(u,app,w->command,w->index,0,0);
            break;
        }
        return true;
    }
    if(e->type==SDL_MOUSEBUTTONUP){bool captured=u->mouse_capture;u->mouse_capture=false;return captured;}
    if(e->type==SDL_MOUSEMOTION)return u->mouse_capture||panel;
    if(e->type==SDL_MOUSEWHEEL)return panel||u->editing;
    return false;
}
static void element(HudElement *e,float x,float y,float w,float h,const char *text,bool accent){
    *e=(HudElement){.x=x,.y=y,.w=w,.h=h,.scale=1.35f,.color={.83f,.89f,.87f,1}};
    if(!text){e->color[0]=.045f;e->color[1]=.075f;e->color[2]=.085f;e->color[3]=.98f;}
    else snprintf(e->text,64,"%.63s",text);
    if(accent){e->color[0]=.98f;e->color[1]=.77f;e->color[2]=.25f;}
}
bool editor_ui_draw(EditorUi *u){
    HudElement e[240];size_t n=0;
    element(&e[n++],0,0,216,u->height,NULL,false);
    element(&e[n++],u->width-318,0,318,u->height,NULL,false);
    element(&e[n++],216,0,u->width-534,32,NULL,false);
    element(&e[n++],216,u->height-62,u->width-534,62,NULL,false);
    element(&e[n++],12,14,0,0,"IO / WORLD EDITOR",true);
    element(&e[n++],232,12,0,0,u->view.playing?"PLAYTEST / F5 TO STOP":"EDIT / SIMULATION PAUSED",true);
    element(&e[n++],12,122,0,0,"CAMERA REGIONS",true);
    element(&e[n++],u->width-306,18,0,0,u->view.portal?"ENTRANCE PORTAL":u->view.envelope?"GEOMETRY ORBIT":"CAMERA RIG",true);
    element(&e[n++],u->width-306,45,0,0,u->view.dirty?"UNSAVED CHANGES":"SAVED DOCUMENT",u->view.dirty);
    element(&e[n++],u->width-306,398,0,0,u->view.portal?"0 = EXTERIOR":"ACTIVATION VOLUME",true);
    for(int f=0;f<(u->view.portal?10:16);f++)element(&e[n++],u->width-306,field_y(f)+8,0,0,u->view.portal?portal_labels[f]:labels[f],false);
    element(&e[n++],u->width-306,620,0,0,"CLICK VALUE TO TYPE",false);
    element(&e[n++],u->width-306,642,0,0,"ENTER APPLY / ESC CANCEL",false);
    if(u->view.trajectory&&!u->view.playing&&!u->view.portal){
        element(&e[n],12,680,0,0,"PLAYER PATH",false);e[n].color[0]=.2f;e[n].color[1]=.9f;e[n++].color[2]=1.f;
        element(&e[n],124,680,0,0,"CAMERA",false);e[n].color[0]=1.f;e[n].color[1]=.5f;e[n++].color[2]=.2f;
    }
    element(&e[n++],232,u->height-48,0,0,u->view.playing?"WASD / SPACE JUMP / SHIFT CROUCH / R-DRAG ORBIT":"DRAG ORBIT / RIGHT DRAG PAN / SCROLL ZOOM",false);
    char status[64];size_t available=(size_t)fmaxf(1,(u->width-550)/8.1f);
    snprintf(status,sizeof(status),"%.*s",(int)available,u->view.status);
    element(&e[n++],232,u->height-24,0,0,status,true);
    Widget widgets[80];size_t count=layout(u,widgets);
    for(size_t i=0;i<count;i++){
        Widget *w=&widgets[i];bool selected=w->command==IO_EDIT_SELECT&&w->index==u->view.selected;
        bool typing=u->editing&&w->field==u->field&&w->command==IO_EDIT_SET&&!w->index;
        element(&e[n],w->x,w->y,w->w,w->h,NULL,false);
        e[n].color[0]=selected||typing?.25f:.10f;e[n].color[1]=selected||typing?.22f:.15f;e[n++].color[2]=.16f;
        char label[64];snprintf(label,sizeof(label),"%.*s",w->field>=0?10:63,typing?u->input:w->label);
        element(&e[n],w->x+6,w->y+8,0,0,label,selected||typing);
        e[n].scale=w->field>=0?1.2f:1.3f;
        if(!w->active)e[n].color[3]=.3f;
        n++;
    }
    return hud_draw_elements(&u->hud,u->width,u->height,e,n);
}
bool editor_ui_quit(EditorUi *u,IoApp *app,SDL_Window *window){
    if(u->editing&&!finish(u,app,true))return false;
    io_app_editor_view(app,&u->view);if(!u->view.dirty)return true;
    const SDL_MessageBoxButtonData buttons[]={{SDL_MESSAGEBOX_BUTTON_ESCAPEKEY_DEFAULT,0,"Cancel"},{0,1,"Discard"},{SDL_MESSAGEBOX_BUTTON_RETURNKEY_DEFAULT,2,"Save"}};
    SDL_MessageBoxData box={SDL_MESSAGEBOX_WARNING,window,"Unsaved camera rigs","Save changes before closing?",3,buttons,NULL};int choice=0;
    if(SDL_ShowMessageBox(&box,&choice)!=0)return false;
    return choice==1||(choice==2&&command(u,app,IO_EDIT_SAVE,0,0,0));
}
