# Huge District

```sh
make release
./build/release/io --scene assets/district/huge.json
```

This scene spans 2,048 world units across 32 by 32 city blocks in the retained
10,000 by 10,000 world. The surrounding forest extends farther. It contains
80,446 placed items: 8,192 moving characters, 7,280 houses, 29,108 trees, 10,560
road sections, 4,096 lamps, 3,000 market stalls, 9,000 crates, 3,000 barrels,
4,096 benches, and 2,114 ground/intersection tiles.

The initial camera opens at overview scale. Scroll up or press + repeatedly to
inspect a neighborhood; right-drag or arrow keys pan. N creates a second camera
and Tab switches between retained views. R returns to the original overview.
The [ and ] keys change render/simulation radius.

The scene sets `camera.render_distance` to 1,709 units. Existing scenes without
that field retain 120 units; values must be 8..20,000. This radius remains
independent of zoom. A wide radius keeps many NPCs active even while zoomed in;
all retained cameras contribute to simulation selection.

Buildings, lamps, runners, trees, and road segments select high/low geometry by
projected size. Tiny crates, benches, and barrels disappear below a 2-pixel
threshold while remaining in world state. Shared procedural glTF meshes provide
pine trees, stalls, furniture, and simplified roads. Original Blender models and
their LODs provide the street architecture and characters.

The generator is deterministic and writes ordinary scene JSON plus shared glTF
assets. No district-specific behavior was added to the renderer or world:

```sh
python3 tools/build_district.py
python3 tools/build_district.py --blocks 40 --npcs-per-block 12 --forest-trees 40000 --output build/district-larger/scene.json
```

The first command regenerates the shipped scene and its `.stats.json` count
manifest. The second creates a separate larger workload. Density controls scale
the workload, with no requirement to fill every world grid location.

To measure the native renderer with the same scene:

```sh
./build/release/io --scene assets/district/huge.json --benchmark-out build/huge-benchmark.json --benchmark-frames 300 --warmup 120 --benchmark-orbit --benchmark-watch
```

Enter advances past the preview and ends post-run inspection. Keep the window
size unchanged during measurement. The benchmark currently overrides render
distance to 20,000, so it can include more edge-of-map objects than normal play.
Reports retain actual visible/candidate counts and CPU/GPU timings.

This is a mixed rendering and route-simulation workload. Characters follow
independent authored routes; there is no navigation AI, crowd avoidance,
collision, forest transparency, shadow rendering, or asset streaming. Repeated
meshes and simple opaque materials make it cheaper than an equally populated
game with unique assets and complex lighting.
