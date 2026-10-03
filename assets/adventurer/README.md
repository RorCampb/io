# Drawing-Inspired Adventurer

Simplified interpretation of Rory's character drawing: pointed cap, loose tunic,
shorts, tall boots, belt pouch and a sword resting over one shoulder. Blue and red
are material variants of the same geometry, not different gameplay classes.

- `adventurer.blend`: editable blue/red asset scenes plus a lit Preview scene.
- `preview.png`: Blender studio render, not an engine screenshot.
- `blue.glb`, `red.glb`: opaque, texture-free meshes, 7,266 triangles each.
- `package.json`: existing version-1 engine asset contract.
- `encounter.json`: separate playable encounter; original box demo unchanged.
- `catalog.json`: authored Blender Z-up bounds and mesh counts.

These are **static posed prototypes**, without a skeleton or animation clips.
Gameplay can move them and apply death knockback, but limbs and sword do not yet
animate. The sword is decorative geometry, not an independent weapon collider.
The death collider approximates the torso/legs/cap, excluding arms and sword.

Authored height is approximately 2.14 meters, with origin between the feet. GLBs
use standard glTF Y-up coordinates, converted to engine Z-up by the importer.
Lights, plinths and preview placement are excluded from the exported assets.

From the repository root:

```sh
./build/release/io --scene assets/adventurer/encounter.json
cargo run --offline --bin io-validate -- package assets/adventurer/package.json
cargo run --offline --bin io-validate -- scene assets/adventurer/encounter.json
```

Rebuild generated assets and preview (overwrites this package's generated files):

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --python tools/build_adventurer.py
```

Keep hand-edited Blender work under a different filename before regenerating.
The builder reads the existing encounter rules and writes only this asset folder;
it does not read or modify the original drawing or other Blender projects.
