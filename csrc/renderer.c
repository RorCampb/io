#include "renderer.h"
#include <OpenGL/gl3.h>
#include <stdio.h>
#include <stdlib.h>
#include <limits.h>
#include <math.h>

static const char *vertex_source =
    "#version 410 core\n"
    "layout(location=0) in vec3 position;\n"
    "layout(location=1) in mat4 model;\n"
    "layout(location=5) in vec3 color;\n"
    "uniform mat4 clip_from_world;\n"
    "out VertexData { vec3 color; } vertex;\n"
    "void main(){ gl_Position=clip_from_world*model*vec4(position,1.0); vertex.color=color; }\n";
static const char *geometry_source =
    "#version 410 core\n"
    "layout(lines) in;\n"
    "layout(triangle_strip,max_vertices=4) out;\n"
    "in VertexData { vec3 color; } vertex[];\n"
    "out vec3 edge_color; noperspective out float edge_offset;\n"
    "uniform vec2 viewport; uniform float line_width;\n"
    "void emit_edge(vec4 p, vec2 offset, float side, vec3 color){\n"
    " gl_Position=p+vec4(offset*side*p.w,0.0,0.0);\n"
    " edge_offset=side*(line_width*0.5+1.0); edge_color=color; EmitVertex(); }\n"
    "void main(){\n"
    " vec4 a=gl_in[0].gl_Position,b=gl_in[1].gl_Position;\n"
    " vec2 delta=(b.xy/b.w-a.xy/a.w)*viewport;\n"
    " float len=length(delta); if(len<0.0001) return;\n"
    " vec2 normal=vec2(-delta.y,delta.x)/len;\n"
    " vec2 offset=normal*(line_width*0.5+1.0)*2.0/viewport;\n"
    " emit_edge(a,offset,1.0,vertex[0].color); emit_edge(a,offset,-1.0,vertex[0].color);\n"
    " emit_edge(b,offset,1.0,vertex[1].color); emit_edge(b,offset,-1.0,vertex[1].color); EndPrimitive(); }\n";
static const char *fragment_source =
    "#version 410 core\n"
    "in vec3 edge_color; noperspective in float edge_offset;\n"
    "uniform float line_width; out vec4 output_color;\n"
    "void main(){ float half_width=line_width*0.5+1.0;\n"
    " float coverage=1.0-smoothstep(half_width-1.0,half_width,abs(edge_offset));\n"
    " if(coverage<=0.0) discard; output_color=vec4(edge_color,coverage); }\n";
static const char *solid_vertex_source =
    "#version 410 core\n"
    "layout(location=0) in vec3 position;\n"
    "layout(location=1) in mat4 model;\n"
    "layout(location=5) in vec3 color;\n"
    "layout(location=6) in vec3 normal;\n"
    "layout(location=7) in vec4 material_color;\n"
    "layout(location=8) in vec3 emission;\n"
    "layout(location=9) in uvec4 joints;\n"
    "layout(location=10) in vec4 weights;\n"
    "layout(location=11) in uint joint_offset;\n"
    "uniform samplerBuffer joint_matrices;\n"
    "uniform vec2 outline_offset;\n"
    "uniform mat4 clip_from_world;\n"
    "out vec3 surface_color; out vec3 world_position; out vec3 world_normal; out vec3 surface_emission;\n"
    "mat4 joint(uint i){ int at=int((joint_offset+i)*4u); return mat4(texelFetch(joint_matrices,at),\n"
    " texelFetch(joint_matrices,at+1),texelFetch(joint_matrices,at+2),texelFetch(joint_matrices,at+3)); }\n"
    "void main(){ mat4 skin=mat4(1.0);\n"
    " if(dot(weights,vec4(1.0))>0.0) skin=weights.x*joint(joints.x)+weights.y*joint(joints.y)+weights.z*joint(joints.z)+weights.w*joint(joints.w);\n"
    " mat4 transform=model*skin; vec4 p=transform*vec4(position,1.0); world_position=p.xyz;\n"
    " world_normal=vec3(0.0);\n"
    " if(dot(normal,normal)>0.0) world_normal=normalize(transpose(inverse(mat3(transform)))*normal);\n"
    " gl_Position=clip_from_world*p; gl_Position.xy+=outline_offset*gl_Position.w; surface_color=color*material_color.rgb; surface_emission=emission; }\n";
