#ifndef IO_H
#define IO_H
#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

typedef struct IoApp IoApp;
typedef uint64_t IoCameraId;
typedef enum IoActionKind {
    IO_ACTION_ORBIT=1, IO_ACTION_ZOOM=2, IO_ACTION_RESET_VIEW=3,
    IO_ACTION_RESIZE_A=4, IO_ACTION_RESIZE_B=5, IO_ACTION_RESIZE_C=6,
    IO_ACTION_NEW_CAMERA=7, IO_ACTION_NEXT_CAMERA=8, IO_ACTION_PAN=9,
    IO_ACTION_DISTANCE=10, IO_ACTION_FOLLOW=11, IO_ACTION_GRID=12
} IoActionKind;
typedef struct IoAction {uint32_t kind; float x,y; int32_t delta;} IoAction;
typedef struct IoVec3 {float x,y,z;} IoVec3;
typedef struct IoVertex {
    IoVec3 position,normal;
    float color[4],emission[3];
    uint32_t joints[4];
    float weights[4];
} IoVertex;
typedef struct IoInstance {
    uint64_t item_id;
    uint32_t model_id; // Concrete mesh selected by Rust for this camera.
    float transform[16];
    float color[3];
    uint32_t joint_offset;
} IoInstance;
typedef struct IoFrame {
    const IoInstance *instances;
    size_t instance_count;
    const IoVec3 *grid;
    size_t grid_vertex_count;
    float clip_from_world[16];
    uint64_t serial,world_items,candidates,active_simulations;
    IoVec3 target;
    float render_distance;
    const float *joint_matrices;
    size_t joint_count;
} IoFrame;
typedef struct IoModel {
    const IoVertex *vertices;
    size_t vertex_count;
    const uint32_t *indices;
    size_t index_count;
    uint32_t topology;
} IoModel;
typedef struct IoItemState {
    uint64_t id;
    IoVec3 anchor,size;
    uint32_t health; // Current durability, or zero if the component is absent.
    uint64_t simulated_ticks;
} IoItemState;
typedef struct IoWorkerStats {
    uint64_t tick,overruns;
    double simulation_ms,snapshot_ms,snapshot_age_ms;
    uint32_t status; // 1 running, 2 stopped, 3 physics failure, 4 worker panic.
} IoWorkerStats;
typedef struct IoDamageText { float x,y,alpha;uint32_t amount; } IoDamageText;
typedef struct IoProjectileView { IoVec3 position;float radius; } IoProjectileView;
typedef struct IoGameView {
    uint64_t selected_item;
    uint32_t enabled,free_movement,line_count,damage_count;
    char lines[12][64];
    IoDamageText damage[16];
    uint32_t projectile_count,reserved;
    IoProjectileView projectiles[4];
} IoGameView;
typedef struct IoGameAction { uint32_t kind,slot; float x,y; } IoGameAction;
/* Owned UI copy; no borrowed strings. Null clears output and returns false. */
bool io_app_game_view(const IoApp *app,IoGameView *out);
/* 1 start, 2 screen move(x,y), 3 select ability(slot), 4 pass, 5 target, 6 player,
   7 click target(logical x,y), 8 orbit, 9 talk, 10 recruit, 11 map toggle.
   Selection does not execute attacks. Exploration actions require that plugin.
   Worker mode returns queue acceptance; rules are validated on the worker. */
bool io_app_game_action(IoApp *app,uint32_t kind,uint32_t slot,float x,float y);

// Handles belong to Rust. Use on one thread; free once. Null handles are accepted.
IoApp *io_app_new(void);
// Real-time simulation worker; the handle and all OpenGL calls stay on the caller thread.
IoApp *io_app_new_realtime(void);
/* Configured target simulation rate; null returns zero. Not measured throughput. */
uint32_t io_app_tick_hz(const IoApp *app);
// Last consumed publication, not a synchronous read of the live worker world.
bool io_app_worker_stats(const IoApp *app,IoWorkerStats *out);
void io_app_free(IoApp *app);
bool io_app_dispatch(IoApp *app,IoAction action);
// Resolve a UTF-8 state name within the item's appearance. Returns true only
// when changed in synchronous mode; in worker mode, true means queued, not applied.
// Invalid item/state/null arguments are rejected. A full worker queue returns false.
// name must be NUL-terminated; consume borrowed frames before calling.
bool io_app_set_visual_state(IoApp *app,uint64_t item_id,const char *name);
IoCameraId io_app_active_camera(const IoApp *app);
// Clone the active camera without selecting it. ID 0 means failure.
IoCameraId io_app_create_camera(IoApp *app);
bool io_app_select_camera(IoApp *app,IoCameraId camera);
bool io_app_set_viewport(IoApp *app,IoCameraId camera,int32_t width,int32_t height);
bool io_app_set_camera_target(IoApp *app,IoCameraId camera,IoVec3 target);
// World-unit radius around the orthographic camera target, clamped to [8,20000].
bool io_app_set_render_distance(IoApp *app,IoCameraId camera,float distance);
void io_app_update(IoApp *app,float seconds);
// In worker mode update polls publications; seconds does not drive the worker clock.
// Column-major matrices. Arrays are borrowed until the next mutable app call or free.
// Invalid handles/IDs clear the output and return false. Never free borrowed arrays.
bool io_app_frame(IoApp *app,IoCameraId camera,IoFrame *out);
// Indexed models; returned arrays are immutable and valid for the process lifetime.
bool io_model_get(uint32_t model_id,IoModel *out);
// Read state whether or not an item is visible. Output is an owned value copy.
bool io_app_item_state(const IoApp *app,uint64_t item_id,IoItemState *out);
#endif
