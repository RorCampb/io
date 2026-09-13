use crate::AnimationEvents;
use io_types::Vec3;

#[derive(Clone, Debug, PartialEq)]
pub enum Playback {
    Loop,
    Once { duration: f64 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnimationState {
    clip: usize,
    time: f64,
    previous_time: f64,
    speed: f32,
    events: Option<AnimationEvents>,
    playback: Playback,
}
impl AnimationState {
    pub fn clip(&self) -> usize {
        self.clip
    }
    pub fn time(&self) -> f64 {
        self.time
    }
    pub fn previous_time(&self) -> f64 {
        self.previous_time
    }
    pub fn speed(&self) -> f32 {
        self.speed
    }
    pub fn events(&self) -> Option<&AnimationEvents> {
        self.events.as_ref()
    }
    pub fn playback(&self) -> &Playback {
        &self.playback
    }
    pub fn set_speed(&mut self, speed: f32) -> Result<(), String> {
        if !speed.is_finite() || !(0.0..=64.0).contains(&speed) {
            return Err("animation speed must be finite and within 0..64".into());
        }
        self.speed = speed;
        Ok(())
    }
    pub fn seek(&mut self, time: f64) -> Result<(), String> {
        if !time.is_finite() || time < 0. {
            return Err("animation time must be finite and nonnegative".into());
        }
        self.time = match self.playback {
            Playback::Loop => time,
            Playback::Once { duration } => time.min(duration),
        };
        self.previous_time = self.time;
        Ok(())
    }
    pub fn set_events(&mut self, events: AnimationEvents) -> Result<(), String> {
        match self.playback {
            Playback::Loop => self.events = Some(events),
            Playback::Once { .. } => {
                return Err("loop event tracks require looping playback".into())
            }
        }
        Ok(())
    }
    pub(crate) fn freeze(&mut self) {
        self.speed = 0.;
        self.events = None;
        self.previous_time = self.time;
    }
    pub fn looping(clip: usize) -> Self {
        Self {
            clip,
            time: 0.,
            previous_time: 0.,
            speed: 1.,
            events: None,
            playback: Playback::Loop,
        }
    }

    pub fn once(clip: usize, duration: f64) -> Result<Self, String> {
        if !duration.is_finite() || duration <= 0. {
            return Err("one-shot animation needs a positive duration".into());
        }
        Ok(Self {
            playback: Playback::Once { duration },
            ..Self::looping(clip)
        })
    }

    pub fn sample_time(&self, alpha: f32, duration: f64) -> f32 {
        let alpha = if alpha.is_finite() { alpha } else { 1. };
        let time =
            self.previous_time + (self.time - self.previous_time) * f64::from(alpha.clamp(0., 1.));
        match self.playback {
            Playback::Loop => time.rem_euclid(duration.max(f64::EPSILON)) as f32,
            Playback::Once { duration } => time.clamp(0., duration) as f32,
        }
    }

    pub(crate) fn advance(&mut self, dt: f32) -> bool {
        self.previous_time = self.time;
        if !dt.is_finite() || dt <= 0. {
            return false;
        }
        let next = self.time + f64::from(dt) * f64::from(self.speed);
        if !next.is_finite() {
            return false;
        }
        self.time = next;
        if let Playback::Once { duration } = self.playback {
            self.time = self.time.min(duration);
        }
        self.time > self.previous_time
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checked_animation_mutators_preserve_state_on_rejected_input() {
        let mut state = AnimationState::looping(0);
        state.seek(0.5).unwrap();
        let saved = state.clone();
        for value in [f32::NAN, f32::INFINITY, -1., 65.] {
            assert!(state.set_speed(value).is_err());
            assert_eq!(state, saved);
        }
        for value in [f64::NAN, f64::INFINITY, -1.] {
            assert!(state.seek(value).is_err());
            assert_eq!(state, saved);
        }
        let mut once = AnimationState::once(0, 1.).unwrap();
        assert!(once
            .set_events(AnimationEvents::new(1., Vec::new()).unwrap())
            .is_err());
        once.seek(10.).unwrap();
        assert_eq!(once.time(), 1.);
        assert!(once.sample_time(f32::NAN, 1.).is_finite());
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PathMotion {
    points: Vec<Vec3>,
    lengths: Vec<f32>,
    total: f64,
    speed: f32,
    distance: f64,
}
impl PathMotion {
    pub fn new(points: Vec<Vec3>, speed: f32, distance: f64) -> Option<Self> {
        if points.len() < 2
            || points.iter().any(|p| !p.finite())
            || !speed.is_finite()
            || speed < 0.
            || !distance.is_finite()
        {
            return None;
        }
        let lengths: Vec<_> = points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .map(|(a, b)| {
                let d = *b - *a;
                d.dot(d).sqrt()
            })
            .collect();
        if lengths.iter().any(|l| !l.is_finite() || *l <= 0.) {
            return None;
        }
        let total = lengths.iter().map(|v| f64::from(*v)).sum();
        Some(Self {
            points,
            lengths,
            total,
            speed,
            distance,
        })
    }
    pub fn pose(&self) -> (Vec3, f32) {
        let mut remaining = self.distance.rem_euclid(self.total) as f32;
        for (i, &length) in self.lengths.iter().enumerate() {
            if remaining <= length || i + 1 == self.lengths.len() {
                let delta = self.points[(i + 1) % self.points.len()] - self.points[i];
                return (
                    self.points[i] + delta.scaled((remaining / length).clamp(0., 1.)),
                    delta.x.atan2(-delta.y),
                );
            }
            remaining -= length;
        }
        unreachable!()
    }
    pub(crate) fn advance(&mut self, dt: f32) -> (Vec3, f32) {
        self.distance =
            (self.distance + f64::from(dt) * f64::from(self.speed)).rem_euclid(self.total);
        self.pose()
    }
}
