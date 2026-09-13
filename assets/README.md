# Imported Models

The included `demo.glb` is a whole Blender scene. The demo selects the objects
named exactly `donut` and `icing` as model ID `2`, excluding the surrounding
planes, cube, and other donuts. Change that selection in `src/model.rs` when
using a different export; an empty selection loads the default scene.

The importer applies parent/object transforms and converts glTF's Y-up axis to
the world's Z-up axis before normalizing the combined geometry to fit a unit
box with its proportions preserved. Uniform item sizes preserve those proportions.
The demo uses size `(10, 10, 10)` near the starting camera's target.

The asset path is resolved from the project directory recorded at build time.
Failed imports report an error instead of silently displaying a cube. Positions,
triangle indices, and Blender's vertex normals are loaded. Normals use the inverse
transpose of object transforms and are interpolated for smooth lighting, preserving
authored hard edges. Primitives without normals use geometric face lighting.
Imported materials, textures, and animations are not yet supported.
