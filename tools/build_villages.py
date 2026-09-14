"""Build a deterministic one-square-kilometer exploration demo. No engine source edits.

python3 tools/build_villages.py
Generated assets stay in assets/villages. Authored dialogue.json is input, never overwritten.
"""
import copy
import json
import math
from pathlib import Path
import random
from build_district import Mesh

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "assets/villages"
SPACING = 5
SIZE = 201
SETTLEMENTS = [("Brookmere", -280, -180), ("Clover Hollow", 270, -80), ("Highmeadow", 0, 280)]


def raw_height(x, y):
    return 24 + 20 * math.sin(x / 155) * math.cos(y / 190) + 12 * math.sin((x + y) / 120) + 10 * math.cos(x / 110 - y / 150)


def terrain_height(x, y):
    h = raw_height(x, y)
    for _, cx, cy in SETTLEMENTS:
        d = math.hypot(x - cx, y - cy)
        if d < 200:
            t = max(0, min(1, (d - 120) / 80))
            t = t * t * (3 - 2 * t)
            h = raw_height(cx, cy) * (1 - t) + h * t
    return h


def sampled_height(heights, x, y):
    u, v = (x + 500) / SPACING, (y + 500) / SPACING
    i, j = min(int(u), SIZE - 2), min(int(v), SIZE - 2)
    u, v = u - i, v - j
    a, b, c, d = (heights[j*SIZE+i], heights[j*SIZE+i+1],
                   heights[(j+1)*SIZE+i+1], heights[(j+1)*SIZE+i])
    return a + u*(b-a) + v*(c-b) if u >= v else a + v*(d-a) + u*(c-d)


def house(width, depth, height, civic=False):
    m = Mesh()
    m.box((0, 0, height/2), (width, depth, height), (.67,.62,.39) if not civic else (.65,.70,.58))
    w, d, z = width/2+.3, depth/2+.3, height
    p = [(-w,-d,z),(w,-d,z),(0,-d,z+2),(-w,d,z),(w,d,z),(0,d,z+2)]
    for a,b,c in [(0,1,2),(3,5,4),(0,2,5),(0,5,3),(1,4,5),(1,5,2)]:
        m.triangle(p[a],p[b],p[c],(.30,.13,.09) if not civic else (.19,.28,.29))
    m.box((0,-depth/2-.04,1.1),(1,.12,2.2),(.15,.09,.04))
    for x in (-width*.3,width*.3):
        m.box((x,-depth/2-.08,height*.63),(1.0,.15,1.1),(.18,.39,.44))
        m.box((x,-depth/2-.17,height*.63),(.08,.04,1.1),(.80,.73,.48))
    m.box((width*.25,depth*.2,height+1),(0.6,.6,2.2),(.38,.25,.16))
    return m


def props():
    house(7,6,4).save(OUT / "house.gltf")
    house(10,7,4).save(OUT / "shop.gltf")
    house(12,8,5,True).save(OUT / "hall.gltf")
    m=Mesh()
    m.cone(.26,.70,1.4,(.60,.64,.55),10,.31)
    m.cone(.16,1.46,1.76,(.67,.43,.26),10,.16)
    m.cone(.17,1.73,1.85,(.12,.07,.04),10,.07)
    for side in (-1,1):
        m.box((side*.13,0,.36),(.18,.22,.7),(.13,.11,.075))
        m.box((side*.36,0,1.12),(.12,.17,.57),(.67,.43,.26))
        m.box((side*.13,-.07,.09),(.20,.36,.18),(.09,.07,.045))
        m.box((side*.06,-.162,1.65),(.035,.018,.035),(.035,.025,.02))
    m.save(OUT / "villager.gltf")
    tree=Mesh()
    tree.cone(.30,0,3,(.25,.13,.07),8,.20)
    tree.cone(2.4,2,7.5,(.12,.32,.07),10,.15)
    tree.cone(1.9,4,9,(.19,.42,.09),10)
    tree.save(OUT / "tree.gltf")


