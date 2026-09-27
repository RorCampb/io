# Mixamo Action Adventure Pack

The downloaded FBXs are converted offline through Blender, not loaded directly
by the engine. Original files in Downloads are untouched. No renderer, world or
game-rule source changes are needed to load this package.

## View It

From the repository root:

```sh
./build/release/io --scene assets/action-adventure/preview.json
```

The three figures are idle, walking in place, and running in place. Scroll zooms,
left-drag orbits and right/middle-drag pans (this is a non-game preview scene).

To see the whole pack or an individual clip:

```sh
./build/release/io --scene assets/action-adventure/gallery.json
./build/release/io --scene assets/action-adventure/clips/running_in_place.json
```

The gallery is sorted by clip identifier, six columns per row. `catalog.json`
maps identifiers to original filenames, duration, source travel and nominal speed.
All previews loop for inspection, including actions normally played once; turns,
landings and cover transitions may visibly reset at the loop boundary.

## Delivered Assets

- X Bot: 65 joints and 49,112 triangles, original opaque material colors.
- 22 original clips with their authored skeletal travel preserved.
- Separate `walking_in_place` and `running_in_place` clips with net horizontal
  travel removed. Vertical motion and within-cycle hip sway remain.
- `x_bot.glb`, strict `package.json`, editable compressed `source.blend`, catalog,
  three-character preview, full gallery, and individual clip scenes.

Walking's original travel is approximately 1.786 m/s and running's 4.749 m/s at
1x playback. Matching game travel to these speeds, or scaling playback accordingly,
is needed to avoid foot sliding. Root motion is currently a visual skeleton motion,
not automatically applied to the Item's collision/controller position.

The FBXs share a hierarchy and proportions, but the bind skeleton has a small
origin offset and slightly different finger bone axes. The importer validates
joint correspondence, parent relationships, uniform scale and positions within
one millimeter after origin alignment. It bakes source deformation onto the
character's bind basis, verifies sampled poses, and exports linear keyframes.
It rejects incompatible proportions rather than pretending to retarget arbitrary
characters. This does not rig or retarget the existing drawing-inspired adventurer.

No locomotion state machine, blending, foot IK, root-motion controller, or automatic
walk/run selection has been added. These scenes prove imported clip playback;
existing village characters and gameplay remain unchanged. No LOD reduction has
been generated for this relatively detailed model.

## Rebuild

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --python-exit-code 1 \
  --python tools/import_mixamo.py -- \
  --source "/Users/rorycampbell/Downloads/Action Adventure Pack" --overwrite
cargo run --offline --bin io-validate -- package assets/action-adventure/package.json
cargo run --offline --bin io-validate -- scene assets/action-adventure/preview.json
```

`--inspect` writes a bone/material/action report instead of building. `--output`
selects another directory; `--character` selects the character FBX. `--in-place`
lists normalized clip identifiers to adapt (defaults to walking and running).
An empty `--in-place` list preserves only the original clips.

Rebuilding with `--overwrite` replaces generated outputs, including source.blend;
keep hand-edited Blender work at another path. The importer will reject textured
or transparent character materials instead of silently discarding them. The engine
package validator remains the final authority on supported exported material data.
