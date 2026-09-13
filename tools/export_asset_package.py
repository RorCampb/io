"""Extract a reusable package from an inline scene catalog, without changing meshes.

Run io-validate afterward to check actual geometry, clips, and rig compatibility.
Build recipes/exporters may instead write the same contract directly.
"""
import argparse
import copy
import json
import os
from pathlib import Path
import struct


def gltf_document(path):
    data = path.read_bytes()
    if path.suffix == ".gltf":
        return json.loads(data)
    if len(data) < 20 or data[:4] != b"glTF":
        raise ValueError(f"invalid GLB: {path}")
    length = struct.unpack_from("<I", data, 12)[0]
    return json.loads(data[20:20+length])


def export(source, output, name, scene_output=None):
    source, output = source.resolve(), output.resolve()
    scene = json.loads(source.read_text())
    if scene.get("packages"):
        raise ValueError("provide an inline catalog; imported packages are already reusable")
    if output == source or (scene_output and scene_output.resolve() in (source, output)):
        raise ValueError("source, package output, and scene output must be distinct files")
    assets = copy.deepcopy(scene.get("assets", []))
    skinned = False
    for asset in assets:
        if "file" in asset:
            path = (source.parent / asset["file"]).resolve()
            if not path.is_relative_to(output.parent):
                raise ValueError(f"{path} must be inside the package directory {output.parent}")
            asset["file"] = path.relative_to(output.parent).as_posix()
            document = gltf_document(path)
            asset["kind"] = "skinned" if document.get("skins") else "static"
            asset["clips"] = [clip.get("name", "unnamed") for clip in document.get("animations", [])]
        else:
            asset["kind"], asset["clips"] = "static", []
        skinned |= asset["kind"] == "skinned"
    appearances = scene.get("appearances") or [{"name": a["name"], "base_asset": a["name"]} for a in assets]
    package = {"format": "io.asset-package", "version": 1, "name": name,
               "coordinates": {"units": "meters", "up_axis": "+Y", "handedness": "right", "origin": "authored"},
               "requires": ["opaque_materials"] + (["skeletal_animation"] if skinned else []),
               "assets": assets, "appearances": appearances}
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(package, indent=2) + "\n")
    if scene_output:
        scene_output = scene_output.resolve()
        scene.pop("assets", None)
        scene.pop("appearances", None)
        scene["packages"] = [{"namespace": name, "file": os.path.relpath(output, scene_output.parent)}]
        for item in scene["items"]:
            for field in ("asset", "appearance"):
                if field in item:
                    item[field] = name + "/" + item[field]
        scene_output.parent.mkdir(parents=True, exist_ok=True)
        scene_output.write_text(json.dumps(scene, indent=2) + "\n")
    print(f"Wrote {output}; validate with cargo run --bin io-validate -- package {output}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scene", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--name", required=True)
    parser.add_argument("--scene-output", type=Path)
    args = parser.parse_args()
    try:
        export(args.scene, args.output, args.name, args.scene_output)
    except (ValueError, OSError) as error:
        parser.error(str(error))


if __name__ == "__main__":
    main()
