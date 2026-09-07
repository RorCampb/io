#include <SDL2/SDL.h>
#include <SDL2/SDL_opengl.h>
#include <stdbool.h>
#include <stdio.h>

#include "io.h"
#include "renderer.h"

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

int main(void) {
    if (SDL_Init(SDL_INIT_VIDEO) != 0) {
        fprintf(stderr, "SDL_Init failed: %s\n", SDL_GetError());
        return 1;
    }

    SDL_GL_SetAttribute(SDL_GL_CONTEXT_MAJOR_VERSION, 2);
    SDL_GL_SetAttribute(SDL_GL_CONTEXT_MINOR_VERSION, 1);
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

    IoWorld *world = io_world_new();
    if (world == NULL) {
        renderer_destroy(&renderer);
        SDL_GL_DeleteContext(context);
        SDL_DestroyWindow(window);
        SDL_Quit();
        return 1;
    }

    bool running = true;
    while (running) {
        SDL_Event event;
        while (SDL_PollEvent(&event)) {
            switch (event.type) {
                case SDL_QUIT:
                    running = false;
                    break;
                case SDL_MOUSEMOTION:
                    if (event.motion.state & SDL_BUTTON_LMASK) {
                        io_world_orbit_camera(world, -(float)event.motion.xrel * 0.006f,
                                              (float)event.motion.yrel * 0.006f);
                    }
                    break;
                case SDL_MOUSEWHEEL: {
                    float steps = event.wheel.preciseY;
                    if (event.wheel.direction == SDL_MOUSEWHEEL_FLIPPED) {
                        steps = -steps;
                    }
                    io_world_zoom_camera(world, steps);
                    break;
                }
                case SDL_KEYDOWN:
                    if (event.key.repeat) {
                        break;
                    }
                    switch (event.key.keysym.sym) {
                        case SDLK_ESCAPE:
                            running = false;
                            break;
                        case SDLK_r:
                            io_world_reset_camera(world);
                            break;
                        case SDLK_EQUALS:
                        case SDLK_PLUS:
                        case SDLK_KP_PLUS:
                            io_world_zoom_camera(world, 1.0f);
                            break;
                        case SDLK_MINUS:
                        case SDLK_KP_MINUS:
                            io_world_zoom_camera(world, -1.0f);
                            break;
                        case SDLK_q:
                            io_world_resize_units_a(world, 1);
                            break;
                        case SDLK_a:
                            io_world_resize_units_a(world, -1);
                            break;
                        case SDLK_w:
                            io_world_resize_units_b(world, 1);
                            break;
                        case SDLK_s:
                            io_world_resize_units_b(world, -1);
                            break;
                        case SDLK_e:
                            io_world_resize_units_c(world, 1);
                            break;
                        case SDLK_d:
                            io_world_resize_units_c(world, -1);
                            break;
                        default:
                            break;
                    }
                    break;
                default:
                    break;
            }
        }

        if (!running) {
            break;
        }
        int width, height, drawable_width, drawable_height;
        // Query both sizes each frame to also catch moves between displays with different DPI.
        SDL_GetWindowSize(window, &width, &height);
        SDL_GL_GetDrawableSize(window, &drawable_width, &drawable_height);
        if ((SDL_GetWindowFlags(window) & SDL_WINDOW_MINIMIZED) ||
            !renderer_resize(&renderer, width, height, drawable_width, drawable_height)) {
            SDL_Delay(16);
            continue;
        }

        io_world_set_viewport(world, width, height);

        size_t line_count = 0;
        size_t point_count = 0;
        const IoLine *lines = io_world_lines(world, &line_count);
        const IoPoint *points = io_world_points(world, &point_count);

        renderer_clear();
        renderer_draw_lines(&renderer, lines, line_count);
        renderer_draw_points(&renderer, points, point_count);
        SDL_GL_SwapWindow(window);
        if (!vsync) {
            SDL_Delay(16);
        }
    }

    io_world_free(world);
    renderer_destroy(&renderer);
    SDL_GL_DeleteContext(context);
    SDL_DestroyWindow(window);
    SDL_Quit();
    return 0;
}
