#![forbid(unsafe_code)]
use crate::camera_boom::{CameraBoom, OrbitCamera};
use crate::camera_projection::CameraProjection;
use io_types::{Bounds, Vec3};
use io_world::Space;
use std::f32::consts::{FRAC_PI_4, TAU};

#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InteriorCamera {
    pub zoom: f32,
    pub pitch_degrees: f32,
    pub approach: f32,
    pub response_seconds: f32,
    pub lookahead_seconds: f32,
    pub yaw_limit_degrees: f32,
}
impl InteriorCamera {
    pub fn valid(self) -> bool {
        [
            self.zoom,
            self.pitch_degrees,
            self.approach,
            self.response_seconds,
            self.lookahead_seconds,
            self.yaw_limit_degrees,
        ]
        .iter()
        .all(|v| v.is_finite())
            && (1. ..=16.).contains(&self.zoom)
            && (5. ..=40.).contains(&self.pitch_degrees)
            && (0.5..=20.).contains(&self.approach)
            && (0.01..=1.).contains(&self.response_seconds)
            && (0. ..=1.).contains(&self.lookahead_seconds)
            && (0. ..=30.).contains(&self.yaw_limit_degrees)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Projection {
    Orthographic {},
    ZoomPerspective {
        start_zoom: f32,
        end_zoom: f32,
        vertical_fov_degrees: f32,
        near_clip: f32,
        smoothing_seconds: f32,
    },
}
impl Default for Projection {
    fn default() -> Self {
        Self::Orthographic {}
    }
}
impl Projection {
    pub fn valid(self) -> bool {
        match self {
            Self::Orthographic {} => true,
            Self::ZoomPerspective {
                start_zoom,
                end_zoom,
                vertical_fov_degrees,
                near_clip,
                smoothing_seconds,
            } => {
                [
                    start_zoom,
                    end_zoom,
                    vertical_fov_degrees,
                    near_clip,
                    smoothing_seconds,
                ]
                .iter()
                .all(|v| v.is_finite())
                    && (0.025..=16.).contains(&start_zoom)
                    && (start_zoom + 0.01..=16.).contains(&end_zoom)
                    && (20. ..=100.).contains(&vertical_fov_degrees)
                    && (0.001..=1.).contains(&near_clip)
                    && (0. ..=2.).contains(&smoothing_seconds)
            }
        }
    }
}

/// Viewport coverage covers a world-space height band without expanding simulation.
#[derive(Clone, Copy, Debug, Default, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RenderCoverage {
    #[default]
    Radius,
    Viewport {
        min_z: f32,
        max_z: f32,
    },
}
impl RenderCoverage {
    pub fn valid(self) -> bool {
        match self {
            Self::Radius => true,
            Self::Viewport { min_z, max_z } => {
                min_z.is_finite()
                    && max_z.is_finite()
                    && min_z.abs() <= 100_000.
                    && max_z.abs() <= 100_000.
                    && min_z <= max_z
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Camera {
    anchor: Option<RoomAnchor>,
    room_views: std::collections::HashMap<io_world::InteriorId, crate::camera_rig::RigController>,
    active_location: io_world::SpaceLocation,
    target_easing: bool,
    authored: Option<crate::camera_rig::RigController>,
    envelope: crate::camera_envelope::Envelope,
    envelope_context: Option<crate::camera_envelope::Context>,
    boom: Option<CameraBoom>,
    interior: Option<InteriorCamera>,
    interior_blend: f32,
    interior_yaw: f32,
    target: Vec3,
    render_distance: f32,
    coverage: RenderCoverage,
    zoom: f32,
    desired_zoom: f32,
    projection: Projection,
    yaw: f32,
    pitch: f32,
    width: i32,
    height: i32,
}
#[derive(Clone, Debug)]
struct RoomAnchor {
    room: io_world::InteriorId,
    follow: crate::camera_rig::RigController,
}
pub struct SpaceView<'a> {
    pub rigs: &'a io_scene::CameraRigs,
    pub regions: &'a [io_world::Interior],
    pub portals: &'a [io_world::Portal],
    pub location: io_world::SpaceLocation,
    pub point: Vec3,
}
struct Guidance {
    selection: crate::camera_rig::Selection,
    spaces: Vec<Bounds>,
    anchored: bool,
}

impl Camera {
    pub fn room_anchored(&self) -> bool {
        self.anchor.is_some()
    }
    fn leave_anchor(&mut self) {
        if let Some(anchor) = self.anchor.take() {
            if let Some(current) = self.authored.take() {
                self.room_views.insert(anchor.room, current);
            }
            self.authored = Some(anchor.follow);
            self.target_easing = true;
        }
    }
    pub fn toggle_room_anchor(&mut self) -> bool {
        if self.anchor.is_some() {
            self.leave_anchor();
            return true;
        }
        let io_world::SpaceLocation::Interior(room) = self.active_location else {
            return false;
        };
        let Some(context) = &self.envelope_context else {
            return false;
        };
        if !matches!(context.motion, io_scene::RigMotion::InteriorEnvelope { .. }) {
            return false;
        }
        let Some(follow) = self.authored.take() else {
            return false;
        };
        let mut pose = follow.pose();
        pose.yaw_degrees = ((pose.yaw_degrees / 90.).round() * 90. + 180.).rem_euclid(360.) - 180.;
        pose.pitch_degrees = 12.;
        pose.zoom = pose.min_zoom;
        self.authored = Some(
            self.room_views
                .remove(&room)
                .unwrap_or_else(|| crate::camera_rig::RigController::held(&pose)),
        );
        self.anchor = Some(RoomAnchor { room, follow });
        self.target_easing = true;
        true
    }
    pub fn track_space(&mut self, s: SpaceView<'_>, seconds: f32) {
        use io_world::{InteriorId, SpaceLocation};
        self.active_location = s.location;
        if self
            .anchor
            .as_ref()
            .is_some_and(|a| s.location != SpaceLocation::Interior(a.room))
        {
            self.leave_anchor();
        }
        let selection = match s.location {
            SpaceLocation::Interior(InteriorId(id)) => crate::camera_rig::Selection {
                region: s
                    .regions
                    .get(id as usize)
                    .filter(|r| s.rigs.interiors.contains_key(&r.name))
                    .map(|_| id as usize),
                weight: 1.,
            },
            SpaceLocation::Exterior => s
                .portals
                .iter()
                .filter_map(|p| {
                    let destination = if p.from == SpaceLocation::Exterior {
                        p.to
                    } else if p.to == SpaceLocation::Exterior {
                        p.from
                    } else {
                        return None;
                    };
                    let SpaceLocation::Interior(InteriorId(id)) = destination else {
                        return None;
                    };
                    let rig = s
                        .regions
                        .get(id as usize)
                        .and_then(|r| s.rigs.interiors.get(&r.name))?;
                    let (_, weight) = p.approach(SpaceLocation::Exterior, s.point, rig.approach)?;
                    Some(crate::camera_rig::Selection {
                        region: Some(id as usize),
                        weight,
                    })
                })
                .max_by(|a, b| a.weight.total_cmp(&b.weight))
                .unwrap_or(crate::camera_rig::Selection {
                    region: None,
                    weight: 0.,
                }),
        };
        let foot = self
            .anchor
            .as_ref()
            .and_then(|a| s.regions.get(a.room.0 as usize))
            .map_or(s.point, |r| {
                let p = r.bounds.center();
                Vec3::new(p.x, p.y, r.bounds.min.z)
            });
        let guidance = (!s.portals.is_empty() || self.anchor.is_some()).then(|| Guidance {
            selection,
            spaces: self
                .anchor
                .as_ref()
                .and_then(|a| s.regions.get(a.room.0 as usize))
                .map_or_else(
                    || crate::camera_envelope::portal_spaces(s.regions, s.portals, s.rigs),
                    |r| {
                        let mut b = r.bounds;
                        b.max.z = r.ceiling.unwrap_or(b.max.z);
                        vec![b]
                    },
                ),
            anchored: self.anchor.is_some(),
        });
        let h = self.track(s.rigs, s.regions, Some(foot), seconds, guidance);
        let goal = foot + Vec3::new(0., 0., h);
        if self.target_easing {
            self.target = self.target
                + (goal - self.target)
                    .scaled(-(-seconds / s.rigs.exterior.response_seconds).exp_m1());
            let d = goal - self.target;
            if d.dot(d) < 1e-6 {
                self.target_easing = false;
            }
        } else {
            self.target = goal;
        }
        if let Some(context) = &mut self.envelope_context {
            context.foot = Some(self.target - Vec3::new(0., 0., h));
        }
        self.advance_envelope(0.);
    }
    pub fn track_rigs(
        &mut self,
        rigs: &io_scene::CameraRigs,
        regions: &[io_world::Interior],
        point: Option<Vec3>,
        seconds: f32,
    ) -> f32 {
        self.track(rigs, regions, point, seconds, None)
    }
    fn track(
        &mut self,
        rigs: &io_scene::CameraRigs,
        regions: &[io_world::Interior],
        point: Option<Vec3>,
        seconds: f32,
        guidance: Option<Guidance>,
    ) -> f32 {
        self.boom = None;
        self.interior = None;
        let rig = self
            .authored
            .get_or_insert_with(|| crate::camera_rig::RigController::new(&rigs.exterior));
        match &guidance {
            Some(g) => rig.update_selected(rigs, regions, point, seconds, Some(g.selection)),
            None => rig.update(rigs, regions, point, seconds),
        }
        let pose = rig.pose();
        let region = rig.region().and_then(|i| regions.get(i)).cloned();
        let approach = region
            .as_ref()
            .and_then(|r| rigs.interiors.get(&r.name))
            .map_or(rigs.exterior.approach, |r| r.approach);
        self.envelope_context = Some(crate::camera_envelope::Context {
            motion: rigs.motion,
            region,
            foot: point,
            approach,
            spaces: guidance.as_ref().map_or_else(
                || crate::camera_envelope::spaces(regions, rigs),
                |g| g.spaces.clone(),
            ),
            weight: guidance.as_ref().map(|g| g.selection.weight),
            anchored: guidance.as_ref().is_some_and(|g| g.anchored),
        });
        self.advance_envelope(seconds);
        pose.target_height
    }
    pub fn advance_envelope(&mut self, seconds: f32) -> bool {
        if !seconds.is_finite() || seconds < 0. {
            return false;
        }
        let (Some(context), Some(controller)) = (&self.envelope_context, &self.authored) else {
            return false;
        };
        let near = match self.projection {
            Projection::Orthographic {} => 0.05,
            Projection::ZoomPerspective { near_clip, .. } => near_clip,
        };
        self.envelope.update(crate::camera_envelope::Step {
            motion: context.motion,
            rig: &controller.pose(),
            region: context.region.as_ref(),
            foot: context.foot,
            approach: context.approach,
            aspect: self.width as f32 / self.height as f32,
            near,
            seconds,
            spaces: &context.spaces,
            weight: context.weight,
            anchored: context.anchored,
        });
        matches!(context.motion, io_scene::RigMotion::InteriorEnvelope { .. })
    }
    pub fn preview_rig(&mut self, rig: &io_scene::CameraRig, target: Vec3) {
        self.anchor = None;
        self.target_easing = false;
        self.active_location = io_world::SpaceLocation::Exterior;
        self.envelope = Default::default();
        self.envelope_context = None;
        self.boom = None;
        self.interior = None;
        self.authored = Some(crate::camera_rig::RigController::new(rig));
        self.target = target;
    }
    pub fn inspect_region(&mut self, bounds: Bounds) {
        self.anchor = None;
        self.target_easing = false;
        self.active_location = io_world::SpaceLocation::Exterior;
        self.envelope = Default::default();
        self.envelope_context = None;
        self.authored = None;
        self.boom = None;
        self.interior = None;
        self.yaw = FRAC_PI_4;
        self.pitch = 35_f32.to_radians();
        self.target = bounds.center();
        self.set_zoom((20. / bounds.extent().x.max(bounds.extent().y).max(2.)).clamp(0.025, 16.));
    }
    pub fn capture_rig(&self) -> (f32, f32, f32) {
        let (y, p) = self.angles();
        (
            (y.to_degrees() + 180.).rem_euclid(360.) - 180.,
            p.to_degrees(),
            self.effective_zoom(),
        )
    }
    pub fn set_orbit_camera(&mut self, settings: Option<OrbitCamera>) -> bool {
        if settings.is_some_and(|s| !s.valid())
            || (settings.is_some() && matches!(self.projection, Projection::Orthographic {}))
        {
            return false;
        }
        if settings.is_some() {
            self.anchor = None;
            self.authored = None;
            self.envelope = Default::default();
            self.envelope_context = None;
            self.interior = None;
        }
        self.boom = settings.map(CameraBoom::new);
        let (min, max) = self.zoom_limits();
        self.zoom = self.zoom.clamp(min, max);
        self.desired_zoom = self.desired_zoom.clamp(min, max);
        true
    }
    fn orbit_lens(&self) -> Option<(f32, f32, f32)> {
        let boom = self.boom.as_ref()?;
        let Projection::ZoomPerspective {
            vertical_fov_degrees,
            near_clip,
            ..
        } = self.projection
        else {
            return None;
        };
        let tangent = (vertical_fov_degrees.to_radians() * 0.5).tan();
        let requested = (self.height as f32 / (32. * self.zoom * tangent))
            .clamp(boom.settings.min_distance, boom.settings.max_distance);
        Some((requested, tangent, near_clip))
    }
    pub fn resolve_orbit(
        &mut self,
        world: &dyn io_world::WorldView,
        velocity: Vec3,
        exclude: Option<u64>,
        seconds: f32,
    ) -> bool {
        let Some((requested, tangent, near)) = self.orbit_lens() else {
            return false;
        };
        let (yaw, pitch) = self.intent_angles();
        let direction = crate::camera_steering::direction(yaw, pitch);
        // Enclose the near-plane rectangle as well as the eye, including wide windows.
        let near_radius = near
            * (1. + tangent.powi(2) * (1. + (self.width as f32 / self.height as f32).powi(2)))
                .sqrt();
        self.boom
            .as_mut()
            .unwrap()
            .update(crate::camera_boom::BoomStep {
                world,
                pivot: self.target,
                direction,
                requested,
                velocity,
                exclude,
                seconds,
                near_radius,
            })
    }
    /// Orbit in free space; redirect eye movement along wall contact planes.
    pub fn orbit_around(
        &mut self,
        world: &dyn io_world::WorldView,
        exclude: Option<u64>,
        yaw: f32,
        pitch: f32,
    ) -> bool {
        if !yaw.is_finite() || !pitch.is_finite() {
            return false;
        }
        if self
            .boom
            .as_ref()
            .is_none_or(|b| b.settings.avoidance.is_some())
        {
            return self.orbit(yaw, pitch);
        }
        self.resolve_orbit(world, Vec3::default(), exclude, 0.);
        let (requested, tangent, near) = self.orbit_lens().unwrap();
        let boom = self.boom.as_ref().unwrap();
        let distance = boom.distance(requested);
        let radius = boom.collision_radius(
            near * (1. + tangent.powi(2) * (1. + (self.width as f32 / self.height as f32).powi(2)))
                .sqrt(),
        );
        let start = (self.yaw, self.pitch);
        let dy = yaw.clamp(-std::f32::consts::PI, std::f32::consts::PI);
        self.orbit(dy, pitch);
        let dp = self.pitch - start.1;
        let sweep = |start: (f32, f32), dy: f32, dp: f32, distance: f32| {
            let eye = |t: f32| {
                self.target
                    + crate::camera_steering::direction(start.0 + dy * t, start.1 + dp * t)
                        .scaled(distance)
            };
            let clear = |from, to| {
                io_world::cast_sphere(world, eye(from), eye(to), radius, exclude)
                    .is_ok_and(|t| t == 1.)
                    && io_world::cast_sphere(world, self.target, eye(to), radius, exclude)
                        .is_ok_and(|t| t == 1.)
            };
            // Small arc segments plus swept chords catch blockers between clear endpoints.
            let steps = (dy.abs().max(dp.abs()) / 2_f32.to_radians()).ceil().max(1.) as u32;
            let mut accepted = 0.;
            for i in 1..=steps {
                let end = i as f32 / steps as f32;
                if clear(accepted, end) {
                    accepted = end;
                } else {
                    let mut blocked = end;
                    for _ in 0..12 {
                        let mid = (accepted + blocked) * 0.5;
                        if clear(accepted, mid) {
                            accepted = mid;
                        } else {
                            blocked = mid;
                        }
                    }
                    break;
                }
            }
            accepted
        };
        let steps = (dy.abs().max(dp.abs()) / 2_f32.to_radians()).ceil().max(1.) as u32;
        let (sy, sp) = (dy / steps as f32, dp / steps as f32);
        let mut next = start;
        let mut resolved_distance = distance;
        for _ in 0..steps {
            let accepted = sweep(next, sy, sp, resolved_distance);
            if accepted < 1. {
                let eye = self.target
                    + crate::camera_steering::direction(next.0, next.1).scaled(resolved_distance);
                let desired = self.target
                    + crate::camera_steering::direction(next.0 + sy, next.1 + sp)
                        .scaled(resolved_distance);
                if let Some(mut slid) =
                    crate::camera_slide::slide(world, eye, desired - eye, radius, exclude)
                {
                    let offset = slid - self.target;
                    let length = offset.dot(offset).sqrt();
                    if length > requested {
                        slid = self.target + offset.scaled(requested / length);
                    }
                    let offset = slid - self.target;
                    let length = offset.dot(offset).sqrt();
                    // Retain line of sight and the requested maximum distance. Sliding
                    // never licenses passing a second obstacle or cutting a corner.
                    let visible = io_world::cast_sphere(world, self.target, slid, radius, exclude)
                        .is_ok_and(|t| t == 1.);
                    let safe = io_world::cast_sphere(world, eye, slid, radius, exclude)
                        .is_ok_and(|t| t == 1.);
                    if length > 0.001 && visible && safe {
                        let angles = crate::camera_steering::angles(offset.scaled(1. / length));
                        if angles.1.abs() <= 85_f32.to_radians() {
                            next = angles;
                            resolved_distance = length;
                            continue;
                        }
                    }
                }
            }
            next.0 += sy * accepted;
            next.1 += sp * accepted;
        }
        self.yaw = next.0.rem_euclid(TAU);
        self.pitch = next.1;
        self.boom.as_mut().unwrap().place_eye(resolved_distance);
        true
    }
    fn zoom_limits(&self) -> (f32, f32) {
        match (&self.boom, self.projection) {
            (
                Some(boom),
                Projection::ZoomPerspective {
                    vertical_fov_degrees,
                    ..
                },
            ) => {
                let scale =
                    self.height as f32 / (32. * (vertical_fov_degrees.to_radians() * 0.5).tan());
                (
                    scale / boom.settings.max_distance,
                    scale / boom.settings.min_distance,
                )
            }
            _ => (0.025, 16.),
        }
    }
    pub fn set_interior_camera(&mut self, value: Option<InteriorCamera>) -> bool {
        if value.is_some_and(|v| !v.valid()) {
            return false;
        }
        self.interior = value;
        self.interior_blend = 0.;
        true
    }
    pub fn track_interior(
        &mut self,
        volumes: &[io_world::Interior],
        foot: Option<(Vec3, Vec3)>,
        seconds: f32,
    ) -> f32 {
        let Some(settings) = self.interior else {
            return 0.;
        };
        let selected = foot.and_then(|(p, v)| {
            let prediction = p + v.scaled(settings.lookahead_seconds);
            volumes
                .iter()
                .filter(|v| v.ceiling.is_some())
                .map(|v| {
                    (
                        v,
                        v.proximity(prediction, settings.approach)
                            .max(v.proximity(p, settings.approach)),
                    )
                })
                .max_by(|a, b| a.1.total_cmp(&b.1))
        });
        let goal = selected.map_or(0., |(_, weight)| weight);
        if let Some((v, _)) = selected.filter(|(_, weight)| *weight > 0.) {
            self.interior_yaw = (-v.entry_direction.x).atan2(-v.entry_direction.y);
        }
        self.interior_blend +=
            (goal - self.interior_blend) * -(-seconds / settings.response_seconds).exp_m1();
        self.interior_blend = self.interior_blend.clamp(0., 1.);
        self.interior_blend
    }
    fn angles(&self) -> (f32, f32) {
        if let Some(r) = &self.authored {
            let p = r.pose();
            let mut pitch = p.pitch_degrees.to_radians();
            if let Some(e) = self.envelope.pose {
                pitch += (e.rise.atan2(e.retreat).max(0.001) - pitch) * self.envelope.blend;
            }
            return (p.yaw_degrees.to_radians(), pitch);
        }
        self.boom
            .as_ref()
            .and_then(|b| b.direction())
            .map_or_else(|| self.intent_angles(), crate::camera_steering::angles)
    }
    fn intent_angles(&self) -> (f32, f32) {
        match self.interior {
            Some(s) => {
                let delta = (self.yaw - self.interior_yaw + std::f32::consts::PI).rem_euclid(TAU)
                    - std::f32::consts::PI;
                let constrained = delta.clamp(
                    -s.yaw_limit_degrees.to_radians(),
                    s.yaw_limit_degrees.to_radians(),
                );
                (
                    if self.boom.is_some() {
                        self.yaw
                    } else {
                        self.yaw + (constrained - delta) * self.interior_blend
                    },
                    if self.boom.is_some() {
                        let default_pitch = (1.0_f32 / 3.0_f32.sqrt()).asin();
                        (self.pitch
                            + (s.pitch_degrees.to_radians() - default_pitch) * self.interior_blend)
                            .clamp(2_f32.to_radians(), 85_f32.to_radians())
                    } else {
                        self.pitch
                            + (s.pitch_degrees.to_radians() - self.pitch) * self.interior_blend
                    },
                )
            }
            None => (self.yaw, self.pitch),
        }
    }
    fn effective_zoom(&self) -> f32 {
        if let Some(r) = &self.authored {
            return self.envelope.zoom.unwrap_or_else(|| r.pose().zoom);
        }
        if let Some((requested, tangent, _)) = self.orbit_lens() {
            let radius = self.boom.as_ref().unwrap().distance(requested).max(0.001);
            return self.height as f32 / (32. * radius * tangent);
        }
        self.interior.map_or(self.zoom, |s| {
            (self.zoom.ln() + (s.zoom.max(self.zoom).ln() - self.zoom.ln()) * self.interior_blend)
                .exp()
        })
    }
    pub fn target(&self) -> Vec3 {
        self.target
    }
    pub fn render_distance(&self) -> f32 {
        self.render_distance
    }
    pub fn set_coverage(&mut self, coverage: RenderCoverage) -> bool {
        if !coverage.valid() {
            return false;
        }
        self.coverage = coverage;
        true
    }
    fn orthographic_radius(&self) -> f32 {
        if self.boom.is_some() {
            // Perspective coverage is computed from the frustum below; a horizontal
            // orbit must not divide by sin(pitch) in the orthographic estimate.
            return self.render_distance;
        }
        match self.coverage {
            RenderCoverage::Radius => self.render_distance,
            RenderCoverage::Viewport { min_z, max_z } => {
                let (hw, hh) = self.half_view();
                let dz = (min_z - self.target.z)
                    .abs()
                    .max((max_z - self.target.z).abs());
                let (sp, cp) = self.angles().1.sin_cos();
                // Screen-up = ground-up * sin(pitch) + height * cos(pitch).
                let ground_half = (hh + dz * cp) / sp;
                self.render_distance.max(hw.hypot(ground_half).hypot(dz))
            }
        }
    }
    pub fn render_radius(&self) -> f32 {
        self.view().radius
    }
    pub fn set_projection(&mut self, projection: Projection) -> bool {
        if !projection.valid()
            || (self.boom.is_some() && matches!(projection, Projection::Orthographic {}))
        {
            return false;
        }
        self.projection = projection;
        self.desired_zoom = self.zoom;
        true
    }
    pub fn view(&self) -> CameraProjection {
        let (right, up, forward) = self.basis();
        let (half_width, half_height) = self.half_view();
        let (mut convergence, near_clip) = match self.projection {
            Projection::Orthographic {} => (0., 0.05),
            Projection::ZoomPerspective {
                start_zoom,
                end_zoom,
                vertical_fov_degrees,
                near_clip,
                ..
            } => {
                let t = ((self.effective_zoom().ln() - start_zoom.ln())
                    / (end_zoom.ln() - start_zoom.ln()))
                .clamp(0., 1.);
                let blend = t * t * (3. - 2. * t);
                (
                    blend
                        * (self
                            .authored
                            .as_ref()
                            .map_or(vertical_fov_degrees, |r| r.pose().fov_degrees)
                            .to_radians()
                            * 0.5)
                            .tan()
                        / (self.height as f32 / (2. * self.base_pixels_per_unit())),
                    near_clip,
                )
            }
        };
        if let Some((requested, _, _)) = self.orbit_lens() {
            convergence = 1. / self.boom.as_ref().unwrap().distance(requested).max(0.001);
        }
        if let Some(p) = self.envelope.pose {
            convergence +=
                (1. / p.retreat.hypot(p.rise).max(0.05) - convergence) * self.envelope.blend;
        }
        let depth = self.orthographic_radius() * 2. + 1024.;
        // A fixed tiny near distance at the receding virtual eye collapses depth
        // precision during the blend. This harmonic bound approaches the old
        // orthographic front plane continuously and keeps depth useful near target.
        let precision_front = depth / (1. + convergence * depth);
        let front = if self.boom.is_some() {
            let eye_distance = 1. / convergence;
            eye_distance - near_clip.min(eye_distance * 0.5)
        } else if convergence > 0. {
            let eye_distance = 1. / convergence;
            precision_front.min(eye_distance - near_clip.min(eye_distance * 0.5))
        } else {
            depth
        };
        let mut view = CameraProjection {
            target: self.target,
            right,
            up,
            forward,
            half_width,
            half_height,
            width: self.width as f32,
            height: self.height as f32,
            convergence,
            front,
            back: -depth,
            radius: self.render_distance,
        };
        match self.coverage {
            RenderCoverage::Radius => (),
            RenderCoverage::Viewport { min_z, max_z } => view.cover_height_band(min_z, max_z),
        }
        view
    }
    pub fn set_target(&mut self, target: Vec3) -> bool {
        if !target.finite() {
            return false;
        }
        self.target = target;
        true
    }
    pub fn set_zoom(&mut self, zoom: f32) -> bool {
        if !zoom.is_finite() || !(0.025..=16.).contains(&zoom) {
            return false;
        }
        self.zoom = zoom;
        self.desired_zoom = zoom;
        true
    }
    pub fn copy_viewport(&mut self, other: &Self) {
        self.set_viewport(other.width, other.height);
    }
    pub fn new(target: Vec3) -> Self {
        assert!(target.finite(), "camera target must be finite");
        Self {
            anchor: None,
            room_views: Default::default(),
            active_location: io_world::SpaceLocation::Exterior,
            target_easing: false,
            authored: None,
            envelope: Default::default(),
            envelope_context: None,
            boom: None,
            interior: None,
            interior_blend: 0.,
            interior_yaw: 0.,
            target,
            render_distance: 120.,
            coverage: RenderCoverage::Radius,
            zoom: 1.,
            desired_zoom: 1.,
            projection: Projection::default(),
            yaw: FRAC_PI_4,
            pitch: (1.0_f32 / 3.0_f32.sqrt()).asin(),
            width: 1280,
            height: 800,
        }
    }
    pub fn orbit(&mut self, yaw: f32, pitch: f32) -> bool {
        if !yaw.is_finite() || !pitch.is_finite() {
            return false;
        }
        if let Some(r) = &mut self.authored {
            r.orbit(yaw, pitch);
            self.advance_envelope(0.);
            return true;
        }
        if yaw != 0. || pitch != 0. {
            let (intent_yaw, intent_pitch) = self.intent_angles();
            if let Some(boom) = &mut self.boom {
                if let Some(visible) = boom.manual_orbit() {
                    let (visible_yaw, visible_pitch) = crate::camera_steering::angles(visible);
                    // Begin dragging from the visible view, not a hidden pre-avoidance angle.
                    self.yaw += visible_yaw - intent_yaw;
                    self.pitch += visible_pitch - intent_pitch;
                }
            }
        }
        self.yaw = (self.yaw + yaw.rem_euclid(TAU)).rem_euclid(TAU);
        let min_pitch = if self.boom.is_some() { -85_f32 } else { 5_f32 };
        self.pitch = (self.pitch + pitch).clamp(min_pitch.to_radians(), 85.0_f32.to_radians());
        true
    }
    pub fn zoom_by(&mut self, steps: f32) -> bool {
        if !steps.is_finite() {
            return false;
        }
        if let Some(r) = &mut self.authored {
            r.zoom(steps);
            return true;
        }
        let (min, max) = self.zoom_limits();
        self.desired_zoom = (self.desired_zoom.clamp(min, max)
            * (steps.clamp(-100., 100.) * 0.12).exp())
        .clamp(min, max);
        match self.projection {
            Projection::Orthographic {} => self.zoom = self.desired_zoom,
            Projection::ZoomPerspective {
                smoothing_seconds: 0.,
                ..
            } => self.zoom = self.desired_zoom,
            Projection::ZoomPerspective { .. } => (),
        }
        true
    }
    /// Presentation-time easing, independent of physics ticks and reversible mid-scroll.
    pub fn advance(&mut self, seconds: f32) -> bool {
        if !seconds.is_finite() || seconds <= 0. || self.zoom == self.desired_zoom {
            return false;
        }
        let smoothing = match self.projection {
            Projection::Orthographic {} => 0.,
            Projection::ZoomPerspective {
                smoothing_seconds, ..
            } => smoothing_seconds,
        };
        let error = self.desired_zoom.ln() - self.zoom.ln();
        self.zoom = if smoothing == 0. || error.abs() < 1e-5 {
            self.desired_zoom
        } else {
            (self.zoom.ln() + error * -(-seconds / smoothing).exp_m1()).exp()
        };
        true
    }
    pub fn set_distance(&mut self, distance: f32) -> bool {
        if !distance.is_finite() || distance <= 0. {
            return false;
        }
        self.render_distance = distance.clamp(8., 20000.);
        true
    }
    pub fn set_viewport(&mut self, w: i32, h: i32) -> bool {
        if w <= 0 || h <= 0 || (w == self.width && h == self.height) {
            return false;
        }
        if self.boom.is_some() {
            // Orbit zoom represents distance, not pixels; resizing must not dolly.
            let ratio = h as f32 / self.height as f32;
            self.zoom *= ratio;
            self.desired_zoom *= ratio;
        }
        self.width = w;
        self.height = h;
        self.advance_envelope(0.);
        true
    }
    pub fn basis(&self) -> (Vec3, Vec3, Vec3) {
        let (yaw, pitch) = self.angles();
        let (s, c) = yaw.sin_cos();
        let (sp, cp) = pitch.sin_cos();
        (
            Vec3::new(c, -s, 0.),
            Vec3::new(-s * sp, -c * sp, cp),
            Vec3::new(s * cp, c * cp, sp),
        )
    }
    fn base_pixels_per_unit(&self) -> f32 {
        let lens_scale = match (&self.authored, self.projection) {
            (
                Some(r),
                Projection::ZoomPerspective {
                    start_zoom,
                    end_zoom,
                    vertical_fov_degrees,
                    ..
                },
            ) => {
                let t = ((self.effective_zoom().ln() - start_zoom.ln())
                    / (end_zoom.ln() - start_zoom.ln()))
                .clamp(0., 1.);
                let ratio = (r.pose().fov_degrees.to_radians() * 0.5).tan()
                    / (vertical_fov_degrees.to_radians() * 0.5).tan();
                1. + (ratio - 1.) * t * t * (3. - 2. * t)
            }
            _ => 1.,
        };
        16. * self.effective_zoom() / lens_scale
    }
    pub fn pixels_per_unit(&self) -> f32 {
        let mut half_height = self.height as f32 / (2. * self.base_pixels_per_unit());
        if let Some(p) = self.envelope.pose {
            let goal = p.retreat.hypot(p.rise).max(0.05) * (p.fov.to_radians() * 0.5).tan();
            half_height += (goal - half_height) * self.envelope.blend;
        }
        self.height as f32 / (2. * half_height)
    }
    pub fn fov_degrees(&self) -> f32 {
        let base = self.authored.as_ref().map_or_else(
            || match self.projection {
                Projection::ZoomPerspective {
                    vertical_fov_degrees,
                    ..
                } => vertical_fov_degrees,
                Projection::Orthographic {} => 45.,
            },
            |r| r.pose().fov_degrees,
        );
        self.envelope
            .pose
            .map_or(base, |p| base + (p.fov - base) * self.envelope.blend)
    }
    /// Unit-speed ground motion aligned with screen right/up, independent of zoom.
    pub fn ground_direction(&self, x: f32, y: f32) -> Option<Vec3> {
        if !x.is_finite() || !y.is_finite() || x.abs() > 1. || y.abs() > 1. {
            return None;
        }
        let (right, _, _) = self.basis();
        let yaw = self.angles().0;
        let up = Vec3::new(-yaw.sin(), -yaw.cos(), 0.);
        let direction = right.scaled(x) + up.scaled(y);
        Some(direction.scaled(1. / (x * x + y * y).sqrt().max(1.)))
    }
    pub fn project(&self, point: Vec3) -> Option<(f32, f32)> {
        self.view().project(point)
    }
    /// Ray in logical window pixels, clipped to this view's near/far planes.
    pub fn pick_depth(&self, x: f32, y: f32, bounds: Bounds) -> Option<f32> {
        self.view().pick_depth(x, y, bounds)
    }
    pub fn half_view(&self) -> (f32, f32) {
        (
            self.width as f32 / (2. * self.pixels_per_unit()),
            self.height as f32 / (2. * self.pixels_per_unit()),
        )
    }
    pub fn pan(&mut self, dx: f32, dy: f32, space: &Space) -> bool {
        if !dx.is_finite() || !dy.is_finite() {
            return false;
        }
        let (right, _, _) = self.basis();
        let (yaw, pitch) = self.angles();
        let ground_up = Vec3::new(-yaw.sin(), -yaw.cos(), 0.);
        let delta = right.scaled(-dx / self.pixels_per_unit())
            + ground_up.scaled(dy / (self.pixels_per_unit() * pitch.sin()));
        let target = self.target + delta;
        if !target.finite() {
            return false;
        }
        let target = space.clamp_target(target);
        if let Some(context) = &mut self.envelope_context {
            if let Some(foot) = &mut context.foot {
                *foot = *foot + (target - self.target);
            }
        }
        self.set_target(target);
        self.advance_envelope(0.);
        true
    }
    #[cfg(test)]
    pub fn sees(&self, bounds: Bounds) -> bool {
        self.view().sees(bounds)
    }
    #[cfg(test)]
    pub fn clip_from_world(&self) -> [f32; 16] {
        self.view().matrix()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn free_orbit() -> Camera {
        let mut c = hybrid();
        c.set_target(Vec3::new(0., 0., 1.33));
        c.set_zoom(8.);
        c.set_orbit_camera(Some(OrbitCamera {
            target_height_fraction: Some(0.7),
            avoidance: None,
            min_distance: 0.4,
            max_distance: 35.,
            clearance: 0.12,
            response_seconds: 0.18,
            lookahead_seconds: 0.,
        }));
        c.orbit(-c.yaw, -c.pitch);
        c
    }
    fn blocker(id: u64, at: Vec3, half: Vec3) -> io_world::Item {
        io_world::Item {
            id,
            transform: io_world::Transform::new(at, Vec3::new(1., 1., 1.), 0.).unwrap(),
            collider: Some(io_world::Collider::new(io_world::ColliderShape::Box {
                half_extents: half,
            })),
            physics_body: Some(io_world::PhysicsBody::new(io_world::BodyKind::Static)),
            ..Default::default()
        }
    }
    #[test]
    fn repeated_overhead_input_does_not_ratchet_radius_inward() {
        let mut c = free_orbit();
        let w = io_world::World::new(
            Space::new(Vec3::new(100., 100., 100.)),
            vec![blocker(1, Vec3::new(0., 0., 4.5), Vec3::new(30., 30., 0.5))],
        );
        c.resolve_orbit(&w, Vec3::default(), None, 0.);
        let distance = 1. / c.view().convergence;
        for _ in 0..600 {
            c.orbit_around(&w, None, 0., 0.03);
            c.resolve_orbit(&w, Vec3::default(), None, 1. / 60.);
            // Rebuilding a dirty presentation frame also resolves collision at dt=0.
            c.resolve_orbit(&w, Vec3::default(), None, 0.);
        }
        assert!(
            (1. / c.view().convergence - distance).abs() < 0.002,
            "held overhead orbit changed distance from {distance} to {}",
            1. / c.view().convergence
        );
        assert!(
            c.pitch > 0.1 && c.pitch < 1.,
            "overhead probe must actually reach ceiling, pitch {}",
            c.pitch
        );
    }
    #[test]
    fn manual_orbit_circles_actor_and_stops_at_floor_without_dollying() {
        let mut c = free_orbit();
        let w = io_world::World::new(
            Space::new(Vec3::new(100., 100., 100.)),
            vec![
                blocker(1, Vec3::new(0., 0., -0.5), Vec3::new(40., 40., 0.5)),
                blocker(2, Vec3::new(0., 0., 1.), Vec3::new(0.35, 0.35, 1.)),
            ],
        );
        let distance = 1. / c.view().convergence;
        let target = c.target;
        for _ in 0..360 {
            assert!(c.orbit_around(&w, Some(2), TAU / 360., 0.));
            c.resolve_orbit(&w, Vec3::default(), Some(2), 1. / 60.);
            assert!((1. / c.view().convergence - distance).abs() < 1e-4);
            assert_eq!(c.target, target);
        }
        assert!(c.basis().2.dot(Vec3::new(0., 1., 0.)) > 0.9999);
        assert!(c.orbit_around(&w, Some(2), 0., -1.));
        c.resolve_orbit(&w, Vec3::default(), Some(2), 1. / 60.);
        assert!((1. / c.view().convergence - distance).abs() < 1e-4);
        assert!(c.pitch < 0. && c.pitch > -0.3);
        let stopped = c.pitch;
        c.orbit_around(&w, Some(2), 0., 0.1);
        assert!(c.pitch > stopped + 0.09, "reverse must release immediately");
    }
    #[test]
    fn orbit_slides_past_a_wall_without_tunneling_or_changing_zoom_intent() {
        let mut c = free_orbit();
        let w = io_world::World::new(
            Space::new(Vec3::new(100., 100., 100.)),
            vec![blocker(1, Vec3::new(3., 0., 2.), Vec3::new(0.01, 20., 20.))],
        );
        let desired_zoom = c.desired_zoom;
        let target = c.target;
        c.orbit_around(&w, None, std::f32::consts::PI, 0.);
        c.resolve_orbit(&w, Vec3::default(), None, 1. / 60.);
        assert!(
            c.yaw > 0.6,
            "camera must progress along wall, not stop at first contact"
        );
        assert_eq!(c.desired_zoom, desired_zoom);
        assert_eq!(c.target, target);
        let distance = 1. / c.view().convergence;
        let eye = c.target + c.basis().2.scaled(distance);
        assert!(eye.x < 2.88);
        assert_eq!(
            io_world::cast_sphere(&w, c.target, eye, 0.12, None).unwrap(),
            1.
        );
    }
    #[test]
    fn orbit_distance_survives_resize_and_camera_can_look_up_or_down() {
        let mut c = hybrid();
        c.set_orbit_camera(Some(OrbitCamera {
            target_height_fraction: None,
            avoidance: None,
            min_distance: 0.4,
            max_distance: 35.,
            clearance: 0.12,
            response_seconds: 0.18,
            lookahead_seconds: 0.,
        }));
        let requested = c.orbit_lens().unwrap().0;
        c.set_viewport(800, 1600);
        assert!((c.orbit_lens().unwrap().0 - requested).abs() < 1e-5);
        c.set_coverage(RenderCoverage::Viewport {
            min_z: -5.,
            max_z: 20.,
        });
        c.orbit(0., -c.pitch);
        assert!(c.view().matrix().iter().all(|v| v.is_finite()));
        c.orbit(0., -2.);
        assert!(c.basis().2.z < 0.);
        c.orbit(0., 4.);
        assert!(c.basis().2.z > 0.);
        assert!(c.view().matrix().iter().all(|v| v.is_finite()));
    }
    #[test]
    fn bounded_orbit_preserves_scroll_intent_and_shared_projection() {
        use io_world::{BodyKind, Collider, ColliderShape, Item, PhysicsBody, Transform, World};
        let mut c = hybrid();
        c.set_zoom(3.);
        c.set_orbit_camera(Some(OrbitCamera {
            target_height_fraction: None,
            avoidance: None,
            min_distance: 1.2,
            max_distance: 24.,
            clearance: 0.15,
            response_seconds: 0.18,
            lookahead_seconds: 0.25,
        }));
        let world = World::new(
            Space::new(Vec3::new(100., 100., 100.)),
            vec![Item {
                id: 1,
                transform: Transform::new(Vec3::new(50., 54., 1.), Vec3::new(1., 1., 1.), 0.)
                    .unwrap(),
                collider: Some(Collider::new(ColliderShape::Box {
                    half_extents: Vec3::new(20., 0.01, 20.),
                })),
                physics_body: Some(PhysicsBody::new(BodyKind::Static)),
                ..Item::default()
            }],
        );
        c.resolve_orbit(&world, Vec3::default(), None, 0.);
        let before = c.view();
        assert_eq!(c.desired_zoom, 3.);
        assert!(1. / before.convergence < 10.);
        let eye = before.target + before.forward.scaled(1. / before.convergence);
        assert_eq!(
            io_world::cast_sphere(&world, before.target, eye, 0.15, None).unwrap(),
            1.
        );
        assert_eq!(c.project(c.target).unwrap(), (640., 400.));
        let p = c.target + before.right.scaled(0.3);
        let xy = c.project(p).unwrap();
        assert!(c.pick_depth(xy.0, xy.1, point_bounds(p)).is_some());
        let matrix = before.matrix();
        let clip: [f32; 4] = std::array::from_fn(|i| {
            matrix[i] * p.x + matrix[i + 4] * p.y + matrix[i + 8] * p.z + matrix[i + 12]
        });
        assert!((xy.0 - (clip[0] / clip[3] + 1.) * 640.).abs() < 0.01);
        let mut independent = c.clone();
        c.zoom_by(-100.);
        c.advance(100.);
        assert!((c.orbit_lens().unwrap().0 - 24.).abs() < 1e-4);
        c.zoom_by(1.);
        c.advance(100.);
        assert!(
            c.orbit_lens().unwrap().0 < 24.,
            "scroll must respond immediately after reaching the bound"
        );
        c.zoom_by(100.);
        c.advance(100.);
        assert!((c.orbit_lens().unwrap().0 - 1.2).abs() < 1e-4);
        assert_eq!(independent.desired_zoom, 3.);
        assert!(!independent.set_projection(Projection::Orthographic {}));
        independent.orbit(std::f32::consts::PI, 0.);
        let clear = World::new(Space::new(Vec3::new(100., 100., 100.)), Vec::new());
        for _ in 0..180 {
            independent.resolve_orbit(&clear, Vec3::default(), None, 1. / 60.);
        }
        assert!(
            (1. / independent.view().convergence - independent.orbit_lens().unwrap().0).abs()
                < 0.01
        );
    }
    fn hybrid() -> Camera {
        let mut camera = Camera::new(Vec3::new(50., 50., 1.));
        assert!(camera.set_projection(Projection::ZoomPerspective {
            start_zoom: 1.5,
            end_zoom: 5.,
            vertical_fov_degrees: 45.,
            near_clip: 0.05,
            smoothing_seconds: 0.16,
        }));
        camera
    }
    fn point_bounds(p: Vec3) -> Bounds {
        let e = Vec3::new(0.01, 0.01, 0.01);
        Bounds {
            min: p - e,
            max: p + e,
        }
    }
    #[test]
    fn transition_preserves_depth_resolution_between_separate_surfaces() {
        let mut camera = hybrid();
        for (width, height) in [(1280, 800), (1940, 1044), (800, 1280)] {
            camera.set_viewport(width, height);
            for step in 0..=900 {
                let zoom = 1.5 + step as f32 * 0.005;
                camera.set_zoom(zoom);
                let view = camera.view();
                let m = view.matrix().map(f64::from);
                let depth = |p: Vec3| {
                    let row = |i| {
                        m[i] * f64::from(p.x)
                            + m[4 + i] * f64::from(p.y)
                            + m[8 + i] * f64::from(p.z)
                            + m[12 + i]
                    };
                    ((row(2) / row(3) + 1.) * 0.5 * 16_777_215.).round()
                };
                // Even a 1 cm gap must not collapse to a single 24-bit depth value.
                let separation =
                    depth(camera.target) - depth(camera.target + view.forward.scaled(0.01));
                assert!(
                    separation >= 16.,
                    "lost depth precision at zoom {zoom}, {width}x{height}: {separation} levels"
                );
            }
        }
    }
    #[test]
    fn projection_contract_rejects_invalid_settings_without_mutation() {
        let mut camera = hybrid();
        let before = camera.clip_from_world();
        for (start, end, fov, near, smooth) in [
            (5., 1., 45., 0.05, 0.16),
            (1., 1., 45., 0.05, 0.16),
            (1., 5., 0., 0.05, 0.16),
            (1., 5., 180., 0.05, 0.16),
            (1., 5., 45., 0., 0.16),
            (1., 5., 45., 0.05, -1.),
            (f32::NAN, 5., 45., 0.05, 0.16),
            (1., 5., 45., f32::INFINITY, 0.16),
        ] {
            assert!(!camera.set_projection(Projection::ZoomPerspective {
                start_zoom: start,
                end_zoom: end,
                vertical_fov_degrees: fov,
                near_clip: near,
                smoothing_seconds: smooth,
            }));
            assert_eq!(camera.clip_from_world(), before);
        }
        assert!(
            serde_json::from_str::<Projection>(r#"{"type":"orthographic","unexpected":1}"#)
                .is_err()
        );
        assert!(serde_json::from_str::<Projection>(
            r#"{"type":"zoom_perspective","start_zoom":1}"#
        )
        .is_err());
    }
    #[test]
    fn hybrid_projection_matches_matrix_picking_and_target_plane_scale() {
        let mut camera = hybrid();
        for zoom in [0.025, 1.5, 1.501, 2., 3.5, 4.999, 5., 16.] {
            for (width, height) in [(1280, 800), (800, 1280), (2400, 600)] {
                camera.set_zoom(zoom);
                camera.set_viewport(width, height);
                camera.orbit(0.3, 0.);
                let view = camera.view();
                let m = view.matrix();
                for z in [-5., 0., view.front * 0.2] {
                    let p = camera.target
                        + view.right.scaled(0.3)
                        + view.up.scaled(0.2)
                        + view.forward.scaled(z);
                    let (x, y) = camera.project(p).unwrap();
                    let clip: [f32; 4] = std::array::from_fn(|i| {
                        m[i] * p.x + m[4 + i] * p.y + m[8 + i] * p.z + m[12 + i]
                    });
                    assert!((x - (clip[0] / clip[3] + 1.) * width as f32 * 0.5).abs() < 0.03);
                    assert!((y - (1. - clip[1] / clip[3]) * height as f32 * 0.5).abs() < 0.03);
                    if z == 0. {
                        assert!(
                            (x - width as f32 * 0.5 - 0.3 * camera.pixels_per_unit()).abs() < 0.005
                        );
                    }
                    if point_bounds(p).within_radius(camera.target, camera.render_radius()) {
                        assert!(camera.sees(point_bounds(p)));
                        assert!(
                            camera.pick_depth(x, y, point_bounds(p)).is_some(),
                            "zoom {zoom}"
                        );
                    }
                }
                let behind = camera.target + view.forward.scaled(view.front + 1.);
                assert!(camera.project(behind).is_none());
                assert!(!camera.sees(point_bounds(behind)));
            }
        }
    }
    #[test]
    fn hybrid_blend_is_continuous_and_orthographic_endpoint_is_unchanged() {
        let mut camera = hybrid();
        let mut ortho = Camera::new(camera.target);
        for zoom in [0.025, 1., 1.5] {
            camera.set_zoom(zoom);
            ortho.set_zoom(zoom);
            assert_eq!(camera.clip_from_world(), ortho.clip_from_world());
        }
        for threshold in [1.5, 5.] {
            camera.set_zoom(threshold - 0.00001);
            let a = camera
                .project(camera.target + Vec3::new(2., 1., 1.))
                .unwrap();
            camera.set_zoom(threshold + 0.00001);
            let b = camera
                .project(camera.target + Vec3::new(2., 1., 1.))
                .unwrap();
            assert!((a.0 - b.0).abs() < 0.02 && (a.1 - b.1).abs() < 0.02);
        }
        camera.set_zoom(5.);
        let view = camera.view();
        let eye_distance = 1. / view.convergence;
        assert!((2. * (view.half_height / eye_distance).atan().to_degrees() - 45.).abs() < 0.001);
    }
    #[test]
    fn hybrid_zoom_easing_is_time_based_reversible_and_bounded() {
        let mut a = hybrid();
        let mut b = a.clone();
        a.zoom_by(10.);
        b.zoom_by(10.);
        a.advance(0.5);
        for _ in 0..72 {
            b.advance(0.5 / 72.);
        }
        assert!((a.zoom - b.zoom).abs() < 1e-4);
        assert!(a.zoom > 1. && a.zoom < a.desired_zoom);
        let before = a.zoom;
        a.zoom_by(-20.);
        a.advance(0.1);
        assert!(a.zoom < before && a.zoom > a.desired_zoom);
        assert!(!a.advance(f32::NAN) && !a.advance(-1.));
        a.zoom_by(100.);
        a.advance(100.);
        assert!((a.zoom - 16.).abs() < 1e-4);
        a.zoom_by(-100.);
        a.advance(100.);
        assert!((a.zoom - 0.025).abs() < 1e-5);
    }
    #[test]
    fn perspective_lod_and_picking_account_for_depth() {
        let mut camera = hybrid();
        camera.set_zoom(5.);
        let view = camera.view();
        let scale = Vec3::new(1., 1., 1.);
        let local = Bounds {
            min: Vec3::new(-0.5, -0.5, -0.5),
            max: Vec3::new(0.5, 0.5, 0.5),
        };
        let front = camera.target + view.forward.scaled(2.);
        let back = camera.target - view.forward.scaled(2.);
        assert!(
            view.projected_diameter(local, scale, front)
                > view.projected_diameter(local, scale, back)
        );
        let (x, y) = camera.project(camera.target).unwrap();
        assert!(
            camera.pick_depth(x, y, point_bounds(front)).unwrap()
                < camera.pick_depth(x, y, point_bounds(back)).unwrap()
        );
        let crossing_eye = camera.target + view.forward.scaled(1. / view.convergence);
        assert_eq!(
            view.projected_diameter(local, scale, crossing_eye),
            f32::MAX
        );
    }
    #[test]
    fn hybrid_viewport_coverage_contains_visible_height_slab_without_expanding_simulation() {
        let mut camera = hybrid();
        camera.set_distance(8.);
        camera.set_coverage(RenderCoverage::Viewport {
            min_z: -5.,
            max_z: 20.,
        });
        for pitch in [5_f32, 35., 85.] {
            camera.pitch = pitch.to_radians();
            for zoom in [0.025, 1.5, 1.6, 2.5, 5., 16.] {
                camera.set_zoom(zoom);
                for (width, height) in [(1280, 800), (800, 1280), (2400, 600)] {
                    camera.set_viewport(width, height);
                    let view = camera.view();
                    for x in [-0.99, 0., 0.99] {
                        for y in [-0.99, 0., 0.99] {
                            let plane = view.right.scaled(x * view.half_width)
                                + view.up.scaled(y * view.half_height);
                            let axis = view.forward - plane.scaled(view.convergence);
                            for height in [-5., 0., 20.] {
                                if axis.z.abs() < 1e-6 {
                                    continue;
                                }
                                let depth = (height - camera.target.z - plane.z) / axis.z;
                                if depth < view.back || depth > view.front {
                                    continue;
                                }
                                let p = camera.target + plane + axis.scaled(depth);
                                assert!(
                                    camera.sees(point_bounds(p)),
                                    "{pitch}/{zoom}/{width}/{height}: {p:?}"
                                );
                            }
                        }
                    }
                    assert_eq!(camera.render_distance(), 8.);
                }
            }
        }
    }
    #[test]
    fn viewport_coverage_contains_height_band_corners_at_any_orbit_and_zoom() {
        let mut c = Camera::new(Vec3::new(-280., -180., 25.));
        c.set_distance(180.);
        assert!(!c.set_coverage(RenderCoverage::Viewport {
            min_z: 10.,
            max_z: 0.
        }));
        assert!(!c.set_coverage(RenderCoverage::Viewport {
            min_z: f32::NAN,
            max_z: 0.
        }));
        assert!(serde_json::from_str::<RenderCoverage>(
            r#"{"type":"viewport","min_z":0,"max_z":100,"extra":1}"#
        )
        .is_err());
        assert!(c.set_coverage(RenderCoverage::Viewport {
            min_z: -10.,
            max_z: 100.
        }));
        for pitch in [5_f32, 35., 85.] {
            c.pitch = pitch.to_radians();
            for zoom in [0.025, 0.1, 0.65, 16.] {
                c.set_zoom(zoom);
                for (w, h) in [(1280, 800), (800, 1280), (2400, 600)] {
                    c.set_viewport(w, h);
                    c.orbit(0.7, 0.);
                    let (right, _, _) = c.basis();
                    let ground_up = Vec3::new(-c.yaw.sin(), -c.yaw.cos(), 0.);
                    let (hw, hh) = c.half_view();
                    for z in [-10., 100.] {
                        for x in [-hw, hw] {
                            for y in [-hh, hh] {
                                let dz = z - c.target.z;
                                let p = c.target
                                    + right.scaled(x)
                                    + ground_up.scaled((y - dz * c.pitch.cos()) / c.pitch.sin())
                                    + Vec3::new(0., 0., dz);
                                let bounds = Bounds {
                                    min: p - Vec3::new(0.1, 0.1, 0.1),
                                    max: p + Vec3::new(0.1, 0.1, 0.1),
                                };
                                assert!(c.sees(bounds), "missing corner at {pitch}/{zoom}/{w}/{h}");
                            }
                        }
                    }
                    assert_eq!(c.render_distance(), 180., "simulation radius must not grow");
                }
            }
        }
    }
    #[test]
    fn movement_and_picking_follow_orbit_zoom_and_logical_viewport() {
        let mut camera = Camera::new(Vec3::new(3., 4., 0.));
        for yaw in [0., 0.7, 1.2, 3.] {
            camera.orbit(yaw, 0.1);
            let (right, up, _) = camera.basis();
            let w = camera.ground_direction(0., 1.).unwrap();
            let d = camera.ground_direction(1., 0.).unwrap();
            assert!(w.dot(up) > 0. && w.dot(right).abs() < 1e-5);
            assert!(d.dot(right) > 0.99 && d.dot(up).abs() < 1e-5);
            let diagonal = camera.ground_direction(1., 1.).unwrap();
            assert!((diagonal.dot(diagonal) - 1.).abs() < 1e-5);
            for (width, height, zoom) in [(1280, 800, 2.), (640, 400, 4.)] {
                camera.set_viewport(width, height);
                camera.set_zoom(zoom);
                let bounds = Bounds {
                    min: Vec3::new(3., 4., 0.),
                    max: Vec3::new(4., 5., 2.),
                };
                let (x, y) = camera.project(bounds.center()).unwrap();
                assert!(camera.pick_depth(x, y, bounds).is_some());
                assert!(camera.pick_depth(-1., y, bounds).is_none());
                assert!(camera.pick_depth(f32::NAN, y, bounds).is_none());
            }
        }
        assert!(camera.ground_direction(f32::NAN, 0.).is_none());
    }
    #[test]
    fn camera_matrix_centers_target_and_zoom_scales_view() {
        let mut c = Camera::new(Vec3::new(5000., 5000., 0.));
        c.orbit(0.7, 0.2);
        let m = c.clip_from_world();
        for row in 0..3 {
            let v = m[row] * c.target.x
                + m[4 + row] * c.target.y
                + m[8 + row] * c.target.z
                + m[12 + row];
            assert!(v.abs() < 0.001);
        }
        let before = c.half_view().0;
        c.zoom_by(2.0_f32.ln() / 0.12);
        assert!((c.half_view().0 - before * 0.5).abs() < 0.001);
        assert!(!c.orbit(f32::NAN, 0.));
        assert!(!c.zoom_by(f32::INFINITY));
        assert!(!c.set_distance(-1.));
    }
    #[test]
    fn overflowing_pan_preserves_camera_state() {
        let mut camera = Camera::new(Vec3::new(10., 10., 0.));
        camera.set_zoom(0.025);
        let target = camera.target();
        let space = Space::new(Vec3::new(100., 100., 100.));
        assert!(!camera.pan(f32::MAX, f32::MAX, &space));
        assert_eq!(camera.target(), target);
        assert!(camera.clip_from_world().iter().all(|v| v.is_finite()));
    }
    #[test]
    fn bounds_overlapping_view_or_distance_are_not_lost() {
        let c = Camera::new(Vec3::new(0., 0., 0.));
        let (right, _, _) = c.basis();
        let p = right.scaled(c.half_view().0 + 1.);
        let b = Bounds {
            min: p - Vec3::new(4., 4., 4.),
            max: p + Vec3::new(4., 4., 4.),
        };
        assert!(c.sees(b));
        let far = Bounds {
            min: Vec3::new(500., 500., 0.),
            max: Vec3::new(501., 501., 1.),
        };
        assert!(!c.sees(far));
    }
}