static const char *solid_fragment_source =
    "#version 410 core\n"
    "in vec3 surface_color; in vec3 world_position; in vec3 world_normal; in vec3 surface_emission; out vec4 output_color;\n"
    "uniform bool outline_mode;\n"
    "void main(){ if(outline_mode){output_color=vec4(1.0,0.86,0.12,1.0);return;} vec3 face_normal=cross(dFdx(world_position),dFdy(world_position));\n"
    " vec3 n=normalize(dot(world_normal,world_normal)>0.000001 ? world_normal : face_normal);\n"
    " float light=0.3+0.7*abs(dot(n,normalize(vec3(0.4,-0.6,1.0))));\n"
    " vec3 linear_color=surface_color*light+surface_emission;\n"
    " output_color=vec4(pow(clamp(linear_color,0.0,1.0),vec3(1.0/2.2)),1.0); }\n";

static const char *effect_vertex_source=
    "#version 410 core\nlayout(location=0) in vec3 position;layout(location=1) in float radius;\n"
    "uniform mat4 clip_from_world;uniform float pixel_scale;\n"
    "void main(){gl_Position=clip_from_world*vec4(position,1);gl_PointSize=clamp(2.0*radius*pixel_scale,4.0,128.0);}\n";
static const char *effect_fragment_source=
    "#version 410 core\nout vec4 output_color;void main(){float r=length(gl_PointCoord*2.0-1.0);\n"
    "if(r>1.0)discard;float a=1.0-smoothstep(0.45,1.0,r);\n"
    "output_color=vec4(mix(vec3(0.1,0.65,1.0),vec3(0.9,1.0,1.0),1.0-r),a);}\n";

static GLuint compile_shader(GLenum type,const char *source) {
    GLuint shader=glCreateShader(type);
    glShaderSource(shader,1,&source,NULL); glCompileShader(shader);
    GLint ok=0;glGetShaderiv(shader,GL_COMPILE_STATUS,&ok);
    if(!ok){
        char log[4096];glGetShaderInfoLog(shader,sizeof(log),NULL,log);
        fprintf(stderr,"Shader compile failed: %s\n",log);glDeleteShader(shader);return 0;
    }
    return shader;
}

