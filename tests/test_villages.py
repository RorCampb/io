import base64
import json
import struct
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]

class VillageContentTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.scene=json.loads((ROOT / "assets/villages/world.json").read_text())

    def test_square_kilometer_and_dialogue_contract(self):
        terrain=self.scene["terrain"]
        self.assertEqual((terrain["width"]-1)*terrain["spacing"],1000)
        self.assertEqual((terrain["depth"]-1)*terrain["spacing"],1000)
        self.assertEqual(len(terrain["heights"]),terrain["width"]*terrain["depth"])
        lines=json.loads((ROOT / "assets/villages/dialogue.json").read_text())
        self.assertEqual(lines,self.scene["exploration"]["dialogues"])
        self.assertEqual({k:len(v) for k,v in lines.items()},{"men":40,"women":40,"children":40,"parents":40,"shop":20,"civic":20})
        all_lines=[line for group in lines.values() for line in group]
        self.assertEqual(len(set(all_lines)),200)
        coverage=self.scene["camera"]["coverage"]
        self.assertEqual(coverage["type"],"viewport")
        self.assertLessEqual(coverage["min_z"],min(terrain["heights"]))
        self.assertGreaterEqual(coverage["max_z"],max(terrain["heights"])+10)

    def test_households_staff_and_companions_are_bound_items(self):
        items={i["name"]:i for i in self.scene["items"]}
        exploration=self.scene["exploration"]
        self.assertEqual(len(exploration["settlements"]),3)
        self.assertEqual(len(exploration["npcs"]),60)
        for village in range(3):
            npcs=[n for n in exploration["npcs"] if n["settlement"]==village]
            self.assertEqual(sum(n["role"]=="companion" for n in npcs),1)
            self.assertEqual(sum(n["role"]=="raider" for n in npcs),3)
            self.assertTrue({"shopkeeper","innkeeper","clerk"}.issubset({n["job"] for n in npcs}))
            for house in range(4):
                self.assertEqual(sum(n["home"]==f"v{village}-house{house}" for n in npcs),4)
        for npc in exploration["npcs"]:
            self.assertIn(npc["home"],items)
            self.assertIn("grounded",items[npc["item"]])
            self.assertNotIn("motion",items[npc["item"]])

    def test_terrain_exports_smooth_unit_normals(self):
        mesh=json.loads((ROOT / "assets/villages/terrain-0-0.gltf").read_text())
        attributes=mesh["meshes"][0]["primitives"][0]["attributes"]
        accessor=mesh["accessors"][attributes["NORMAL"]]
        self.assertEqual(accessor["count"],mesh["accessors"][attributes["POSITION"]]["count"])
        view=mesh["bufferViews"][accessor["bufferView"]]
        data=base64.b64decode(mesh["buffers"][0]["uri"].split(",",1)[1])
        for i in range(accessor["count"]):
            normal=struct.unpack_from("<3f",data,view["byteOffset"]+i*12)
            self.assertAlmostEqual(sum(n*n for n in normal),1.,places=5)

if __name__=="__main__":
    unittest.main()
