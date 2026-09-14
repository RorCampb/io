#include "hud.h"
#include <OpenGL/gl3.h>
#include <math.h>
#include <stdio.h>
#include <string.h>

typedef struct HudVertex {float x,y,u,v;uint32_t mask[2];float color[4];} HudVertex;
enum { MAX_VERTICES=6*(2+HUD_LINE_COUNT*(HUD_LINE_LENGTH-1)+12*63+16*12) };

static const char *vertex_source=
    "#version 410 core\n"
    "layout(location=0) in vec2 position; layout(location=1) in vec2 uv;\n"
    "layout(location=2) in uvec2 mask; layout(location=3) in vec4 color;\n"
    "uniform vec2 viewport; out vec2 glyph_uv; flat out uvec2 glyph_mask; out vec4 ink;\n"
    "void main(){gl_Position=vec4(position.x*2.0/viewport.x-1.0,1.0-position.y*2.0/viewport.y,0,1);\n"
    "glyph_uv=uv;glyph_mask=mask;ink=color;}\n";
static const char *fragment_source=
    "#version 410 core\n"
    "in vec2 glyph_uv; flat in uvec2 glyph_mask; in vec4 ink; out vec4 color;\n"
    "void main(){ivec2 p=clamp(ivec2(floor(glyph_uv*vec2(5,7))),ivec2(0),ivec2(4,6));\n"
    "uint bit=uint(p.y*5+p.x); uint bits=bit<32u?glyph_mask.x:glyph_mask.y;\n"
    "if(((bits>>(bit%32u))&1u)==0u)discard;color=ink;}\n";

static GLuint compile(GLenum type,const char *source){
    GLuint shader=glCreateShader(type);glShaderSource(shader,1,&source,NULL);glCompileShader(shader);
    GLint ok=0;glGetShaderiv(shader,GL_COMPILE_STATUS,&ok);
    if(!ok){char log[2048];glGetShaderInfoLog(shader,sizeof(log),NULL,log);
        fprintf(stderr,"HUD shader: %s\n",log);glDeleteShader(shader);return 0;}
    return shader;
}