def generate():
    OUT.mkdir(parents=True,exist_ok=True)
    dialogue=json.loads((OUT / "dialogue.json").read_text())
    assert sum(map(len,dialogue.values())) == 200
    props()
    heights=[round(terrain_height(-500+i*SPACING,-500+j*SPACING),5) for j in range(SIZE) for i in range(SIZE)]
    height=lambda x,y: sampled_height(heights,x,y)
    coverage={"type":"viewport","min_z":math.floor(min(heights))-2,"max_z":math.ceil(max(heights))+16}
    def normal(x,y):
        xm,xp=max(-500,x-.1),min(500,x+.1)
        ym,yp=max(-500,y-.1),min(500,y+.1)
        nx,ny=-(height(xp,y)-height(xm,y))/(xp-xm),-(height(x,yp)-height(x,ym))/(yp-ym)
        length=math.sqrt(nx*nx+ny*ny+1)
        return nx/length,1/length,-ny/length
    scene={"version":1,"dimensions":[1000,1000,100],"origin":[0,0,0],"simulation":{"tick_hz":60},
           "camera":{"target":[-286,-183,height(-286,-183)+1],"zoom":.65,"render_distance":180,"follow":"hero","coverage":coverage},
           "terrain":{"origin":[-500,-500],"spacing":SPACING,"width":SIZE,"depth":SIZE,"heights":heights},
           "assets":[{"name":"box","builtin":"box"}]+[{"name":n,"file":n+".gltf"} for n in ("house","shop","hall","villager","tree")],
           "packages":[{"namespace":"adventurer","file":"../adventurer/package.json"}],"items":[]}
    def item(name,asset,x,y,z=None,scale=None,**extra):
        obj={"name":name,"asset":asset,"position":[x,y,height(x,y) if z is None else z]}
        if scale:obj["scale"]=scale
        obj.update(extra);scene["items"].append(obj);return obj
    # 50m tiles allow the existing spatial/frustum queries to reject distant terrain.
    for row in range(20):
        for col in range(20):
            m=Mesh();cx,cy=-500+col*50,-500+row*50
            for j in range(10):
                for i in range(10):
                    x,y=cx+i*5,cy+j*5
                    pts=[(x-cx,y-cy,height(x,y)),(x+5-cx,y-cy,height(x+5,y)),(x+5-cx,y+5-cy,height(x+5,y+5)),(x-cx,y+5-cy,height(x,y+5))]
                    shade=.015*math.sin(x/80)*math.cos(y/65)+height(x,y)*.0008
                    color=(.20+shade,.43+shade,.095+shade/2)
                    m.triangle(pts[0],pts[1],pts[2],color);m.triangle(pts[0],pts[2],pts[3],color)
            name=f"terrain-{row}-{col}";m.save(OUT / (name+".gltf"),[normal(p[0]+cx,-p[2]+cy) for p in m.positions]);scene["assets"].append({"name":name,"file":name+".gltf"});item(name,name,cx,cy,0)
    paths=[]
    def road(points):
        m=Mesh()
        edges=[]
        for i,point in enumerate(points):
            a,b=points[max(0,i-1)],points[min(len(points)-1,i+1)]
            dx,dy=b[0]-a[0],b[1]-a[1];length=math.hypot(dx,dy)
            nx,ny=-dy/length*2.6,dx/length*2.6
            edges.append([(point[0]+s*nx,point[1]+s*ny,height(point[0]+s*nx,point[1]+s*ny)+.15) for s in (1,-1)])
        for a,b in zip(edges,edges[1:]):
            p=[a[0],a[1],b[1],b[0]]
            m.triangle(p[0],p[1],p[2],(.50,.69,.27));m.triangle(p[0],p[2],p[3],(.50,.69,.27))
        name=f"path-{len(paths)}";paths.append(name);m.save(OUT / (name+".gltf"));scene["assets"].append({"name":name,"file":name+".gltf"});item(name,name,0,0,0)
    for a,b in zip(SETTLEMENTS,SETTLEMENTS[1:]+SETTLEMENTS[:1]):
        direction=1 if b[1]>a[1] else -1
        a=(a[0],a[1]+direction*75,a[2]);b=(b[0],b[1]-direction*75,b[2])
        dx,dy=b[1]-a[1],b[2]-a[2];length=math.hypot(dx,dy)
        points=[]
        for i in range(301):
            t=i/300;wave=35*math.sin(t*math.tau)*math.sin(math.pi*t)
            points.append((a[1]+dx*t-dy/length*wave,a[2]+dy*t+dx/length*wave))
        # Split ribbons too; one enormous road bound would stay visible everywhere.
        for i in range(0,300,25):road(points[i:i+26])
    rng=random.Random(734)
    for i in range(240):
        x,y=rng.uniform(-480,480),rng.uniform(-480,480)
        if any(max(abs(x-cx),abs(y-cy))<125 for _,cx,cy in SETTLEMENTS):continue
        scale=rng.uniform(.65,1.2);item(f"tree-{i}","tree",x,y,scale=[scale]*3)
    grounded={"radius":.34,"height":2.1,"max_slope":1.}
    hero=item("hero","box",-286,-183,health=120,grounded=grounded)
    hero.pop("asset");hero["appearance"]="adventurer/blue"
    combat={"version":1,"movement_seconds":3,"abilities":{"sword":{"range":3,"effect":{"type":"damage","amount":20}},"spark":{"range":16,"effect":{"type":"damage","amount":15},"delivery":{"type":"projectile","speed":12}},"claw":{"range":2.5,"effect":{"type":"damage","amount":4}}},"templates":{},"combatants":[{"item":"hero","template":"hero"}]}
    death={"mass":2,"knockback_impulse":3,"lift_impulse":2,"half_extents":[.34,.25,1.07],"offset":[0,0,1.07]}
    combat["confirm_round_start"] = True
    for name,control,faction,speed,initiative,abilities in [("hero","player",1,5,30,["sword","spark"]),("companion","npc",1,5,20,["sword","spark"]),("raider","npc",2,2.5,10,["claw"])]:
        combat["templates"][name]={"control":control,"faction":faction,"initiative":initiative,"movement_speed":speed,"abilities":abilities,"opportunity_ability":abilities[0],"death_physics":death}
    npcs=[]
    names=[("Alden","Mira","Pip","Tess"),("Bram","Elin","Kit","Wren"),("Oren","Lena","Ash","Nell"),("Hugh","Sana","Finn","June")]
    for village,(_,cx,cy) in enumerate(SETTLEMENTS):
        h=height(cx,cy)
        item(f"v{village}-floor","box",cx-80,cy-80,h-.53,[160,160,.5],tint=[.20,.43,.095],physics_body={"type":"static"},collider={"shape":{"type":"box","half_extents":[80,80,.25]},"offset":[80,80,.25]})
        road([(cx-75+i,cy) for i in range(151)])
        for k in range(5):
            hx=cx-34+k*17
            item(f"v{village}-house{k}","house",hx,cy+24,physics_body={"type":"static"},collider={"shape":{"type":"box","half_extents":[3.5,3,2]},"offset":[0,0,2]})
            road([(hx,cy+i) for i in range(0,22)])
        for name,asset,x in [("store","shop",cx-27),("inn","shop",cx+27),("hall","hall",cx)]:
            w,d,z=(6,4,2.5) if name=="hall" else (5,3.5,2)
            item(f"v{village}-{name}",asset,x,cy-26,yaw_degrees=180,physics_body={"type":"static"},collider={"shape":{"type":"box","half_extents":[w,d,z]},"offset":[0,0,z]})
            road([(x,cy-i) for i in range(23)])
        for family in range(4):
            hx=cx-34+family*17
            for member in range(4):
                idx=family*4+member;name=f"v{village}-n{idx}";child=member>=2
                x,y=hx+(member%2)*2-1,cy+16-(member//2)*2
                scale=.65 if child else 1
                tint=[(.85,.95,.73),(.80,.85,1.),(1.,.77,.61),(.95,.91,.60)][(family+member+village)%4]
                item(name,"villager",x,y,scale=[scale]*3,tint=tint,health=40,grounded={"radius":.26 if child else .33,"height":1.25 if child else 1.9,"max_slope":1.})
                groups=["children"] if child else ["men" if member==0 else "women","parents"]
                job="child" if child else "resident"
                if (family,member) in [(0,0),(1,1)]:groups.append("shop");job="shopkeeper" if family==0 else "innkeeper"
                if (family,member)==(2,0):groups.append("civic");job="clerk"
                # Walkable outdoor household/plaza routes, never through occupied buildings.
                route=[[x,y],[x,cy+8],[cx+(idx%4-2)*3,cy+8],[x,cy+12]]
                if job in ("shopkeeper","innkeeper","clerk"):
                    wx=cx+{"shopkeeper":-27,"innkeeper":27,"clerk":0}[job]
                    route=[[wx,cy-18],[wx,cy+8],[x,cy+8],[x,y],[x,cy+8],[wx,cy+8]]
                    scene["items"][-1]["position"]=[wx,cy-18,height(wx,cy-18)]
                npcs.append({"item":name,"name":names[family][member],"settlement":village,"role":"resident","family":["Holm","Reed","Vale","Moss"][family],"home":f"v{village}-house{family}","job":job,"groups":groups,"route":route,"speed":.6+idx%3*.15})
        companion=f"v{village}-companion"
        obj=item(companion,"box",cx-2,cy-3,health=80,grounded=grounded,tint=[1.,.88,.5]);obj.pop("asset");obj["appearance"]="adventurer/blue"
        npcs.append({"item":companion,"name":["Cora","Rowan","Ivo"][village],"settlement":village,"role":"companion","family":["Bell","Thorn","Hill"][village],"home":f"v{village}-house4","job":"companion","groups":["women" if village==0 else "men"],"route":[[cx-2,cy-3],[cx+2,cy-3],[cx+2,cy-6],[cx-2,cy-6]],"speed":.4})
        combat["combatants"].append({"item":companion,"template":"companion"})
        item(f"v{village}-camp","box",cx+66,cy+9,scale=[2,2,1],tint=[.37,.20,.09])
        for k in range(3):
            name=f"v{village}-raider{k}";x,y=cx+65+k*3,cy-5+k*4
            obj=item(name,"box",x,y,health=30,grounded=grounded);obj.pop("asset");obj["appearance"]="adventurer/red"
            npcs.append({"item":name,"name":f"Raider {k+1}","settlement":village,"role":"raider","family":"Road camp","home":f"v{village}-camp","job":"raider","groups":["men"],"route":[[x,y],[x+2,y]],"speed":1.})
            combat["combatants"].append({"item":name,"template":"raider"})
    scene["exploration"]={"version":1,"player":"hero","settlements":[{"name":n,"center":[x,y]} for n,x,y in SETTLEMENTS],"npcs":npcs,"dialogues":dialogue,"combat":combat}
    (OUT / "world.json").write_text(json.dumps(scene,separators=(",",":"))+"\n")
    overview=copy.deepcopy(scene);overview["camera"]={"target":[0,0,15],"zoom":.055,"render_distance":180,"coverage":coverage}
    (OUT / "overview.json").write_text(json.dumps(overview,separators=(",",":"))+"\n")
    print(f"Built 1000x1000m, {len(npcs)} NPCs, 3 companions, {len(scene['items'])} Items, 200 dialogue lines")


if __name__=="__main__":
    generate()
