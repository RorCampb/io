#include <SDL2/SDL.h>
#include <OpenGL/gl3.h>
#include <string.h>
#include <inttypes.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <math.h>

#include "io.h"
#include "renderer.h"
#include "input.h"
#include "benchmark.h"

static bool parse_count(const char *text,unsigned int *out) {
    char *end=NULL;
    unsigned long value=strtoul(text,&end,10);
    if(end==text || *end || value>10000)return false;
    *out=(unsigned int)value;return true;
}

static bool capture_smoke_frame(int width,int height,const char *path) {
    size_t stride=(size_t)width*3;
    unsigned char *pixels=malloc(stride*(size_t)height);
    if(!pixels)return false;
    glPixelStorei(GL_PACK_ALIGNMENT,1);
    glReadPixels(0,0,width,height,GL_RGB,GL_UNSIGNED_BYTE,pixels);
    size_t lit=0;
    for(size_t i=0;i<stride*(size_t)height;i+=3)if(pixels[i]>30)lit++;
    bool ok=glGetError()==GL_NO_ERROR && lit>100;
    FILE *file=fopen(path,"wb");
    if(file){
        fprintf(file,"P6\n%d %d\n255\n",width,height);
        for(int y=height-1;y>=0;y--)fwrite(pixels+(size_t)y*stride,1,stride,file);
        fclose(file);
    }
    free(pixels);
    fprintf(stderr,"Framebuffer readback: %zu lit pixels\n",lit);
    return ok;
}

static bool create_window(SDL_Window **window, SDL_GLContext *context) {
    const int sample_counts[] = {4, 2, 0};
    for (size_t i = 0; i < sizeof(sample_counts) / sizeof(sample_counts[0]); i += 1) {
        int samples = sample_counts[i];
        if (SDL_GL_SetAttribute(SDL_GL_MULTISAMPLEBUFFERS, samples > 0 ? 1 : 0) != 0 ||
            SDL_GL_SetAttribute(SDL_GL_MULTISAMPLESAMPLES, samples) != 0) {
            fprintf(stderr, "Cannot request %dx MSAA: %s\n", samples, SDL_GetError());
            continue;
        }
        *window = SDL_CreateWindow(
            "io", SDL_WINDOWPOS_CENTERED, SDL_WINDOWPOS_CENTERED, 1280, 800,
            SDL_WINDOW_OPENGL | SDL_WINDOW_RESIZABLE | SDL_WINDOW_ALLOW_HIGHDPI
        );
        if (*window != NULL) {
            *context = SDL_GL_CreateContext(*window);
            if (*context != NULL) {
                return true;
            }
        }
        fprintf(stderr, "OpenGL startup with %d samples failed: %s\n", samples, SDL_GetError());
        if (*window != NULL) {
            SDL_DestroyWindow(*window);
            *window = NULL;
        }
    }
    return false;
}

