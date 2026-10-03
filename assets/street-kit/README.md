# Street Kit

Original procedural demo models in a shared warm, stylized palette. The editable
source is `street-kit.blend`. Its scene selector contains each individual asset,
`Street preview`, and `Runner preview`. In `Runner preview`, play frames 1-24 to
see the looping run. In `runner`, select `RunnerRig` to inspect or edit the bones.

## Contents

| Export | Contents |
| --- | --- |
| `runner.glb` | Mustard-jacket character, a 14-bone skeleton, skin weights, and `Run` clip |
| `house_cottage.glb` | Sage cottage, gabled roof, chimney, windows, shutters, and door |
| `house_townhouse.glb` | Two-story apricot house with windows, cornice, canopy, and chimney |
| `road_straight.glb` | A 10-meter road segment with markings, curbs, and sidewalks |
| `street_lamp.glb` | Iron lamp with brass details and an emissive lantern lens |
| `street_tree.glb` | Small planted street tree |

`street-preview.png` and `runner-preview.png` are Blender renders, not screenshots
from the io engine. `run-cycle.mp4` shows the run loop. `catalog.json` records
dimensions, triangle counts, and exported animation/skin information.

## Placement and Animation

One Blender unit is one meter. Assets use Z-up in Blender and Y-up in glTF.
The character and house fronts face Blender -Y (glTF +Z). The road connects at
Y = -5 and Y = +5 in Blender, so copies tile at 10-meter intervals. Road surface
height is Z = 0; sidewalk paving is at Z = 0.14.

The character has segmented, single-bone-weighted limbs: an intentionally simple
stylized rig, not a production anatomical rig. The baked `Run` animation is one
second long at 24 fps; source frame 25 repeats frame 1. It runs in place, with
opposing arm swings, bent knees, foot lifts, body bounce, and brief airborne
transitions. World movement is a separate engine responsibility. A starting
travel speed around 1.2 m/s suits the short stride; adjust playback speed and
travel together when integrating it.

All colors use glTF material factors; there are no external textures to manage.
The lantern has an emissive material but does not export a scene light.

The existing io importer uniformly normalizes meshes and removes their minimum
corner. For a static asset, use `uniform_import_size` from the catalog on all
three item-size axes to restore authored dimensions. To preserve an authored
origin at P, use item anchor `P + rotation * bounds_min` (Blender/world axes).
Skeletal import will need to preserve the bind-space relationship between
geometry, joints, and inverse-bind matrices; do not normalize animated vertices
independently of their skeleton.

The io application still selects the donut. It needs the planned asset registry,
material rendering, and skeletal playback to display this complete animated demo.

## Rebuild

From the repository root:

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup --python tools/build_street_kit.py
ffmpeg -y -framerate 24 -i assets/street-kit/run-frames/run_%03d.png -c:v libx264 -pix_fmt yuv420p -movflags +faststart assets/street-kit/run-cycle.mp4
```

The generator overwrites its outputs in this directory. Keep hand-edited copies
under a different filename if you intend to regenerate the kit.
