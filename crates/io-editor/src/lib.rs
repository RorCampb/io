#![forbid(unsafe_code)]
pub use io_scene::InteriorDefinition as Region;
use io_scene::{CameraRig, CameraRigs};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Select,
    Set,
    Commit,
    Undo,
    Redo,
    Save,
    Preview,
    Frame,
    Play,
    Capture,
    Duplicate,
    Trajectory,
    AddPortal,
}
impl TryFrom<u32> for Command {
    type Error = String;
    fn try_from(value: u32) -> Result<Self, String> {
        match value {
            1 => Ok(Self::Select),
            2 => Ok(Self::Set),
            3 => Ok(Self::Commit),
            4 => Ok(Self::Undo),
            5 => Ok(Self::Redo),
            6 => Ok(Self::Save),
            7 => Ok(Self::Preview),
            8 => Ok(Self::Frame),
            9 => Ok(Self::Play),
            10 => Ok(Self::Capture),
            11 => Ok(Self::Duplicate),
            12 => Ok(Self::Trajectory),
            13 => Ok(Self::AddPortal),
            _ => Err("Unknown editor command".into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub rigs: CameraRigs,
    pub regions: Vec<Region>,
    pub portals: Vec<io_scene::PortalDefinition>,
}
impl Draft {
    pub fn portal_index(&self, index: usize) -> Option<usize> {
        index
            .checked_sub(self.regions.len() + 1)
            .filter(|i| *i < self.portals.len())
    }
    pub fn selection_count(&self) -> usize {
        1 + self.regions.len() + self.portals.len()
    }
    pub fn validate(&self) -> Result<(), String> {
        io_scene::resolve_layout(&self.regions, &self.portals, [0.; 3])?;
        let mut names = Vec::new();
        for r in &self.regions {
            r.validate()?;
            if names.contains(&r.name) {
                return Err("Duplicate interior name".into());
            }
            names.push(r.name.clone());
        }
        if names.len() > 4096 {
            return Err("Too many interiors".into());
        }
        self.rigs.validate(&names)
    }
    pub fn rig(&self, index: usize) -> Option<&CameraRig> {
        if index == 0 {
            Some(&self.rigs.exterior)
        } else {
            self.regions
                .get(index - 1)
                .and_then(|r| self.rigs.interiors.get(&r.name))
        }
    }
    fn rig_mut(&mut self, index: usize) -> Option<&mut CameraRig> {
        if index == 0 {
            Some(&mut self.rigs.exterior)
        } else {
            let name = &self.regions.get(index - 1)?.name;
            self.rigs.interiors.get_mut(name)
        }
    }
}
pub struct Document {
    draft: Draft,
    saved: Draft,
    needs_save: bool,
    raw: serde_json::Value,
    bytes: Vec<u8>,
    path: PathBuf,
    undo: Vec<Draft>,
    redo: Vec<Draft>,
    pending: Option<Draft>,
}
impl Document {
    pub fn draft(&self) -> &Draft {
        &self.draft
    }
    pub fn open(path: &Path) -> Result<Self, String> {
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        let bytes = fs::read(&path).map_err(|e| e.to_string())?;
        let raw: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if !raw.get("camera").is_some_and(|v| v.is_object()) {
            return Err("Scene requires a camera object".into());
        }
        let regions: Vec<Region> = serde_json::from_value(
            raw.get("interiors")
                .cloned()
                .unwrap_or(serde_json::json!([])),
        )
        .map_err(|e| e.to_string())?;
        let mut rigs: CameraRigs = match raw["camera"].get("rigs").filter(|v| !v.is_null()) {
            Some(value) => serde_json::from_value(value.clone()).map_err(|e| e.to_string())?,
            None => CameraRigs::default(),
        };
        let needs_save = raw["camera"].get("rigs").is_none_or(|v| v.is_null())
            || regions
                .iter()
                .any(|r| !rigs.interiors.contains_key(&r.name));
        for region in &regions {
            rigs.interiors
                .entry(region.name.clone())
                .or_insert_with(|| CameraRig {
                    yaw_degrees: 0.,
                    pitch_degrees: 10.,
                    zoom: 10.,
                    target_height: 0.8,
                    ..CameraRig::default()
                });
        }
        let portals =
            serde_json::from_value(raw.get("portals").cloned().unwrap_or(serde_json::json!([])))
                .map_err(|e| e.to_string())?;
        let draft = Draft {
            rigs,
            regions,
            portals,
        };
        draft.validate()?;
        Ok(Self {
            saved: draft.clone(),
            needs_save,
            draft,
            raw,
            bytes,
            path,
            undo: Vec::new(),
            redo: Vec::new(),
            pending: None,
        })
    }
    pub fn dirty(&self) -> bool {
        self.needs_save || self.draft != self.saved
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn commit(&mut self) {
        if let Some(before) = self.pending.take() {
            if before != self.draft {
                if self.undo.len() == 64 {
                    self.undo.remove(0);
                }
                self.undo.push(before);
                self.redo.clear();
            }
        }
    }
    pub fn undo(&mut self) {
        self.commit();
        if let Some(d) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.draft, d));
        }
    }
    pub fn redo(&mut self) {
        self.commit();
        if let Some(d) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.draft, d));
        }
    }
    pub fn values(&self, index: usize) -> Option<[f32; 16]> {
        if let Some(i) = self.draft.portal_index(index) {
            let p = &self.draft.portals[i];
            let mut out = [0.; 16];
            out[..3].copy_from_slice(&p.center);
            out[3..6].copy_from_slice(&p.normal);
            out[6] = p.width;
            out[7] = p.height;
            let number = |s: &io_scene::SpaceReference| match s {
                io_scene::SpaceReference::Exterior => 0.,
                io_scene::SpaceReference::Interior(name) => self
                    .draft
                    .regions
                    .iter()
                    .position(|r| r.name == *name)
                    .map_or(0., |i| (i + 1) as f32),
            };
            out[8] = number(&p.from);
            out[9] = number(&p.to);
            return Some(out);
        }
        let r = self.draft.rig(index)?;
        let mut out = [
            r.yaw_degrees,
            r.pitch_degrees,
            r.zoom,
            r.target_height,
            r.yaw_limit_degrees,
            r.min_zoom,
            r.max_zoom,
            r.response_seconds,
            r.approach,
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
            r.fov_degrees,
        ];
        if index > 0 {
            let b = self.draft.regions.get(index - 1)?;
            for i in 0..3 {
                out[9 + i] = b.min[i];
                out[12 + i] = b.max[i] - b.min[i];
            }
        }
        Some(out)
    }
    pub fn set(&mut self, index: usize, field: usize, value: f32) -> Result<(), String> {
        if !value.is_finite() || field >= 16 {
            return Err("Invalid numeric value".into());
        }
        let mut next = self.draft.clone();
        if let Some(i) = next.portal_index(index) {
            let endpoint = |v: f32| -> Result<io_scene::SpaceReference, String> {
                if v.fract() != 0. || v < 0. {
                    return Err("Space index must be an integer; 0 means exterior".into());
                }
                if v == 0. {
                    Ok(io_scene::SpaceReference::Exterior)
                } else {
                    next.regions
                        .get(v as usize - 1)
                        .map(|r| io_scene::SpaceReference::Interior(r.name.clone()))
                        .ok_or("Unknown space index".into())
                }
            };
            let reference = if field == 8 || field == 9 {
                Some(endpoint(value)?)
            } else {
                None
            };
            let p = &mut next.portals[i];
            match field {
                0..=2 => p.center[field] = value,
                3..=5 => p.normal[field - 3] = value,
                6 => p.width = value,
                7 => p.height = value,
                8 => p.from = reference.unwrap(),
                9 => p.to = reference.unwrap(),
                _ => return Err("Unknown portal field".into()),
            }
        } else if field < 9 || field == 15 {
            let r = next.rig_mut(index).ok_or("Unknown rig")?;
            match field {
                0 => r.yaw_degrees = value,
                1 => r.pitch_degrees = value,
                2 => r.zoom = value,
                3 => r.target_height = value,
                4 => r.yaw_limit_degrees = value,
                5 => r.min_zoom = value,
                6 => r.max_zoom = value,
                7 => r.response_seconds = value,
                8 => r.approach = value,
                15 => r.fov_degrees = value,
                _ => unreachable!(),
            }
        } else {
            let b = next
                .regions
                .get_mut(index.checked_sub(1).ok_or("Exterior has no volume")?)
                .ok_or("Unknown volume")?;
            let axis = (field - 9) % 3;
            if field < 12 {
                let delta = value - b.min[axis];
                b.min[axis] = value;
                b.max[axis] += delta;
                if axis == 2 {
                    b.ceiling = b.ceiling.map(|v| v + delta);
                }
            } else {
                b.max[axis] = b.min[axis] + value;
                if axis == 2 && b.ceiling.is_some() {
                    b.ceiling = Some(b.max[2]);
                }
            }
        }
        next.validate()?;
        if self.pending.is_none() {
            self.pending = Some(self.draft.clone());
        }
        self.draft = next;
        Ok(())
    }
    pub fn duplicate(&mut self, index: usize) -> Result<usize, String> {
        let mut next = self.draft.clone();
        let mut region = next
            .regions
            .get(index.checked_sub(1).ok_or("Select an interior first")?)
            .ok_or("Unknown interior")?
            .clone();
        let rig = next.rig(index).ok_or("Unknown rig")?.clone();
        let mut n = 1;
        loop {
            region.name = format!("Interior {n}");
            if !next.regions.iter().any(|r| r.name == region.name) {
                break;
            }
            n += 1;
        }
        next.rigs.interiors.insert(region.name.clone(), rig);
        next.regions.push(region);
        next.validate()?;
        self.commit();
        self.pending = Some(self.draft.clone());
        self.draft = next;
        self.commit();
        Ok(self.draft.regions.len())
    }
    pub fn add_portal(&mut self, index: usize) -> Result<usize, String> {
        let region = self
            .draft
            .regions
            .get(index.checked_sub(1).ok_or("Select an interior")?)
            .ok_or("Select an interior")?;
        let mut n = 1;
        while self
            .draft
            .portals
            .iter()
            .any(|p| p.name == format!("Entrance {n}"))
        {
            n += 1;
        }
        let portal = region.entrance(&self.draft.regions, format!("Entrance {n}"))?;
        let mut next = self.draft.clone();
        next.portals.push(portal);
        next.validate()?;
        self.commit();
        self.pending = Some(self.draft.clone());
        self.draft = next;
        self.commit();
        Ok(self.draft.selection_count() - 1)
    }
    pub fn capture(&mut self, index: usize, yaw: f32, pitch: f32, zoom: f32) -> Result<(), String> {
        let mut next = self.draft.clone();
        let rig = next.rig_mut(index).ok_or("Unknown rig")?;
        rig.yaw_degrees = yaw;
        rig.pitch_degrees = pitch;
        if !(rig.min_zoom..=rig.max_zoom).contains(&zoom) {
            return Err("View zoom is outside rig limits; adjust MIN/MAX ZOOM first".into());
        }
        rig.zoom = zoom;
        next.validate()?;
        self.commit();
        self.pending = Some(self.draft.clone());
        self.draft = next;
        self.commit();
        Ok(())
    }
    pub fn save(&mut self) -> Result<(), String> {
        self.commit();
        self.draft.validate()?;
        if fs::read(&self.path).map_err(|e| e.to_string())? != self.bytes {
            return Err(
                "Scene changed on disk. Save refused; reopen to avoid overwriting external edits"
                    .into(),
            );
        }
        let mut raw = self.raw.clone();
        raw["camera"]["rigs"] =
            serde_json::to_value(&self.draft.rigs).map_err(|e| e.to_string())?;
        // Orbit is an explicit runtime override; rig previews remain authoring data.
        raw["camera"].as_object_mut().unwrap().remove("interior");
        raw["interiors"] = serde_json::to_value(&self.draft.regions).map_err(|e| e.to_string())?;
        raw["portals"] = serde_json::to_value(&self.draft.portals).map_err(|e| e.to_string())?;
        let mut bytes = serde_json::to_vec_pretty(&raw).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let temp = self
            .path
            .with_extension(format!("editor-{}-{stamp}.tmp", std::process::id()));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        let result = (|| -> std::io::Result<()> {
            file.set_permissions(fs::metadata(&self.path)?.permissions())?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temp, &self.path)
        })();
        if let Err(e) = result {
            let _ = fs::remove_file(&temp);
            return Err(e.to_string());
        }
        self.bytes = bytes;
        self.raw = raw;
        self.saved = self.draft.clone();
        self.needs_save = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("io-editor-{}-{id}.json", std::process::id()));
            let mut rigs = CameraRigs::default();
            rigs.interiors.insert("Room".into(), CameraRig::default());
            let scene = serde_json::json!({"camera":{"rigs":rigs,"follow":"hero","zoom":3},
                "interiors":[{"name":"Room","min":[0,0,0],"max":[4,6,3],"ceiling":3,"entry_direction":[0,-1,0]}],
                "items":[{"name":"hero","asset":"package/character"}],"packages":[{"file":"../kit/package.json"}]});
            fs::write(&path, serde_json::to_vec(&scene).unwrap()).unwrap();
            Self(path)
        }
        fn open(&self) -> Document {
            Document::open(&self.0).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    #[test]
    fn invalid_edits_are_atomic_and_gestures_undo_as_one() {
        let fixture = Fixture::new();
        let mut d = fixture.open();
        let before = d.draft.clone();
        assert!(!d.dirty());
        for (index, field, value) in [
            (0, 1, 90.),
            (0, 2, f32::NAN),
            (0, 9, 2.),
            (1, 14, -1.),
            (30, 0, 1.),
            (1, 99, 0.),
            (1, 15, 120.),
        ] {
            assert!(d.set(index, field, value).is_err());
            assert_eq!(d.draft, before);
        }
        d.set(1, 0, 20.).unwrap();
        d.set(1, 0, 25.).unwrap();
        d.commit();
        assert!(d.dirty());
        d.undo();
        assert_eq!(d.draft, before);
        assert!(!d.dirty());
        d.redo();
        assert_eq!(d.values(1).unwrap()[0], 25.);
        d.undo();
        d.set(1, 0, 30.).unwrap();
        d.commit();
        d.redo();
        assert_eq!(d.values(1).unwrap()[0], 30.);
    }
    #[test]
    fn bounds_capture_and_duplicate_preserve_contracts() {
        let fixture = Fixture::new();
        let mut d = fixture.open();
        d.set(1, 11, 2.).unwrap();
        assert_eq!(d.draft.regions[0].ceiling, Some(5.));
        d.set(1, 14, 4.).unwrap();
        assert_eq!(d.draft.regions[0].ceiling, Some(6.));
        d.commit();
        let before = d.draft.clone();
        assert!(d.capture(1, 30., f32::NAN, 3.).is_err());
        assert_eq!(before, d.draft);
        d.capture(1, 30., 20., 6.).unwrap();
        d.undo();
        assert_eq!(before, d.draft);
        let index = d.duplicate(1).unwrap();
        assert_eq!(index, 2);
        assert_ne!(d.draft.regions[0].name, d.draft.regions[1].name);
        assert_eq!(d.draft.rig(1), d.draft.rig(2));
        d.draft.validate().unwrap();
        d.undo();
        assert_eq!(before, d.draft);
    }
    #[test]
    fn save_roundtrips_and_preserves_unrelated_scene_data() {
        let fixture = Fixture::new();
        let mut d = fixture.open();
        let original = d.raw.clone();
        d.set(1, 0, 10.).unwrap();
        d.set(1, 15, 72.).unwrap();
        d.save().unwrap();
        assert!(!d.dirty());
        let reopened = fixture.open();
        assert_eq!(reopened.draft, d.draft);
        assert_eq!(reopened.values(1).unwrap()[15], 72.);
        for key in ["items", "packages"] {
            assert_eq!(reopened.raw[key], original[key]);
        }
        assert_eq!(
            reopened.raw["camera"]["follow"],
            original["camera"]["follow"]
        );
        d.undo();
        assert!(d.dirty());
        d.save().unwrap();
        assert_eq!(fixture.open().values(1).unwrap()[0], 45.);
    }
    #[test]
    fn saving_an_orbit_scene_does_not_reenable_room_guidance() {
        let fixture = Fixture::new();
        let mut raw: serde_json::Value =
            serde_json::from_slice(&fs::read(&fixture.0).unwrap()).unwrap();
        let orbit = serde_json::json!({"min_distance":0.4,"max_distance":35,
            "clearance":0.12,"response_seconds":0.18,"lookahead_seconds":0});
        raw["camera"]["orbit"] = orbit.clone();
        fs::write(&fixture.0, serde_json::to_vec(&raw).unwrap()).unwrap();
        let mut d = fixture.open();
        d.set(1, 0, 45.).unwrap();
        d.save().unwrap();
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(&fixture.0).unwrap()).unwrap();
        assert_eq!(saved["camera"]["orbit"], orbit);
        assert_eq!(fixture.open().values(1).unwrap()[0], 45.);
    }
    #[test]
    fn external_edits_are_never_overwritten() {
        let fixture = Fixture::new();
        let mut d = fixture.open();
        d.set(0, 0, 20.).unwrap();
        fs::write(&fixture.0, b"external change").unwrap();
        assert!(d.save().unwrap_err().contains("changed on disk"));
        assert_eq!(fs::read(&fixture.0).unwrap(), b"external change");
        assert!(d.dirty());
    }
    #[test]
    fn entrances_share_validation_history_and_lossless_persistence() {
        let fixture = Fixture::new();
        let mut d = fixture.open();
        let selection = d.add_portal(1).unwrap();
        assert_eq!(selection, 2);
        assert_eq!(d.values(selection).unwrap()[8], 0.);
        assert_eq!(d.values(selection).unwrap()[9], 1.);
        let created = d.draft.clone();
        assert!(d.set(selection, 6, 20.).is_err());
        assert!(d.set(selection, 9, 99.).is_err());
        assert!(d.set(selection, 9, 0.).is_err());
        assert_eq!(d.draft, created);
        d.set(selection, 6, 1.5).unwrap();
        d.commit();
        d.undo();
        assert_eq!(d.draft, created);
        d.redo();
        d.save().unwrap();
        let reopened = fixture.open();
        assert_eq!(reopened.draft, d.draft);
        assert_eq!(reopened.raw["items"], d.raw["items"]);
        let before = d.draft.clone();
        assert!(
            d.set(1, 10, 1.).is_err(),
            "moving a boundary cannot silently strand its portal"
        );
        assert_eq!(d.draft, before);
        let mut malformed = d.raw.clone();
        malformed["portals"][0]["typo"] = serde_json::json!(1);
        fs::write(&fixture.0, serde_json::to_vec(&malformed).unwrap()).unwrap();
        assert!(Document::open(&fixture.0).is_err());
    }
    #[test]
    fn malformed_documents_and_unknown_commands_are_rejected() {
        let fixture = Fixture::new();
        for text in ["[]", "{}", "{\"camera\":4}"] {
            fs::write(&fixture.0, text).unwrap();
            assert!(Document::open(&fixture.0).is_err());
        }
        assert!(Command::try_from(0).is_err());
        assert!(Command::try_from(u32::MAX).is_err());
    }
}
