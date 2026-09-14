#include "../csrc/hud.h"
#include <assert.h>
#include <math.h>
#include <stdio.h>
#include <string.h>

int main(void){
    Hud hud={0};char lines[HUD_LINE_COUNT][HUD_LINE_LENGTH];hud_reset(&hud);hud_lines(&hud,lines);
    assert(strcmp(lines[0],"FPS --")==0);
    assert(strcmp(lines[3],"BUDGET -- MS")==0);
    assert(strcmp(lines[4],"FRAME -- MS")==0);
    assert(strcmp(lines[5],"UPDATE SYNC")==0);
    hud_set_tick_hz(&hud,144);hud_lines(&hud,lines);
    assert(strcmp(lines[3],"BUDGET 6.944 MS")==0);
    IoWorkerStats worker={.status=1};
    hud_presented(&hud,10.,&worker);
    for(unsigned int i=1;i<=30;i++){
        worker.tick=i/2;worker.snapshot_age_ms=12.;
        hud_presented(&hud,10.+(double)i/60.,&worker);
    }
    assert(hud.measured && fabs(hud.fps-60.)<0.01);
    assert(hud.simulation_hz>25 && hud.simulation_hz<35);
    hud_lines(&hud,lines);assert(strcmp(lines[0],"FPS 60.0")==0);
    assert(strcmp(lines[2],"AGE 12 MS")==0);
    assert(strcmp(lines[4],"FRAME 16.667 MS")==0);
    hud_presented(&hud,12.,&worker);
    assert(hud.fps<1 && hud.simulation_hz==0.);
    hud_lines(&hud,lines);assert(strcmp(lines[3],"BUDGET 6.944 MS")==0);
    assert(strcmp(lines[5],"UPDATE STALLED")==0);
    worker.status=3;hud_presented(&hud,13.,&worker);hud_lines(&hud,lines);
    assert(strcmp(lines[1],"SIM STOPPED")==0);
    assert(strcmp(lines[5],"UPDATE STOPPED")==0);
    hud_presented(&hud,NAN,&worker);assert(isfinite(hud.fps));
    hud_reset(&hud);hud_presented(&hud,20.,NULL);hud_presented(&hud,20.5,NULL);
    assert(hud.fps==2.);hud_lines(&hud,lines);assert(strcmp(lines[1],"SIM SYNC")==0);
    assert(strcmp(lines[3],"BUDGET 6.944 MS")==0);
    assert(strcmp(lines[4],"FRAME 500.000 MS")==0);
    assert(strcmp(lines[5],"UPDATE SYNC")==0);
    hud.dirty=false;hud_set_tick_hz(&hud,30);assert(hud.dirty);
    hud_lines(&hud,lines);assert(strcmp(lines[3],"BUDGET 33.333 MS")==0);
    hud_set_tick_hz(&hud,1000);hud_lines(&hud,lines);assert(strcmp(lines[3],"BUDGET 1.000 MS")==0);
    hud_set_tick_hz(&hud,4);hud_lines(&hud,lines);assert(strcmp(lines[3],"BUDGET 250.000 MS")==0);
    hud_set_tick_hz(&hud,0);hud_lines(&hud,lines);assert(strcmp(lines[3],"BUDGET -- MS")==0);
    hud_presented(&hud,19.,NULL);assert(!hud.measured);
    // Fast presentation must not disguise a slow or frozen explosion.
    for(unsigned int ticks=0;ticks<=36;ticks+=18){
        hud_reset(&hud);hud_set_tick_hz(&hud,144);
        worker=(IoWorkerStats){.status=1};hud_presented(&hud,30.,&worker);
        for(unsigned int i=1;i<=36;i++){
            worker.tick=(uint64_t)i*ticks/36;
            hud_presented(&hud,30.+(double)i/144.,&worker);
        }
        hud_lines(&hud,lines);
        assert(strcmp(lines[4],"FRAME 6.944 MS")==0);
        const char *expected=ticks==0?"UPDATE STALLED":ticks==18?"UPDATE 13.889 MS":"UPDATE 6.944 MS";
        assert(strcmp(lines[5],expected)==0);
    }
    puts("PASS: presented FPS, independent simulation rate, fixed timestep, stalls, reset, and invalid time");
}
