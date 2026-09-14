#include "benchmark.h"
#include <OpenGL/gl3.h>
#include <inttypes.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/resource.h>
#include "input.h"

typedef struct Sample {
    double update, prepare, submit, frame, gpu;
    size_t visible, candidates, joints, bytes;
} Sample;

static double now_ms(void) {
    return (double)SDL_GetPerformanceCounter()*1000./(double)SDL_GetPerformanceFrequency();
}

static int compare_double(const void *a,const void *b) {
    double x=*(const double *)a,y=*(const double *)b;
    return (x>y)-(x<y);
}

static void json_string(FILE *file,const char *text) {
    fputc('"',file);
    for(const unsigned char *p=(const unsigned char *)text;*p;p++) {
        if(*p=='"'||*p=='\\')fputc('\\',file);
        if(*p<32)fprintf(file,"\\u%04x",*p);else fputc(*p,file);
    }
    fputc('"',file);
}

static void summary(FILE *file,const char *name,double *values,unsigned int count) {
    double sum=0.;
    for(unsigned int i=0;i<count;i++)sum+=values[i];
    qsort(values,count,sizeof(*values),compare_double);
    fprintf(file,"\"%s\":{\"mean\":%.6f,\"p50\":%.6f,\"p95\":%.6f,\"p99\":%.6f,\"max\":%.6f}",
            name,sum/count,values[(size_t)ceil(count*0.50)-1],values[(size_t)ceil(count*0.95)-1],
            values[(size_t)ceil(count*0.99)-1],values[count-1]);
}

static uint64_t growths(const Renderer *r) {
    return r->instances.growths+r->grid.growths+r->joints.growths;
}

// Preview runs a separate world; inspection after measurement holds the measured world.
static int inspect_scene(SDL_Window *window,Renderer *r,IoApp *app,double seconds,bool animate,bool hud) {
    SDL_GL_SetSwapInterval(1);
    SDL_SetWindowTitle(window,animate?"io | Preview: Enter to benchmark, Esc to stop":
                                     "io | Benchmark complete: inspect with mouse, Enter for next, Esc to stop");
    double started=now_ms(),previous=started;
    r->frame_uploaded=false;
    hud_reset(&r->hud);
    while(seconds==0. || now_ms()-started<seconds*1000.) {
        SDL_Event event;
        while(SDL_PollEvent(&event)) {
            if(event.type==SDL_QUIT || (event.type==SDL_KEYDOWN && event.key.keysym.sym==SDLK_ESCAPE))return -1;
            if(event.type==SDL_KEYDOWN && (event.key.keysym.sym==SDLK_RETURN || event.key.keysym.sym==SDLK_KP_ENTER))return 1;
            IoAction action;
            if(input_translate(&event,&action)==INPUT_ACTION)io_app_dispatch(app,action);
        }
        double now=now_ms();
        if(animate)io_app_update(app,(float)((now-previous)/1000.));
        previous=now;
        int w,h,dw,dh;
        SDL_GetWindowSize(window,&w,&h);SDL_GL_GetDrawableSize(window,&dw,&dh);
        if(SDL_GetWindowFlags(window)&SDL_WINDOW_MINIMIZED){SDL_Delay(16);continue;}
        IoFrame frame;
        if(!renderer_resize(r,w,h,dw,dh) || !io_app_set_viewport(app,io_app_active_camera(app),w,h) ||
           !io_app_frame(app,io_app_active_camera(app),&frame) || !renderer_draw(r,&frame))return -2;
        if(hud && !renderer_draw_hud(r))return -2;
        SDL_GL_SwapWindow(window);
        if(SDL_GL_GetSwapInterval()==0)SDL_Delay(16);
        if(hud)hud_presented(&r->hud,now_ms()/1000.,NULL);
    }
    return 0;
}

