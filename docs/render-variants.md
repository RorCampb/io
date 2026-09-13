# Render Variants

Items' optional `Renderable` components reference `AppearanceId` and `VisualStateId`
from `io-types`. The world crate retains these alongside gameplay components and
has no mesh or camera dependency. Items without renderables are not drawn.
The root application's catalog resolves each appearance to a canonical base asset
and a set of visual states. Each state contains an ordered LOD group.

Projection chooses a variant for each camera and emits its concrete mesh ID into
the existing `IoInstance.model_id` field. C caches that mesh's immutable geometry
and draws instances grouped by mesh, including its imported material factors.
Neither C's draw interface nor the instance buffer layout changed for variants.

## Scene Data

Appearances can also live in reusable [asset packages](asset-packages.md), imported
by namespace. The same resolver validates both inline and packaged definitions.

Existing items using `"asset": "tree"` retain their original behavior: an implicit
single-mesh appearance, with no size-based omission. New items may instead name
an explicit appearance. Exactly one of `asset` or `appearance` is required.
Asset and appearance names occupy separate namespaces.

This fragment belongs in a complete version-1 scene. Its files must exist relative
to that scene's directory:

```json
{
  "assets": [
    { "name": "tree_high", "file": "tree-high.glb" },
    { "name": "tree_low", "file": "tree-low.glb" },
    { "name": "stump", "file": "stump.glb" }
  ],
  "appearances": [{
    "name": "tree",
    "base_asset": "tree_high",
    "hysteresis": 0.1,
    "lods": [
      { "asset": "tree_high", "min_screen_pixels": 250 },
      { "asset": "tree_low", "min_screen_pixels": 0 }
    ],
    "states": [{
      "name": "destroyed",
      "lods": [{ "asset": "stump", "min_screen_pixels": 0 }]
    }]
  }],
  "items": [{
    "name": "tree_1",
    "appearance": "tree",
    "position": [0, 0, 0],
    "visual_state": "default",
    "on_death_visual_state": "destroyed"
  }]
}
```

`base_asset` supplies the stable animation clips and default occupancy bounds.
Omitting an appearance's `lods` uses that base asset at all sizes. Additional
states must each supply at least one LOD. `default` is reserved for the initial
group; state names and appearance names must be unique within their scopes.

Thresholds must be finite, nonnegative, and strictly decreasing. The first
eligible entry wins. A final threshold of zero always draws some representation;
a positive final threshold omits items below that size. Omission does not remove
items or affect simulation/event scheduling.

## Camera Quality

The current camera is orthographic. Its target distance controls the spatial
query and visibility radius; it does not make an object appear smaller.
LOD uses the projected diameter of a conservative sphere enclosing the
appearance's render bounds after item scale:

```text
diameter = 2 * length(bounds.extent * item.scale) * camera.pixels_per_unit
```

The units are logical window pixels, consistent with the camera's viewport and
zoom. Retina framebuffer density does not change the thresholds. The sphere
is deliberately conservative and stable across animation, item yaw, and camera
translation. Animated envelopes may be substantially larger than a resting pose;
thresholds should be tuned per appearance. This is not a perspective-distance LOD.

Each camera's frame retains its own LOD history. With the default 10% hysteresis,
a 250-pixel boundary upgrades at 275 and downgrades below 225. A fresh view selects
at 250. State changes reset that item's history; items leaving the view lose their
history. A margin of zero disables hysteresis; accepted margins are [0, 0.5).
Single-variant groups ending at zero bypass history lookup.

World occupancy uses the base asset unless `occupancy_bounds` explicitly overrides
it in engine-local coordinates. Visibility uses the union of bounds
from every variant and visual state, including animation envelopes. Switching
representations therefore cannot shrink the spatial index entry, change placement,
or clip a larger variant. Bounds are still not a collision implementation.

## Animation Compatibility

The item's animation player indexes clips on `base_asset` for its entire lifetime.
Projection samples that source at the item's interpolated time, then maps the
palette into the chosen mesh's joint order. Variant clip ordering and optional
absence of variant clips do not change playback or event timing.

For a skinned variant, loading validates unique nonempty joint names, matching
named ancestry/rest transforms, and inverse bind matrices (absolute tolerance
0.0001). Joint ordering may differ. A variant may reference a subset of source
joints when those joints retain their original ancestry and bind space. Rigs with
merged bones or different bind poses require retargeting, which is not implemented.
Source animations also determine conservative bounds for each variant's vertices.

A static mesh can serve as an explicit proxy for an animated appearance. It emits
no bone palette, while the item's canonical animation and events keep advancing.
Assets must retain consistent origins, axes, and units; the engine does not
rescale or align alternative meshes automatically.

`App::set_visual_state(item_id, name)` resolves a name before changing world state
and invalidating cached frames. The C API exposes this as
`io_app_set_visual_state(app, item_id, "destroyed")`. It returns true only on a
change; invalid names/IDs leave state untouched. Gameplay systems in Rust can use
`World::set_visual_state` with a previously resolved state ID. An optional
`on_death_visual_state` switches on zero health, alongside any death animation.
Visual state alone does not change health, motion, or the animation player.

## Demo and Verification

```sh
make
./build/io --scene assets/street-kit/variants-demo.json
```

Zoom with the wheel or +/- to cross LOD thresholds. N creates a camera copy and
Tab switches cameras; each retains its own zoom and selection history. The demo
includes low meshes generated from the existing Blender sources:

| Asset | High Triangles | Low Triangles |
| --- | ---: | ---: |
| Cottage | 10,636 | 1,992 |
| Townhouse | 12,688 | 2,372 |
| Lamp | 1,928 | 376 |
| Tree | 952 | 186 |
| Runner | 6,372 | 1,274 |

The runner keeps its 14-joint rig. These are demonstration decimations, not
artist-tuned production LODs. Regenerate them with:

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --python tools/build_render_variants.py
```

The script writes `assets/street-kit/lods/` and `variants-demo.json`; it does not
save changes to source Blender files or replace the original high meshes.
The original scene and benchmark generator still use full-detail assets, keeping
that workload comparable. Loading all variants costs additional CPU memory;
their GPU meshes are uploaded lazily on first use and retained thereafter.

`make test` covers selection, omission, independent cameras, frame invalidation,
bounds, validation, death state, and canonical animation. `make gpu-test` also
switches the real skinned runner high/low/high, reads back instance and bone
buffers, and checks that previously loaded mesh buffers are reused.

Textures, billboards, material pipeline variants, cross-fades, automatic asset
streaming, and independent background simulation scheduling remain future work.
The current implementation supports the existing opaque GLB/builtin mesh path.
