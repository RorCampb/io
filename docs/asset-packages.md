# Asset Packages

An asset package is exported geometry plus a versioned JSON manifest. Blender,
another DCC tool, or a procedural exporter can produce the same contract. The
engine does not execute exporter scripts or contain per-asset import branches.
The authoritative parser and delivery types are in
`crates/io-assets/src/contract.rs`; actual mesh and rig validation uses the same
load path as the native application, not a separate approximation.

## Minimal Package

Place `package.json` beside `chair.glb`:

```json
{
  "format": "io.asset-package",
  "version": 1,
  "name": "furniture",
  "coordinates": {
    "units": "meters",
    "up_axis": "+Y",
    "handedness": "right",
    "origin": "authored"
  },
  "requires": ["opaque_materials"],
  "assets": [
    {"name": "chair", "file": "chair.glb", "kind": "static", "clips": []}
  ],
  "appearances": [
    {"name": "chair", "base_asset": "chair"}
  ]
}
```

The coordinates describe the delivered glTF, not Blender's editing viewport.
Export right-handed Y-up glTF in meters, preserving the intended local origin.
The importer converts to the engine's Z-up coordinates without normalizing size.
Alternative meshes must agree on origin and scale; the importer does not align
them automatically. A package needs at least one asset and one appearance.

Names start with an ASCII letter or digit; remaining characters are letters,
digits, `_`, `-`, or `.`. Asset and appearance names have separate namespaces.
Unknown fields, format versions, and enum values fail loading.

## Import Into a Scene

A complete small scene using that package:

```json
{
  "version": 1,
  "dimensions": [100, 100, 100],
  "origin": [10, 10, 0],
  "camera": {"target": [0, 0, 0], "zoom": 1},
  "packages": [{"namespace": "furniture", "file": "../furniture/package.json"}],
  "items": [{"name": "seat", "appearance": "furniture/chair", "position": [0, 0, 0]}]
}
```

Import paths are relative to the scene. Mesh paths are relative to the package.
Namespaces are chosen by the scene, must be unique there, and qualify exported
names as `namespace/name`. Package-local appearance references remain unqualified
in the manifest. Different scenes can reuse the same package. Importing it twice
under different namespaces currently creates separate catalog entries, not
content-addressed deduplication.

Legacy inline `assets` and `appearances` still work and can coexist with imports.
Duplicate names in either resolved catalog fail loading. Each item uses exactly
one of `asset` or `appearance`; the latter enables explicit states and LODs.
Packages cannot import other packages or reference another package's meshes.

## Mesh and Appearance Rules

- An asset supplies exactly one `file` (`.glb` or `.gltf`) or `builtin: "box"`.
- Package assets must declare `kind: "static"` or `"skinned"`. This describes
  geometry, not whether a gameplay item moves. Imported joint presence must agree.
- `clips` is required and must exactly match unique, nonempty imported clip names.
  Static assets declare `[]`. Skinned assets may also have no clips, for example
  when borrowing a canonical rig's animation as an LOD.
- `requires` must contain `opaque_materials`; skinned assets additionally require
  `skeletal_animation`. These are the only supported feature names in version 1.
- File paths cannot be absolute, contain `..`, or resolve through symlinks outside
  the package directory. External glTF buffers/images must also stay within it.
  Dependency URIs must be unencoded local paths; data URIs are allowed. Required
  glTF extensions are rejected. These checks are not a hostile-content sandbox.
- Textures and nonopaque materials are unsupported. Supported material factors
  include base color and emission; geometry uses triangles. Unsupported imported
  features fail rather than being advertised as supported by the manifest.

Appearances use the same [render variant contract](render-variants.md) as inline
scenes: `base_asset`, decreasing `lods` thresholds, optional named `states`, and
`hysteresis`. Skeleton compatibility is checked against the canonical base mesh.
Each asset can therefore have different hand-authored LODs and thresholds without
an engine change.

An appearance can override default gameplay occupancy with
`"occupancy_bounds": {"min": [-1, -1, 0], "max": [1, 1, 2]}`. These bounds use
engine-local Z-up coordinates after import, before item placement/scale. They
must be finite and ordered. They do not replace the unioned render envelope and
are not collision shapes or destruction physics.

## Author and Validate

1. Export supported glTF meshes and named skeletal clips from the chosen tool.
2. Create the manifest, listing all exported meshes and reusable appearances.
3. Validate the package, then import it into any scene by namespace.
4. Validate the scene and inspect it in the native renderer for visual quality.

```sh
cargo run --bin io-validate -- package assets/street-kit/package.json
cargo run --bin io-validate -- scene assets/street-kit/package-demo.json
make release
./build/release/io --scene assets/street-kit/package-demo.json
```

The validator does not open SDL/OpenGL. Success prints a JSON count report to
stdout; diagnostics go to stderr. Exit codes are 0 for success, 1 for invalid
content, and 2 for invalid command usage. Package validation loads real geometry
and resolves appearances; scene validation additionally constructs the world
and resolves item, event-target, and clip references. `appearances` counts explicit
definitions, not compatibility appearances synthesized for legacy asset entries.

The optional migration helper extracts a package from an existing inline scene:

```sh
python3 tools/export_asset_package.py \
  --scene assets/street-kit/variants-demo.json \
  --output assets/street-kit/package.json --name street-kit \
  --scene-output assets/street-kit/package-demo.json
```

It writes metadata and rewrites references, without modifying meshes. All source
meshes must already be beneath the output manifest's directory. Always run the
validator afterward; the helper is not the authority on supported glTF content.
The checked-in package demo retains the original 39 items and animation state.

## Limits

This is a delivery contract, not a universal material/animation plugin API. New
models using supported capabilities require data changes only. New renderer
features or gameplay behaviors still require implementations and tests. Package
build recipes, asset-specific decimation policies, hot reload, and streaming are
not implemented. The existing Blender variant generator remains a demo tool;
artists can deliver manually authored variants through this contract instead.

Validation cannot detect unattractive silhouettes or damaged roofs from aggressive
decimation. The known street-kit low-roof and district road-join visual issues
remain asset/layout work, not fixes provided by package validation.