bool renderer_init(Renderer *r) {
    *r=(Renderer){0};r->frame_serial=UINT64_MAX;
    GLuint vertex=compile_shader(GL_VERTEX_SHADER,vertex_source);
    GLuint geometry=compile_shader(GL_GEOMETRY_SHADER,geometry_source);
    GLuint fragment=compile_shader(GL_FRAGMENT_SHADER,fragment_source);
    GLuint solid_vertex=compile_shader(GL_VERTEX_SHADER,solid_vertex_source);
    GLuint solid_fragment=compile_shader(GL_FRAGMENT_SHADER,solid_fragment_source);
    if(!vertex||!geometry||!fragment||!solid_vertex||!solid_fragment){
        if(vertex)glDeleteShader(vertex);
        if(geometry)glDeleteShader(geometry);
        if(fragment)glDeleteShader(fragment);
        if(solid_vertex)glDeleteShader(solid_vertex);
        if(solid_fragment)glDeleteShader(solid_fragment);
        return false;
    }
    r->program=glCreateProgram();
    glAttachShader(r->program,vertex);glAttachShader(r->program,geometry);glAttachShader(r->program,fragment);
    glLinkProgram(r->program);
    glDeleteShader(vertex);glDeleteShader(geometry);glDeleteShader(fragment);
    r->solid_program=glCreateProgram();
    glAttachShader(r->solid_program,solid_vertex);glAttachShader(r->solid_program,solid_fragment);
    glLinkProgram(r->solid_program);
    glDeleteShader(solid_vertex);glDeleteShader(solid_fragment);
    GLint ok=0;glGetProgramiv(r->program,GL_LINK_STATUS,&ok);
    if(!ok){
        char log[4096];glGetProgramInfoLog(r->program,sizeof(log),NULL,log);
        fprintf(stderr,"Shader link failed: %s\n",log);renderer_destroy(r);return false;
    }
    glGetProgramiv(r->solid_program,GL_LINK_STATUS,&ok);
    if(!ok){
        char log[4096];glGetProgramInfoLog(r->solid_program,sizeof(log),NULL,log);
        fprintf(stderr,"Solid shader link failed: %s\n",log);renderer_destroy(r);return false;
    }
    r->matrix_uniform=glGetUniformLocation(r->program,"clip_from_world");
    r->viewport_uniform=glGetUniformLocation(r->program,"viewport");
    r->width_uniform=glGetUniformLocation(r->program,"line_width");
    r->solid_matrix_uniform=glGetUniformLocation(r->solid_program,"clip_from_world");
    r->joints_uniform=glGetUniformLocation(r->solid_program,"joint_matrices");
    r->outline_offset_uniform=glGetUniformLocation(r->solid_program,"outline_offset");
    r->outline_mode_uniform=glGetUniformLocation(r->solid_program,"outline_mode");
    glGetFramebufferAttachmentParameteriv(GL_DRAW_FRAMEBUFFER,GL_STENCIL,GL_FRAMEBUFFER_ATTACHMENT_STENCIL_SIZE,&r->stencil_bits);
    GLuint effect_vertex=compile_shader(GL_VERTEX_SHADER,effect_vertex_source);
    GLuint effect_fragment=compile_shader(GL_FRAGMENT_SHADER,effect_fragment_source);
    if(!effect_vertex||!effect_fragment){if(effect_vertex)glDeleteShader(effect_vertex);if(effect_fragment)glDeleteShader(effect_fragment);renderer_destroy(r);return false;}
    r->effect_program=glCreateProgram();glAttachShader(r->effect_program,effect_vertex);glAttachShader(r->effect_program,effect_fragment);
    glLinkProgram(r->effect_program);glDeleteShader(effect_vertex);glDeleteShader(effect_fragment);
    glGetProgramiv(r->effect_program,GL_LINK_STATUS,&ok);if(!ok){renderer_destroy(r);return false;}
    r->effect_matrix_uniform=glGetUniformLocation(r->effect_program,"clip_from_world");
    r->effect_scale_uniform=glGetUniformLocation(r->effect_program,"pixel_scale");
    glGenVertexArrays(1,&r->effect_vao);glBindVertexArray(r->effect_vao);
    glGenBuffers(1,&r->effect_vbo);glBindBuffer(GL_ARRAY_BUFFER,r->effect_vbo);
    glBufferData(GL_ARRAY_BUFFER,4*sizeof(IoProjectileView),NULL,GL_DYNAMIC_DRAW);
    glEnableVertexAttribArray(0);glVertexAttribPointer(0,3,GL_FLOAT,GL_FALSE,sizeof(IoProjectileView),(void *)offsetof(IoProjectileView,position));
    glEnableVertexAttribArray(1);glVertexAttribPointer(1,1,GL_FLOAT,GL_FALSE,sizeof(IoProjectileView),(void *)offsetof(IoProjectileView,radius));
    glBindVertexArray(0);
    glGetIntegerv(GL_MAX_TEXTURE_BUFFER_SIZE,&r->max_joint_matrices);r->max_joint_matrices/=4;
    if(r->max_joint_matrices<=0 ||
       !dynamic_buffer_init(&r->instances,GL_ARRAY_BUFFER,1024*sizeof(IoInstance),PTRDIFF_MAX) ||
       !dynamic_buffer_init(&r->grid,GL_ARRAY_BUFFER,1024*sizeof(IoVec3),PTRDIFF_MAX) ||
       !dynamic_buffer_init(&r->joints,GL_TEXTURE_BUFFER,4096*16*sizeof(float),
                            (size_t)r->max_joint_matrices*16*sizeof(float))){
        renderer_destroy(r);return false;
    }
    glGenTextures(1,&r->joint_texture);
    glGenVertexArrays(1,&r->grid_vao);glBindVertexArray(r->grid_vao);
    glBindBuffer(GL_ARRAY_BUFFER,r->grid.id);
    glEnableVertexAttribArray(0);
    glVertexAttribPointer(0,3,GL_FLOAT,GL_FALSE,sizeof(IoVec3),(void *)0);
    glBindVertexArray(0);
    glEnable(GL_BLEND);glBlendFunc(GL_SRC_ALPHA,GL_ONE_MINUS_SRC_ALPHA);
    glEnable(GL_DEPTH_TEST);glDepthFunc(GL_LEQUAL);
    glEnable(GL_MULTISAMPLE);
    GLint samples=0;glGetIntegerv(GL_SAMPLES,&samples);
    fprintf(stderr,"OpenGL %s; %dx MSAA; GPU instancing and shader-antialiased edges\n",glGetString(GL_VERSION),samples);
    if(!hud_init(&r->hud)){renderer_destroy(r);return false;}
    if(glGetError()!=GL_NO_ERROR){renderer_destroy(r);return false;}
    return true;
}
void renderer_destroy(Renderer *r) {
    glDeleteBuffers(1,&r->effect_vbo);glDeleteVertexArrays(1,&r->effect_vao);
    if(r->effect_program)glDeleteProgram(r->effect_program);
    hud_destroy(&r->hud);
    for(size_t i=0;i<r->model_count;i++){
        glDeleteBuffers(1,&r->models[i].vbo);glDeleteBuffers(1,&r->models[i].ebo);
        glDeleteVertexArrays(1,&r->models[i].vao);
    }
    free(r->models);
    dynamic_buffer_report(&r->instances,"instances");
    dynamic_buffer_report(&r->grid,"grid");
    dynamic_buffer_report(&r->joints,"joints");
    dynamic_buffer_destroy(&r->instances);dynamic_buffer_destroy(&r->grid);
    dynamic_buffer_destroy(&r->joints);glDeleteTextures(1,&r->joint_texture);
    glDeleteVertexArrays(1,&r->grid_vao);
    if(r->program)glDeleteProgram(r->program);
    if(r->solid_program)glDeleteProgram(r->solid_program);
    *r=(Renderer){0};
}
bool renderer_resize(Renderer *r,int w,int h,int dw,int dh) {
    if(w<=0||h<=0||dw<=0||dh<=0)return false;
    if(r->width==w&&r->height==h&&r->drawable_width==dw&&r->drawable_height==dh)return true;
    r->width=w;r->height=h;r->drawable_width=dw;r->drawable_height=dh;r->pixel_scale=(float)dw/w;
    glViewport(0,0,dw,dh);
    fprintf(stderr,"Window: %dx%d; framebuffer: %dx%d\n",w,h,dw,dh);
    return true;
}
bool renderer_draw_hud(Renderer *r){
    if(!hud_draw(&r->hud,r->width,r->height))return false;
    r->last_upload_bytes+=r->hud.last_upload_bytes;
    return true;
}
static RenderModel *get_model(Renderer *r,uint32_t id) {
    for(size_t i=0;i<r->model_count;i++)if(r->models[i].id==id)return &r->models[i];
    IoModel source;
    if(!io_model_get(id,&source)||source.index_count>INT_MAX)return NULL;
    RenderModel *models=realloc(r->models,(r->model_count+1)*sizeof(*models));
    if(!models)return NULL;
    r->models=models;
    RenderModel *m=&r->models[r->model_count++];
    *m=(RenderModel){.id=id,.index_count=(int)source.index_count,.topology=source.topology};
    glGenVertexArrays(1,&m->vao);glBindVertexArray(m->vao);
    glGenBuffers(1,&m->vbo);glBindBuffer(GL_ARRAY_BUFFER,m->vbo);
    glBufferData(GL_ARRAY_BUFFER,(GLsizeiptr)(source.vertex_count*sizeof(IoVertex)),source.vertices,GL_STATIC_DRAW);
    glEnableVertexAttribArray(0);
    glVertexAttribPointer(0,3,GL_FLOAT,GL_FALSE,sizeof(IoVertex),(void *)offsetof(IoVertex,position));
    glEnableVertexAttribArray(6);glVertexAttribPointer(6,3,GL_FLOAT,GL_FALSE,sizeof(IoVertex),(void *)offsetof(IoVertex,normal));
    glEnableVertexAttribArray(7);glVertexAttribPointer(7,4,GL_FLOAT,GL_FALSE,sizeof(IoVertex),(void *)offsetof(IoVertex,color));
    glEnableVertexAttribArray(8);glVertexAttribPointer(8,3,GL_FLOAT,GL_FALSE,sizeof(IoVertex),(void *)offsetof(IoVertex,emission));
    glEnableVertexAttribArray(9);glVertexAttribIPointer(9,4,GL_UNSIGNED_INT,sizeof(IoVertex),(void *)offsetof(IoVertex,joints));
    glEnableVertexAttribArray(10);glVertexAttribPointer(10,4,GL_FLOAT,GL_FALSE,sizeof(IoVertex),(void *)offsetof(IoVertex,weights));
    glGenBuffers(1,&m->ebo);glBindBuffer(GL_ELEMENT_ARRAY_BUFFER,m->ebo);
    glBufferData(GL_ELEMENT_ARRAY_BUFFER,(GLsizeiptr)(source.index_count*sizeof(uint32_t)),source.indices,GL_STATIC_DRAW);
    return m;
}
static void instance_attributes(Renderer *r,RenderModel *model,size_t start){
    glBindVertexArray(model->vao);glBindBuffer(GL_ARRAY_BUFFER,r->instances.id);
    glEnableVertexAttribArray(11);glVertexAttribDivisor(11,1);
    glVertexAttribIPointer(11,1,GL_UNSIGNED_INT,sizeof(IoInstance),
        (void *)(start*sizeof(IoInstance)+offsetof(IoInstance,joint_offset)));
    for(GLuint i=0;i<4;i++){
        glEnableVertexAttribArray(1+i);glVertexAttribDivisor(1+i,1);
        size_t offset=start*sizeof(IoInstance)+offsetof(IoInstance,transform)+i*4*sizeof(float);
        glVertexAttribPointer(1+i,4,GL_FLOAT,GL_FALSE,sizeof(IoInstance),(void *)offset);
    }
    glEnableVertexAttribArray(5);glVertexAttribDivisor(5,1);
    glVertexAttribPointer(5,3,GL_FLOAT,GL_FALSE,sizeof(IoInstance),
        (void *)(start*sizeof(IoInstance)+offsetof(IoInstance,color)));
}
bool renderer_draw(Renderer *r,const IoFrame *frame) {
    r->last_upload_bytes=0;
    if(frame->joint_count>(size_t)r->max_joint_matrices)return false;
    if(frame->instance_count>INT_MAX||frame->grid_vertex_count>INT_MAX)return false;
    if(!r->frame_uploaded || frame->serial!=r->frame_serial){
        r->frame_uploaded=false;
        const float identity[16]={1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1};
        if(!dynamic_buffer_upload(&r->instances,frame->instances,frame->instance_count,sizeof(IoInstance)) ||
           !dynamic_buffer_upload(&r->grid,frame->grid,frame->grid_vertex_count,sizeof(IoVec3)) ||
           !dynamic_buffer_upload(&r->joints,frame->joint_count?frame->joint_matrices:identity,
                                  frame->joint_count?frame->joint_count:1,16*sizeof(float))){
            fprintf(stderr,"Frame streaming upload rejected or failed\n");return false;
        }
        glActiveTexture(GL_TEXTURE0);glBindTexture(GL_TEXTURE_BUFFER,r->joint_texture);
        glTexBuffer(GL_TEXTURE_BUFFER,GL_RGBA32F,r->joints.id);
        if(glGetError()!=GL_NO_ERROR)return false;
        r->last_upload_bytes=r->instances.used+r->grid.used+r->joints.used;
        r->frame_serial=frame->serial;
        r->frame_uploaded=true;
    }
    glStencilMask(0xff);
    glClearColor(0.13f,0.17f,0.19f,1.);glClear(GL_COLOR_BUFFER_BIT|GL_DEPTH_BUFFER_BIT|GL_STENCIL_BUFFER_BIT);
    glUseProgram(r->program);
    glUniformMatrix4fv(r->matrix_uniform,1,GL_FALSE,frame->clip_from_world);
    glUniform2f(r->viewport_uniform,(float)r->drawable_width,(float)r->drawable_height);
    glUniform1f(r->width_uniform,0.65f*r->pixel_scale);
    glBindVertexArray(r->grid_vao);
    for(GLuint i=0;i<4;i++){
        GLfloat column[4]={0,0,0,0};column[i]=1.f;
        glVertexAttrib4fv(1+i,column);
    }
    glVertexAttrib3f(5,0.28f,0.24f,0.06f);
    glDrawArrays(GL_LINES,0,(GLsizei)frame->grid_vertex_count);
    glUseProgram(r->solid_program);
    glActiveTexture(GL_TEXTURE0);glBindTexture(GL_TEXTURE_BUFFER,r->joint_texture);
    glUniform1i(r->joints_uniform,0);
    glUniformMatrix4fv(r->solid_matrix_uniform,1,GL_FALSE,frame->clip_from_world);
    glUniform2f(r->outline_offset_uniform,0,0);glUniform1i(r->outline_mode_uniform,0);
    for(size_t start=0;start<frame->instance_count;){
        uint32_t id=frame->instances[start].model_id;
        size_t end=start+1;
        while(end<frame->instance_count&&frame->instances[end].model_id==id)end++;
        RenderModel *model=get_model(r,id);
        if(!model){fprintf(stderr,"Missing render model %u\n",id);return false;}
        instance_attributes(r,model,start);
        glDrawElementsInstanced(model->topology,model->index_count,GL_UNSIGNED_INT,(void *)0,(GLsizei)(end-start));
        start=end;
    }
    if(r->hud.game.enabled && r->hud.game.selected_item && r->stencil_bits>0){
        for(size_t i=0;i<frame->instance_count;i++)if(frame->instances[i].item_id==r->hud.game.selected_item){
            RenderModel *model=get_model(r,frame->instances[i].model_id);if(!model)return false;
            instance_attributes(r,model,i);
            // Stamp visible selected geometry, then dilate its silhouette in screen pixels.
            glEnable(GL_STENCIL_TEST);glStencilFunc(GL_ALWAYS,1,0xff);glStencilOp(GL_KEEP,GL_KEEP,GL_REPLACE);
            glColorMask(GL_FALSE,GL_FALSE,GL_FALSE,GL_FALSE);glDepthMask(GL_FALSE);
            glDrawElementsInstanced(model->topology,model->index_count,GL_UNSIGNED_INT,0,1);
            glColorMask(GL_TRUE,GL_TRUE,GL_TRUE,GL_TRUE);
            glStencilMask(0);glStencilFunc(GL_NOTEQUAL,1,0xff);glStencilOp(GL_KEEP,GL_KEEP,GL_KEEP);
            glUniform1i(r->outline_mode_uniform,1);
            for(int y=-1;y<=1;y++)for(int x=-1;x<=1;x++)if(x||y){
                glUniform2f(r->outline_offset_uniform,4.f*x/r->width,4.f*y/r->height);
                glDrawElementsInstanced(model->topology,model->index_count,GL_UNSIGNED_INT,0,1);
            }
            glUniform1i(r->outline_mode_uniform,0);glUniform2f(r->outline_offset_uniform,0,0);
            glDepthMask(GL_TRUE);glStencilMask(0xff);glDisable(GL_STENCIL_TEST);
            break;
        }
    }
    if(r->hud.game.enabled && r->hud.game.projectile_count){
        unsigned int count=r->hud.game.projectile_count<4?r->hud.game.projectile_count:4;
        glUseProgram(r->effect_program);glBindVertexArray(r->effect_vao);glBindBuffer(GL_ARRAY_BUFFER,r->effect_vbo);
        glBufferSubData(GL_ARRAY_BUFFER,0,count*sizeof(IoProjectileView),r->hud.game.projectiles);
        r->last_upload_bytes+=count*sizeof(IoProjectileView);
        glUniformMatrix4fv(r->effect_matrix_uniform,1,GL_FALSE,frame->clip_from_world);
        const float *m=frame->clip_from_world;
        float pixels=0.5f*r->drawable_width*sqrtf(m[0]*m[0]+m[4]*m[4]+m[8]*m[8]);
        glUniform1f(r->effect_scale_uniform,pixels);
        glEnable(GL_PROGRAM_POINT_SIZE);glDepthMask(GL_FALSE);glDrawArrays(GL_POINTS,0,(GLsizei)count);
        glDepthMask(GL_TRUE);glDisable(GL_PROGRAM_POINT_SIZE);
    }
    glBindVertexArray(0);
    GLenum error=glGetError();
    if(error!=GL_NO_ERROR){fprintf(stderr,"OpenGL draw error: 0x%x\n",error);return false;}
    return true;
}
