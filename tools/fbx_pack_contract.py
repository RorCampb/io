"""Pure validation/math helpers for the offline FBX pack importer."""
import math
import re


def clip_id(filename):
    name = re.sub(r"[^a-z0-9]+", "_", filename.lower()).strip("_")
    if not name or len(name) > 60:
        raise ValueError(f"invalid clip identifier: {filename}")
    return name


def check_skeleton(base, other, meters_per_unit=1., tolerance=0.001):
    if not math.isfinite(meters_per_unit) or meters_per_unit <= 0:
        raise ValueError("invalid armature scale")
    if base.keys() != other.keys():
        raise ValueError("bone names differ; retargeting is required")
    for skeleton in (base,other):
        if any(len(b["matrix"]) != 16 or any(not math.isfinite(v) for v in b["matrix"]) for b in skeleton.values()):
            raise ValueError("invalid rest matrix")
    roots = [name for name,bone in base.items() if bone["parent"] is None]
    if len(roots) != 1:
        raise ValueError("expected a single skeleton root")
    # Common origin shifts and bone-axis changes are handled by pose baking.
    # Different proportions are not: corresponding joint positions must agree
    # to a millimeter after origin alignment, in actual scene units.
    offset = [base[roots[0]]["matrix"][i]-other[roots[0]]["matrix"][i] for i in (3,7,11)]
    for name, bone in base.items():
        candidate = other[name]
        if bone["parent"] != candidate["parent"]:
            raise ValueError(f"{name}: parent differs; retargeting is required")
        adjusted = list(candidate["matrix"])
        for i,delta in zip((3,7,11),offset):
            adjusted[i] += delta
        if len(adjusted) != 16 or any(not math.isfinite(v) for v in adjusted + bone["matrix"]):
            raise ValueError(f"{name}: invalid rest matrix")
        if any(abs(bone["matrix"][i]-adjusted[i])*meters_per_unit > tolerance for i in (3,7,11)):
            raise ValueError(f"{name}: rest pose differs; retargeting is required")
    return offset


def planar_drift(first, last, fraction):
    """Remove net XY travel, preserving vertical movement and intra-cycle sway."""
    if not 0 <= fraction <= 1:
        raise ValueError("sample fraction outside clip")
    return ((last[0]-first[0])*fraction, (last[1]-first[1])*fraction, 0.)
