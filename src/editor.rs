use io_types::{Bounds, Vec3};
pub struct Editor {
    pub document: io_editor::Document,
    pub selected: usize,
    pub playing: bool,
    pub preview: bool,
    pub trajectory: bool,
    pub guides: crate::camera_guides::GuideCache,
    pub status: String,
}
impl Editor {
    pub fn refresh_guides(
        &mut self,
        camera: &crate::camera::Camera,
        config: &crate::config::SceneConfig,
    ) {
        let regions = self.regions(config.origin);
        let Ok(portals) = self
            .document
            .draft()
            .portals
            .iter()
            .map(|p| p.resolve(&regions, config.origin))
            .collect::<Result<Vec<_>, _>>()
        else {
            return;
        };
        let speed = config.traversal.as_ref().map_or(2., |t| {
            if self
                .selected
                .checked_sub(1)
                .and_then(|i| regions.get(i))
                .is_some_and(|r| r.bounds.extent().z < t.standing_height)
            {
                t.crouch_speed
            } else {
                t.walk_speed
            }
        });
        let view = camera.view();
        self.guides.refresh(crate::camera_guides::Request {
            draft: self.document.draft(),
            regions: &regions,
            portals: &portals,
            selected: self.selected,
            viewport: (view.width as i32, view.height as i32),
            projection: config.camera.projection,
            speed,
            show: self.trajectory,
        });
    }
    pub fn regions(&self, origin: [f32; 3]) -> Vec<io_world::Interior> {
        let p = |a: [f32; 3]| Vec3::new(a[0] + origin[0], a[1] + origin[1], a[2] + origin[2]);
        self.document
            .draft()
            .regions
            .iter()
            .map(|r| io_world::Interior {
                name: r.name.clone(),
                bounds: Bounds {
                    min: p(r.min),
                    max: p(r.max),
                },
                ceiling: r.ceiling.map(|z| z + origin[2]),
                entry_direction: Vec3::new(
                    r.entry_direction[0],
                    r.entry_direction[1],
                    r.entry_direction[2],
                ),
            })
            .collect()
    }
}
#[repr(C)]
pub struct IoEditorView {
    pub enabled: u32,
    pub playing: u32,
    pub dirty: u32,
    pub preview: u32,
    pub selected: u32,
    pub count: u32,
    pub trajectory: u32,
    pub envelope: u32,
    pub portal: u32,
    pub names: [[u8; 64]; 8],
    pub values: [f32; 16],
    pub status: [u8; 160],
    pub path: [u8; 256],
}
impl Default for IoEditorView {
    fn default() -> Self {
        Self {
            enabled: 0,
            playing: 0,
            dirty: 0,
            preview: 0,
            selected: 0,
            count: 0,
            trajectory: 0,
            envelope: 0,
            portal: 0,
            names: [[0; 64]; 8],
            values: [0.; 16],
            status: [0; 160],
            path: [0; 256],
        }
    }
}
fn text(out: &mut [u8], value: &str) {
    for (slot, b) in out.iter_mut().take(value.len()).zip(value.bytes()) {
        *slot = if b.is_ascii() { b } else { b'?' };
    }
    *out.last_mut().unwrap() = 0;
}
#[no_mangle]
pub unsafe extern "C" fn io_app_editor_enable(app: *mut crate::IoApp) -> bool {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return false;
    };
    match app.state.enable_editor() {
        Ok(()) => true,
        Err(e) => {
            eprintln!("Editor: {e}");
            false
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn io_app_editor_command(
    app: *mut crate::IoApp,
    kind: u32,
    index: u32,
    field: u32,
    value: f32,
) -> bool {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return false;
    };
    app.state
        .editor_command(kind, index as usize, field as usize, value)
}
#[no_mangle]
pub unsafe extern "C" fn io_app_editor_view(
    app: *const crate::IoApp,
    out: *mut IoEditorView,
) -> bool {
    let (Some(app), Some(out)) = (unsafe { app.as_ref() }, unsafe { out.as_mut() }) else {
        return false;
    };
    *out = IoEditorView::default();
    let Some(e) = app.state.editor() else {
        return true;
    };
    out.enabled = 1;
    out.playing = e.playing as u32;
    out.dirty = e.document.dirty() as u32;
    out.preview = e.preview as u32;
    out.trajectory = e.trajectory as u32;
    out.envelope = matches!(
        e.document.draft().rigs.motion,
        io_scene::RigMotion::InteriorEnvelope { .. }
    ) as u32;
    out.selected = e.selected as u32;
    out.portal = e.document.draft().portal_index(e.selected).is_some() as u32;
    out.count = e.document.draft().selection_count() as u32;
    out.values = e.document.values(e.selected).unwrap_or([0.; 16]);
    for (i, name) in out.names.iter_mut().enumerate() {
        let index = e.selected / 8 * 8 + i;
        if index == 0 {
            text(name, "Exterior");
        } else if let Some(r) = e.document.draft().regions.get(index - 1) {
            text(name, &r.name);
        } else if let Some(i) = e.document.draft().portal_index(index) {
            text(name, &format!("P: {}", e.document.draft().portals[i].name));
        }
    }
    text(&mut out.status, &e.status);
    text(&mut out.path, &e.document.path().display().to_string());
    true
}
