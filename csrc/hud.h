#ifndef IO_HUD_H
#define IO_HUD_H
#include "io.h"
enum { HUD_LINE_COUNT=6, HUD_LINE_LENGTH=48 };

typedef struct Hud {
    unsigned int program,vao,vbo;
    int viewport_uniform,vertices,width,height;
    bool dirty,sampling,measured,worker;
    double sample_start,fps,simulation_hz;
    uint64_t sample_frames,start_tick;
    IoWorkerStats stats;
    uint32_t tick_hz;
    IoGameView game;
    size_t last_upload_bytes;
} Hud;

bool hud_init(Hud *hud);
void hud_destroy(Hud *hud);
void hud_reset(Hud *hud);
// Startup timing is retained across sampling resets; zero means unavailable.
void hud_set_tick_hz(Hud *hud,uint32_t tick_hz);
void hud_set_game(Hud *hud,const IoGameView *game);
bool hud_contains_point(const Hud *hud,float x,float y);
// Call only after a completed swap, using a monotonic clock in seconds.
void hud_presented(Hud *hud,double now,const IoWorkerStats *worker);
void hud_lines(const Hud *hud,char lines[HUD_LINE_COUNT][HUD_LINE_LENGTH]);
bool hud_draw(Hud *hud,int logical_width,int logical_height);
#endif