int benchmark_run(SDL_Window *window,Renderer *r,IoApp *app,const BenchmarkOptions *options) {
    unsigned int count=options->frames;
    if(!count || count>10000 || options->warmup>10000)return 1;
    if(options->watch) {
        IoApp *preview=io_app_new();
        if(!preview)return 1;
        io_app_set_render_distance(preview,io_app_active_camera(preview),20000.f);
        int result=inspect_scene(window,r,preview,3.,true,options->hud);
        io_app_free(preview);
        r->frame_uploaded=false;
        if(result<0)return result==-1?130:1;
    }
    if(SDL_GL_SetSwapInterval(0)!=0 || SDL_GL_GetSwapInterval()!=0) {
        fprintf(stderr,"Benchmark requires swap interval zero: %s\n",SDL_GetError());return 1;
    }
    SDL_SetWindowTitle(window,"io | Measuring benchmark (uncapped); Esc to stop");
    hud_reset(&r->hud);
    GLint bits=0;
    glGetQueryiv(GL_TIME_ELAPSED,GL_QUERY_COUNTER_BITS,&bits);
    if(bits==0){fprintf(stderr,"GPU timer queries unavailable\n");return 1;}
    Sample *samples=calloc(count,sizeof(*samples));
    GLuint *queries=calloc(count,sizeof(*queries));
    double *values=malloc(count*sizeof(*values));
    bool ok=false;
    bool cancelled=false;
    if(!samples||!queries||!values)goto cleanup;
    glGenQueries((GLsizei)count,queries);
    int width,height,dw,dh;
    SDL_GetWindowSize(window,&width,&height);SDL_GL_GetDrawableSize(window,&dw,&dh);
    if(!renderer_resize(r,width,height,dw,dh) ||
       !io_app_set_viewport(app,io_app_active_camera(app),width,height) ||
       !io_app_set_render_distance(app,io_app_active_camera(app),20000.f))goto cleanup;
    double started=0.;
    uint64_t initial_growths=0;
    for(unsigned int i=0;i<options->warmup+count;i++) {
        SDL_Event event;
        while(SDL_PollEvent(&event)) {
            if(event.type==SDL_QUIT || (event.type==SDL_KEYDOWN && event.key.keysym.sym==SDLK_ESCAPE)){
                cancelled=true;goto cleanup;
            }
        }
        int w,h,pw,ph;
        SDL_GetWindowSize(window,&w,&h);SDL_GL_GetDrawableSize(window,&pw,&ph);
        if(w!=width || h!=height || pw!=dw || ph!=dh ||
           (SDL_GetWindowFlags(window)&SDL_WINDOW_MINIMIZED)) {
            fprintf(stderr,"Benchmark interrupted by viewport change\n");goto cleanup;
        }
        bool measuring=i>=options->warmup;
        unsigned int index=measuring?i-options->warmup:0;
        if(i==options->warmup) {
            // Drain warm-up work once; never force a GPU finish inside measured frames.
            glFinish();started=now_ms();initial_growths=growths(r);
        }
        double a=now_ms();
        if(options->orbit && !io_app_dispatch(app,(IoAction){.kind=IO_ACTION_ORBIT,.x=0.012f}))goto cleanup;
        io_app_update(app,1.f/60.f);
        double b=now_ms();
        IoFrame frame;
        if(!io_app_frame(app,io_app_active_camera(app),&frame))goto cleanup;
        double c=now_ms();
        if(measuring)glBeginQuery(GL_TIME_ELAPSED,queries[index]);
        bool drawn=renderer_draw(r,&frame);
        if(drawn && options->hud)drawn=renderer_draw_hud(r);
        if(measuring)glEndQuery(GL_TIME_ELAPSED);
        if(!drawn)goto cleanup;
        double d=now_ms();
        SDL_GL_SwapWindow(window);
        if(options->hud)hud_presented(&r->hud,now_ms()/1000.,NULL);
        double e=now_ms();
        if(measuring)samples[index]=(Sample){
            .update=b-a,.prepare=c-b,.submit=d-c,.frame=e-a,
            .visible=frame.instance_count,.candidates=(size_t)frame.candidates,
            .joints=frame.joint_count,.bytes=r->last_upload_bytes,
        };
    }
    // Results are collected after submission, so querying cannot serialize every frame.
    for(unsigned int i=0;i<count;i++) {
        GLuint64 ns=0;
        glGetQueryObjectui64v(queries[i],GL_QUERY_RESULT,&ns);
        samples[i].gpu=(double)ns/1000000.;
    }
    double elapsed=now_ms()-started;
    if(glGetError()!=GL_NO_ERROR)goto cleanup;
    FILE *file=fopen(options->output,"w");
    if(!file){perror(options->output);goto cleanup;}
    GLint msaa=0;glGetIntegerv(GL_SAMPLES,&msaa);
    struct rusage usage={0};getrusage(RUSAGE_SELF,&usage);
    fprintf(file,"{\n\"schema\":1,\"frames\":%u,\"warmup\":%u,\"fixed_dt\":0.016666667,"
            "\"orbit\":%s,\"watch\":%s,\"swap_interval\":0,\"logical_size\":[%d,%d],\"drawable_size\":[%d,%d],\"msaa\":%d,\n",
            count,options->warmup,options->orbit?"true":"false",options->watch?"true":"false",width,height,dw,dh,msaa);
    fprintf(file,"\"renderer\":");json_string(file,(const char *)glGetString(GL_RENDERER));
    fprintf(file,",\"hud\":%s",options->hud?"true":"false");
    fprintf(file,",\"simulation_tick_hz\":%u",io_app_tick_hz(app));
    fprintf(file,",\"gl_version\":");json_string(file,(const char *)glGetString(GL_VERSION));
    fprintf(file,",\"scene\":");json_string(file,getenv("IO_SCENE")?getenv("IO_SCENE"):"default");
    fprintf(file,",\n\"throughput_fps\":%.4f,\"wall_ms_including_gpu_drain\":%.4f,"
            "\"peak_process_rss_bytes\":%ld,\"dynamic_capacity_bytes\":%zu,\"measured_capacity_growths\":%" PRIu64 ",\n",
            count*1000./elapsed,elapsed,usage.ru_maxrss,
            r->instances.capacity+r->grid.capacity+r->joints.capacity,growths(r)-initial_growths);
    fprintf(file,"\"timings_ms\":{");
    const char *names[]={"update","prepare","submit","frame","gpu"};
    for(int field=0;field<5;field++) {
        for(unsigned int i=0;i<count;i++) {
            Sample s=samples[i];
            values[i]=field==0?s.update:field==1?s.prepare:field==2?s.submit:field==3?s.frame:s.gpu;
        }
        if(field)fputc(',',file);
        summary(file,names[field],values,count);
    }
    fprintf(file,"},\n\"samples\":[\n");
    for(unsigned int i=0;i<count;i++) {
        Sample s=samples[i];
        fprintf(file,"%s{\"update_ms\":%.6f,\"prepare_ms\":%.6f,\"submit_ms\":%.6f,\"frame_ms\":%.6f,"
                "\"gpu_ms\":%.6f,\"visible\":%zu,\"candidates\":%zu,\"joints\":%zu,\"upload_bytes\":%zu}",
                i?",\n":"",s.update,s.prepare,s.submit,s.frame,s.gpu,s.visible,s.candidates,s.joints,s.bytes);
    }
    fprintf(file,"\n]}\n");
    bool wrote=ferror(file)==0;
    ok=fclose(file)==0 && wrote;
    if(ok)fprintf(stderr,"Benchmark: %.1f fps, %u measured frames; %s\n",count*1000./elapsed,count,options->output);
    if(ok && options->watch) {
        fprintf(stderr,"Results saved. Enter continues; Esc stops the sweep.\n");
        int result=inspect_scene(window,r,app,0.,false,options->hud);
        cancelled=result==-1;
        if(result==-2)ok=false;
    }
cleanup:
    if(queries && queries[0])glDeleteQueries((GLsizei)count,queries);
    free(samples);free(queries);free(values);
    return cancelled?130:ok?0:1;
}