void hud_reset(Hud *h){
    h->sampling=false;h->measured=false;h->worker=false;h->dirty=true;h->sample_frames=0;
}
void hud_set_tick_hz(Hud *h,uint32_t tick_hz){
    if(h->tick_hz!=tick_hz){h->tick_hz=tick_hz;h->dirty=true;}
}
void hud_set_game(Hud *h,const IoGameView *game){
    if(memcmp(&h->game,game,sizeof(*game))!=0){h->game=*game;h->dirty=true;}
}
bool hud_contains_point(const Hud *h,float x,float y){
    float scale=h->width<300?1.5f:2.f;
    float left=fmaxf(4.f,h->width-118.f*scale-12.f);
    if(x>=left && x<=left+118.f*scale && y>=12.f && y<=12.f+(4.f+12.f*HUD_LINE_COUNT)*scale)return true;
    float panel=fminf(500.f,(float)h->width*0.48f),font=panel/(64.f*6.f+12.f);
    unsigned int rows=h->game.line_count<12?h->game.line_count:12;
    return h->game.enabled && x>=12.f && x<=12.f+panel && y>=12.f && y<=12.f+(rows*12.f+10.f)*font;
}
bool hud_init(Hud *h){
    *h=(Hud){0};hud_reset(h);
    GLuint vertex=compile(GL_VERTEX_SHADER,vertex_source),fragment=compile(GL_FRAGMENT_SHADER,fragment_source);
    if(!vertex||!fragment){if(vertex)glDeleteShader(vertex);if(fragment)glDeleteShader(fragment);return false;}
    h->program=glCreateProgram();glAttachShader(h->program,vertex);glAttachShader(h->program,fragment);
    glLinkProgram(h->program);glDeleteShader(vertex);glDeleteShader(fragment);
    GLint ok=0;glGetProgramiv(h->program,GL_LINK_STATUS,&ok);
    if(!ok){char log[2048];glGetProgramInfoLog(h->program,sizeof(log),NULL,log);
        fprintf(stderr,"HUD link: %s\n",log);hud_destroy(h);return false;}
    h->viewport_uniform=glGetUniformLocation(h->program,"viewport");
    glGenVertexArrays(1,&h->vao);glBindVertexArray(h->vao);
    glGenBuffers(1,&h->vbo);glBindBuffer(GL_ARRAY_BUFFER,h->vbo);
    glBufferData(GL_ARRAY_BUFFER,sizeof(HudVertex)*MAX_VERTICES,NULL,GL_DYNAMIC_DRAW);
    glEnableVertexAttribArray(0);glVertexAttribPointer(0,2,GL_FLOAT,GL_FALSE,sizeof(HudVertex),(void *)offsetof(HudVertex,x));
    glEnableVertexAttribArray(1);glVertexAttribPointer(1,2,GL_FLOAT,GL_FALSE,sizeof(HudVertex),(void *)offsetof(HudVertex,u));
    glEnableVertexAttribArray(2);glVertexAttribIPointer(2,2,GL_UNSIGNED_INT,sizeof(HudVertex),(void *)offsetof(HudVertex,mask));
    glEnableVertexAttribArray(3);glVertexAttribPointer(3,4,GL_FLOAT,GL_FALSE,sizeof(HudVertex),(void *)offsetof(HudVertex,color));
    glBindVertexArray(0);glBindBuffer(GL_ARRAY_BUFFER,0);
    if(glGetError()!=GL_NO_ERROR){hud_destroy(h);return false;}
    return true;
}
void hud_destroy(Hud *h){
    glDeleteBuffers(1,&h->vbo);glDeleteVertexArrays(1,&h->vao);
    if(h->program)glDeleteProgram(h->program);
    *h=(Hud){0};
}
void hud_presented(Hud *h,double now,const IoWorkerStats *worker){
    if(!isfinite(now))return;
    bool threaded=worker!=NULL;
    uint64_t tick=worker?worker->tick:0;
    if(!h->sampling || now<=h->sample_start || threaded!=h->worker || tick<h->start_tick){
        h->sample_start=now;h->sample_frames=0;h->start_tick=tick;
        h->sampling=true;h->worker=threaded;h->measured=false;h->dirty=true;
        if(worker)h->stats=*worker;
        return;
    }
    h->sample_frames++;
    double elapsed=now-h->sample_start;
    if(elapsed>=0.25){
        h->fps=(double)h->sample_frames/elapsed;
        h->simulation_hz=(double)(tick-h->start_tick)/elapsed;
        if(worker)h->stats=*worker;
        h->measured=true;h->dirty=true;
        h->sample_start=now;h->sample_frames=0;h->start_tick=tick;
    }
}
void hud_lines(const Hud *h,char lines[HUD_LINE_COUNT][HUD_LINE_LENGTH]){
    if(h->measured)snprintf(lines[0],48,"FPS %.1f",h->fps);
    else snprintf(lines[0],48,"FPS --");
    if(h->worker){
        if(h->stats.status!=1)snprintf(lines[1],48,"SIM STOPPED");
        else if(h->measured)snprintf(lines[1],48,"SIM %.1f HZ",h->simulation_hz);
        else snprintf(lines[1],48,"SIM -- HZ");
        snprintf(lines[2],48,"AGE %.0f MS",h->stats.snapshot_age_ms);
    }else{
        snprintf(lines[1],48,"SIM SYNC");
        snprintf(lines[2],48,"RENDER CADENCE");
    }
    if(h->tick_hz)snprintf(lines[3],HUD_LINE_LENGTH,"BUDGET %.3f MS",1000./h->tick_hz);
    else snprintf(lines[3],HUD_LINE_LENGTH,"BUDGET -- MS");
    if(h->measured && h->fps>0.)snprintf(lines[4],HUD_LINE_LENGTH,"FRAME %.3f MS",1000./h->fps);
    else snprintf(lines[4],HUD_LINE_LENGTH,"FRAME -- MS");
    // Completion cadence includes waiting and missed deadlines, unlike solver CPU cost.
    if(!h->worker)snprintf(lines[5],HUD_LINE_LENGTH,"UPDATE SYNC");
    else if(h->stats.status!=1)snprintf(lines[5],HUD_LINE_LENGTH,"UPDATE STOPPED");
    else if(!h->measured)snprintf(lines[5],HUD_LINE_LENGTH,"UPDATE -- MS");
    else if(h->simulation_hz>0.)snprintf(lines[5],HUD_LINE_LENGTH,"UPDATE %.3f MS",1000./h->simulation_hz);
    else snprintf(lines[5],HUD_LINE_LENGTH,"UPDATE STALLED");
}