int main(int argc, char **argv) {
    bool smoke=false;
    bool capture=false;
    double capture_at=0.;
    BenchmarkOptions benchmark={.frames=600,.warmup=120};
    for(int i=1;i<argc;i++){
        if(strcmp(argv[i],"--smoke-test")==0)smoke=true;
        else if(strcmp(argv[i],"--scene")==0 && i+1<argc){
            if(setenv("IO_SCENE",argv[++i],1)!=0){perror("IO_SCENE");return 1;}
        }else if(strcmp(argv[i],"--capture-at")==0 && i+1<argc){
            char *end=NULL;
            const char *value=argv[++i];
            capture_at=strtod(value,&end);
            if(end==value || *end || !isfinite(capture_at) || capture_at<0. || capture_at>60.){
                fprintf(stderr,"Capture time must be between 0 and 60 seconds\n");return 1;
            }
            capture=true;
        }else if(strcmp(argv[i],"--benchmark-out")==0 && i+1<argc){
            benchmark.output=argv[++i];
        }else if(strcmp(argv[i],"--benchmark-frames")==0 && i+1<argc){
            if(!parse_count(argv[++i],&benchmark.frames)||!benchmark.frames)return 1;
        }else if(strcmp(argv[i],"--warmup")==0 && i+1<argc){
            if(!parse_count(argv[++i],&benchmark.warmup))return 1;
        }else if(strcmp(argv[i],"--benchmark-orbit")==0){
            benchmark.orbit=true;
        }else if(strcmp(argv[i],"--benchmark-watch")==0){
            benchmark.watch=true;
        }else{fprintf(stderr,"Usage: %s [--scene scene.json] [--smoke-test | --capture-at seconds | --benchmark-out result.json [--benchmark-frames N] [--warmup N] [--benchmark-orbit]]\n",argv[0]);return 1;}
    }
    if(smoke && capture){fprintf(stderr,"Choose either smoke testing or capture\n");return 1;}
    if(benchmark.output && (smoke||capture)){fprintf(stderr,"Benchmark cannot run with smoke/capture\n");return 1;}
    int smoke_frame = 0;
    float *first_pose=NULL;size_t first_joint_count=0;
    if (SDL_Init(SDL_INIT_VIDEO) != 0) {
        fprintf(stderr, "SDL_Init failed: %s\n", SDL_GetError());
        return 1;
    }

    SDL_GL_SetAttribute(SDL_GL_CONTEXT_MAJOR_VERSION, 4);
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_MINOR_VERSION, 1);
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_PROFILE_MASK, SDL_GL_CONTEXT_PROFILE_CORE);
    SDL_GL_SetAttribute(SDL_GL_DEPTH_SIZE, 24);
    SDL_GL_SetAttribute(SDL_GL_DOUBLEBUFFER, 1);

    SDL_Window *window = NULL;
    SDL_GLContext context = NULL;
    if (!create_window(&window, &context)) {
        SDL_Quit();
        return 1;
    }

    if (SDL_GL_MakeCurrent(window, context) != 0) {
        fprintf(stderr, "SDL_GL_MakeCurrent failed: %s\n", SDL_GetError());
        SDL_GL_DeleteContext(context);
        SDL_DestroyWindow(window);
        SDL_Quit();
        return 1;
    }

    bool vsync = SDL_GL_SetSwapInterval(1) == 0;

    Renderer renderer = {0};
    if (!renderer_init(&renderer)) {
        SDL_GL_DeleteContext(context);
        SDL_DestroyWindow(window);
        SDL_Quit();
        return 1;
    }

    IoApp *app = io_app_new();
    if (app == NULL) {
        renderer_destroy(&renderer);
        SDL_GL_DeleteContext(context);
        SDL_DestroyWindow(window);
        SDL_Quit();
        return 1;
    }

    if(benchmark.output){
        int result=benchmark_run(window,&renderer,app,&benchmark);
        io_app_free(app);renderer_destroy(&renderer);
        SDL_GL_DeleteContext(context);SDL_DestroyWindow(window);SDL_Quit();
        return result;
    }
    if(capture){
        /* Deterministic simulation advance for visual regression snapshots. */
        double remaining=capture_at;
        while(remaining>0.){
            double step=remaining<0.125?remaining:0.125;
            io_app_update(app,(float)step);
            remaining-=step;
        }
    }
    bool running = true;
    uint64_t last_title = 0;
    uint64_t last_tick = SDL_GetPerformanceCounter();
    int exit_code = 0;
    while (running) {
        SDL_Event event;
        while (SDL_PollEvent(&event)) {
            IoAction action;
            InputResult result = input_translate(&event, &action);
            if (result == INPUT_QUIT) {
                running = false;
                break;
            }
            if (result == INPUT_ACTION && !io_app_dispatch(app, action)) {
                fprintf(stderr, "Application action rejected: %u\n", action.kind);
            }
        }

        if (!running) {
            break;
        }
        uint64_t now = SDL_GetPerformanceCounter();
        float elapsed = (float)((double)(now-last_tick)/(double)SDL_GetPerformanceFrequency());
        last_tick=now;
        io_app_update(app,capture?0.f:(smoke?0.125f:elapsed));
        int width, height, drawable_width, drawable_height;
        // Query both sizes each frame to also catch moves between displays with different DPI.
        SDL_GetWindowSize(window, &width, &height);
        SDL_GL_GetDrawableSize(window, &drawable_width, &drawable_height);
        if ((SDL_GetWindowFlags(window) & SDL_WINDOW_MINIMIZED) ||
            !renderer_resize(&renderer, width, height, drawable_width, drawable_height)) {
            SDL_Delay(16);
            continue;
        }

        IoCameraId camera = io_app_active_camera(app);
        IoFrame frame = {0};
        if (!io_app_set_viewport(app, camera, width, height) ||
            !io_app_frame(app, camera, &frame)) {
            fprintf(stderr, "Cannot prepare camera %" PRIu64 "\n", camera);
            io_app_free(app);
            renderer_destroy(&renderer);
            SDL_GL_DeleteContext(context);
            SDL_DestroyWindow(window);
            SDL_Quit();
            return 1;
        }

        if(!renderer_draw(&renderer,&frame)){exit_code=1;break;}
        if(capture){
            if(!capture_smoke_frame(drawable_width,drawable_height,"build/capture.ppm"))exit_code=1;
            fprintf(stderr,"Captured at %.3f seconds to build/capture.ppm\n",capture_at);
            running=false;
        }
        if(SDL_GetTicks64()-last_title>250 || smoke){
            char title[256];
            snprintf(title,sizeof(title),"io | Camera %" PRIu64 " | %zu/%" PRIu64 " visible | %" PRIu64
                " candidates | %" PRIu64 " active | target %.0f,%.0f | distance %.0f",
                camera,frame.instance_count,frame.world_items,frame.candidates,frame.active_simulations,
                frame.target.x,frame.target.y,frame.render_distance);
            SDL_SetWindowTitle(window,title);last_title=SDL_GetTicks64();
        }
        if(smoke){
            if(smoke_frame==0){
                if(frame.instance_count==0 || frame.joint_count==0){exit_code=1;break;}
                if(!capture_smoke_frame(drawable_width,drawable_height,"build/smoke.ppm")){exit_code=1;break;}
                first_joint_count=frame.joint_count;
                first_pose=malloc(first_joint_count*16*sizeof(float));
                if(!first_pose){exit_code=1;break;}
                memcpy(first_pose,frame.joint_matrices,first_joint_count*16*sizeof(float));
                fprintf(stderr,"Visible: %zu / %" PRIu64 "; candidates: %" PRIu64 "\n",
                    frame.instance_count,frame.world_items,frame.candidates);
            }
            if(smoke_frame==1){
                if(frame.joint_count!=first_joint_count || memcmp(first_pose,frame.joint_matrices,first_joint_count*16*sizeof(float))==0){
                    fprintf(stderr,"FAIL: skeletal pose did not advance\n");exit_code=1;break;
                }
                if(!capture_smoke_frame(drawable_width,drawable_height,"build/smoke-animated.ppm")){exit_code=1;break;}
                fprintf(stderr,"PASS: %zu joint matrices animate\n",frame.joint_count);
            }
            smoke_frame++;
            if(smoke_frame==2)io_app_dispatch(app,(IoAction){.kind=IO_ACTION_ORBIT,.x=0.5f,.y=0.1f});
            if(smoke_frame==3)io_app_dispatch(app,(IoAction){.kind=IO_ACTION_ZOOM,.x=-5.f});
            if(smoke_frame==4)io_app_dispatch(app,(IoAction){.kind=IO_ACTION_NEW_CAMERA});
            if(smoke_frame==5)io_app_set_camera_target(app,io_app_active_camera(app),(IoVec3){100.f,100.f,0.f});
            if(smoke_frame==6)io_app_dispatch(app,(IoAction){.kind=IO_ACTION_NEXT_CAMERA});
            if(smoke_frame==7)SDL_SetWindowSize(window,900,650);
            if(smoke_frame==8)running=false;
        }
        SDL_GL_SwapWindow(window);
        if (!vsync) {
            SDL_Delay(16);
        }
    }

    free(first_pose);
    io_app_free(app);
    renderer_destroy(&renderer);
    SDL_GL_DeleteContext(context);
    SDL_DestroyWindow(window);
    SDL_Quit();
    if(smoke && exit_code==0)fprintf(stderr,"PASS: native GPU rendering, camera controls, and resize\n");
    return exit_code;
}
