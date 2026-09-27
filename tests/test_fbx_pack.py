import copy
from bisect import bisect_right
import json
import math
from pathlib import Path
import struct
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT / "tools"))
from fbx_pack_contract import check_skeleton, clip_id, planar_drift


class FbxContractTests(unittest.TestCase):
    def test_names_and_planar_motion_policy(self):
        self.assertEqual(clip_id("cover to stand (2)"),"cover_to_stand_2")
        with self.assertRaises(ValueError):
            clip_id("!!!")
        self.assertEqual(planar_drift((0,0,1),(2,4,8),.5),(1,2,0))
        with self.assertRaises(ValueError):
            planar_drift((0,0,0),(1,1,1),2)

    def test_skeleton_contract_rejects_proportion_and_parent_changes(self):
        matrix=[1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.]
        base={"root":{"parent":None,"matrix":matrix},"child":{"parent":"root","matrix":matrix.copy()}}
        base["child"]["matrix"][7]=1.
        shifted=copy.deepcopy(base)
        for bone in shifted.values():
            bone["matrix"][11]+=2.
        self.assertEqual(check_skeleton(base,shifted),[0.,0.,-2.])
        shifted["child"]["matrix"][7]+=.1
        with self.assertRaises(ValueError):
            check_skeleton(base,shifted)
        invalid=copy.deepcopy(base)
        invalid["child"]["parent"]=None
        with self.assertRaises(ValueError):
            check_skeleton(base,invalid)
        invalid=copy.deepcopy(base)
        invalid["child"]["matrix"][0]=float("nan")
        with self.assertRaises(ValueError):
            check_skeleton(base,invalid)


class ExportedMixamoTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory=ROOT / "assets/action-adventure"
        data=(cls.directory / "x_bot.glb").read_bytes()
        magic,version,total=struct.unpack_from("<III",data)
        assert magic==0x46546C67 and version==2 and total==len(data)
        length,kind=struct.unpack_from("<II",data,12)
        assert kind==0x4E4F534A
        cls.gltf=json.loads(data[20:20+length])
        cls.binary=data[28+length:]

    def values(self,accessor):
        a=self.gltf["accessors"][accessor]
        self.assertEqual(a["componentType"],5126)
        size={"SCALAR":1,"VEC3":3,"VEC4":4}[a["type"]]
        view=self.gltf["bufferViews"][a["bufferView"]]
        offset=view.get("byteOffset",0)+a.get("byteOffset",0)
        stride=view.get("byteStride",size*4)
        return [struct.unpack_from("<"+"f"*size,self.binary,offset+i*stride) for i in range(a["count"])]

    def test_all_clips_export_and_have_real_motion(self):
        package=json.loads((self.directory / "package.json").read_text())
        catalog=json.loads((self.directory / "catalog.json").read_text())
        names={a["name"] for a in self.gltf["animations"]}
        self.assertEqual(len(names),24)
        self.assertEqual(names,set(package["assets"][0]["clips"]))
        self.assertEqual(names,{c["clip"] for c in catalog["clips"]})
        self.assertEqual(len(self.gltf["skins"][0]["joints"]),65)
        for animation in self.gltf["animations"]:
            for sampler in animation["samplers"]:
                self.assertIn(sampler.get("interpolation","LINEAR"),("LINEAR","STEP"))
            self.assertTrue(any(len(set(self.values(s["output"])))>1 for s in animation["samplers"]))

    def test_in_place_loops_remove_travel_without_replacing_other_bones(self):
        clips={a["name"]:a for a in self.gltf["animations"]}
        hip=next(i for i,n in enumerate(self.gltf["nodes"]) if n.get("name")=="mixamorig:Hips")
        def channels(name):
            animation=clips[name]
            duration=max(self.values(s["input"])[-1][0] for s in animation["samplers"])
            result={}
            for channel in animation["channels"]:
                sampler=animation["samplers"][channel["sampler"]]
                times=[v[0] for v in self.values(sampler["input"])]
                values=self.values(sampler["output"])
                samples=[]
                # Compare sampled curves, not key counts: the exporter may
                # reduce nearly constant channels differently after adaptation.
                for frame in range(round(duration*30)+1):
                    time=frame/30
                    i=max(0,min(len(times)-2,bisect_right(times,time)-1))
                    if len(times)==1:
                        value=values[0]
                    else:
                        t=max(0,min(1,(time-times[i])/(times[i+1]-times[i])))
                        value=tuple(a*(1-t)+b*t for a,b in zip(values[i],values[i+1]))
                    if channel["target"]["path"]=="rotation":
                        length=math.sqrt(sum(v*v for v in value))
                        value=tuple(v/length for v in value)
                    samples.append(value)
                result[(channel["target"]["node"],channel["target"]["path"])]=samples
            return result
        for name in ["walking","running"]:
            raw,adapted=channels(name),channels(name+"_in_place")
            def travel(values):
                return math.dist(values[0],values[-1])
            self.assertGreater(travel(raw[(hip,"translation")]),1.)
            self.assertLess(travel(adapted[(hip,"translation")]),.01)
            self.assertEqual(raw.keys(),adapted.keys())
            for target in raw:
                if target!=(hip,"translation"):
                    self.assertEqual(len(raw[target]),len(adapted[target]))
                    # Exporting through evaluated matrices introduces float32
                    # decomposition noise, especially under translated parents.
                    for a,b in zip(raw[target],adapted[target]):
                        for x,y in zip(a,b):
                            self.assertAlmostEqual(x,y,delta=0.0005)


if __name__ == "__main__":
    unittest.main()