// Five-bit rows, top to bottom. No external font, atlas, or text dependency.
static void glyph(char c,uint32_t mask[2]){
    /* The bitmap face is uppercase, but authored dialogue may use mixed case. */
    if(c>='a' && c<='z')c=(char)(c-'a'+'A');
    static const unsigned char digits[10][7]={
        {14,17,19,21,25,17,14},{4,12,4,4,4,4,14},{14,17,1,2,4,8,31},
        {30,1,1,14,1,1,30},{2,6,10,18,31,2,2},{31,16,16,30,1,1,30},
        {14,16,16,30,17,17,14},{31,1,2,4,8,8,8},{14,17,17,14,17,17,14},
        {14,17,17,15,1,1,14}};
    static const unsigned char letters[26][7]={
        {14,17,17,31,17,17,17},{30,17,17,30,17,17,30},{14,17,16,16,16,17,14},
        {30,17,17,17,17,17,30},{31,16,16,30,16,16,31},{31,16,16,30,16,16,16},
        {14,17,16,23,17,17,15},{17,17,17,31,17,17,17},{14,4,4,4,4,4,14},
        {7,2,2,2,2,18,12},{17,18,20,24,20,18,17},{16,16,16,16,16,16,31},
        {17,27,21,21,17,17,17},{17,25,21,19,17,17,17},{14,17,17,17,17,17,14},
        {30,17,17,30,16,16,16},{14,17,17,17,21,18,13},{30,17,17,30,20,18,17},
        {15,16,16,14,1,1,30},{31,4,4,4,4,4,4},{17,17,17,17,17,17,14},
        {17,17,17,17,17,10,4},{17,17,17,21,21,21,10},{17,17,10,4,10,17,17},
        {17,17,10,4,4,4,4},{31,1,2,4,8,16,31}};
    unsigned char punctuation[7]={0};const unsigned char *rows=punctuation;
    if(c>='0'&&c<='9')rows=digits[c-'0'];
    else if(c>='A'&&c<='Z')rows=letters[c-'A'];
    else if(c=='.')punctuation[6]=4;
    else if(c=='-')punctuation[3]=14;
    else if(c==','){punctuation[5]=4;punctuation[6]=8;}
    else if(c=='\'' || c=='"'){
        punctuation[0]=punctuation[1]=c=='\''?4:10;
    }
    else if(c==':' || c==';'){
        punctuation[2]=punctuation[5]=4;
        if(c==';')punctuation[6]=8;
    }
    else if(c=='!'){for(int row=0;row<4;row++)punctuation[row]=4;punctuation[6]=4;}
    else if(c=='?'){
        const unsigned char rows[7]={14,17,1,2,4,0,4};memcpy(punctuation,rows,7);
    }
    else if(c=='/'){
        const unsigned char rows[7]={1,2,2,4,8,8,16};memcpy(punctuation,rows,7);
    }
    else if(c=='(' || c==')'){
        const unsigned char left[7]={2,4,8,8,8,4,2},right[7]={8,4,2,2,2,4,8};
        memcpy(punctuation,c=='('?left:right,7);
    }
    else if(c=='[' || c==']'){
        punctuation[0]=punctuation[6]=14;
        for(int row=1;row<6;row++)punctuation[row]=c=='['?8:2;
    }
    mask[0]=mask[1]=0;
    for(unsigned int y=0;y<7;y++)for(unsigned int x=0;x<5;x++)if(rows[y]&(1u<<(4-x))){
        unsigned int bit=y*5+x;mask[bit/32]|=1u<<(bit%32);
    }
}
static void quad(HudVertex *v,int *count,float x,float y,float w,float height,const uint32_t mask[2],const float color[4]){
    const float uv[6][2]={{0,0},{1,0},{1,1},{0,0},{1,1},{0,1}};
    for(int i=0;i<6;i++){
        v[*count]=(HudVertex){.x=x+uv[i][0]*w,.y=y+uv[i][1]*height,.u=uv[i][0],.v=uv[i][1]};
        memcpy(v[*count].mask,mask,2*sizeof(uint32_t));memcpy(v[*count].color,color,4*sizeof(float));(*count)++;
    }
}
bool hud_draw(Hud *h,int width,int height){
    if(width<=0||height<=0)return false;
    h->last_upload_bytes=0;
    GLint program,vao,buffer;glGetIntegerv(GL_CURRENT_PROGRAM,&program);
    glGetIntegerv(GL_VERTEX_ARRAY_BINDING,&vao);glGetIntegerv(GL_ARRAY_BUFFER_BINDING,&buffer);
    GLboolean depth=glIsEnabled(GL_DEPTH_TEST),blend=glIsEnabled(GL_BLEND);
    GLint src_rgb,dst_rgb,src_alpha,dst_alpha;
    glGetIntegerv(GL_BLEND_SRC_RGB,&src_rgb);glGetIntegerv(GL_BLEND_DST_RGB,&dst_rgb);
    glGetIntegerv(GL_BLEND_SRC_ALPHA,&src_alpha);glGetIntegerv(GL_BLEND_DST_ALPHA,&dst_alpha);
    glUseProgram(h->program);glBindVertexArray(h->vao);glBindBuffer(GL_ARRAY_BUFFER,h->vbo);
    glUniform2f(h->viewport_uniform,(float)width,(float)height);
    if(h->dirty||h->width!=width||h->height!=height){
        HudVertex vertices[MAX_VERTICES];int count=0;char lines[HUD_LINE_COUNT][HUD_LINE_LENGTH];hud_lines(h,lines);
        float scale=width<300?1.5f:2.f,panel_width=118.f*scale;
        float x=fmaxf(4.f,width-panel_width-12.f),y=12.f;
        const uint32_t full[2]={UINT32_MAX,7};const float bg[4]={0.025f,0.04f,0.05f,0.90f};
        const float white[4]={0.94f,0.97f,1.f,1.f},yellow[4]={1.f,0.83f,0.2f,1.f},red[4]={1.f,0.38f,0.28f,1.f};
        for(unsigned int i=0;i<h->game.damage_count && i<16;i++){
            IoDamageText hit=h->game.damage[i];
            if(!isfinite(hit.x)||!isfinite(hit.y)||!isfinite(hit.alpha)||hit.alpha<=0.f)continue;
            char text[13];snprintf(text,sizeof(text),"-%u",hit.amount);
            float font=2.2f,color[4]={1.f,0.65f,0.22f,fminf(1.f,hit.alpha)};
            float start=hit.x-(float)strlen(text)*3.f*font;
            for(size_t j=0;j<strlen(text);j++){
                uint32_t bits[2];glyph(text[j],bits);
                quad(vertices,&count,start+(float)j*6.f*font,hit.y,5.f*font,7.f*font,bits,color);
            }
        }
        quad(vertices,&count,x,y,panel_width,(4.f+12.f*HUD_LINE_COUNT)*scale,full,bg);
        for(int line=0;line<HUD_LINE_COUNT;line++){
            float font=line==0?scale*1.5f:scale;
            const float *color=line==0?(h->measured&&h->fps<55?red:yellow):white;
            if(h->tick_hz && h->measured && line==4 && h->fps<(double)h->tick_hz*0.99)color=red;
            if(h->tick_hz && h->measured && h->worker && line==5 &&
                (h->stats.status!=1 || h->simulation_hz<(double)h->tick_hz*0.99))color=red;
            for(size_t i=0;i<strlen(lines[line]);i++){
                uint32_t bits[2];glyph(lines[line][i],bits);
                quad(vertices,&count,x+6.f*scale+(float)i*6.f*font,y+5.f*scale+(float)line*12.f*scale,
                    5.f*font,7.f*font,bits,color);
            }
        }
        if(h->game.enabled){
            unsigned int rows=h->game.line_count<12?h->game.line_count:12;
            float panel=fminf(500.f,(float)width*0.48f),font=panel/(64.f*6.f+12.f);
            quad(vertices,&count,12.f,12.f,panel,((float)rows*12.f+10.f)*font,full,bg);
            for(unsigned int row=0;row<rows;row++)for(unsigned int col=0;col<63 && h->game.lines[row][col];col++){
                uint32_t bits[2];glyph(h->game.lines[row][col],bits);
                quad(vertices,&count,12.f+6.f*font+(float)col*6.f*font,12.f+5.f*font+(float)row*12.f*font,
                    5.f*font,7.f*font,bits,row==0?yellow:white);
            }
        }
        glBufferSubData(GL_ARRAY_BUFFER,0,(GLsizeiptr)(count*sizeof(HudVertex)),vertices);
        h->last_upload_bytes=(size_t)count*sizeof(HudVertex);h->vertices=count;
        h->dirty=false;h->width=width;h->height=height;
    }
    glDisable(GL_DEPTH_TEST);glEnable(GL_BLEND);glBlendFunc(GL_SRC_ALPHA,GL_ONE_MINUS_SRC_ALPHA);
    glDrawArrays(GL_TRIANGLES,0,h->vertices);
    if(depth)glEnable(GL_DEPTH_TEST);else glDisable(GL_DEPTH_TEST);
    if(!blend)glDisable(GL_BLEND);
    glBlendFuncSeparate((GLenum)src_rgb,(GLenum)dst_rgb,(GLenum)src_alpha,(GLenum)dst_alpha);
    glUseProgram((GLuint)program);glBindVertexArray((GLuint)vao);glBindBuffer(GL_ARRAY_BUFFER,(GLuint)buffer);
    return glGetError()==GL_NO_ERROR;
}
