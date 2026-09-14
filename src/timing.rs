#![forbid(unsafe_code)]
//! Validated startup timing; measured throughput never changes the physics step.
use serde::Deserialize;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "TimingConfig")]
pub struct SimulationTiming {
    tick_hz: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TimingConfig {
    #[serde(default = "default_hz")]
    tick_hz: u32,
}

fn default_hz() -> u32 {
    30
}

impl Default for SimulationTiming {
    fn default() -> Self {
        Self {
            tick_hz: default_hz(),
        }
    }
}

impl TryFrom<TimingConfig> for SimulationTiming {
    type Error = String;
    fn try_from(value: TimingConfig) -> Result<Self, Self::Error> {
        Self::new(value.tick_hz)
    }
}

impl SimulationTiming {
    pub fn new(tick_hz: u32) -> Result<Self, String> {
        // World::simulate accepts steps up to 0.25 seconds. The upper bound is
        // a configuration guard, not a promise that a workload can sustain it.
        if !(4..=1000).contains(&tick_hz) {
            return Err("simulation.tick_hz must be an integer within 4..1000".into());
        }
        Ok(Self { tick_hz })
    }

    pub fn tick_hz(self) -> u32 {
        self.tick_hz
    }

    pub fn seconds(self) -> f64 {
        1. / f64::from(self.tick_hz)
    }

    pub fn period(self) -> Duration {
        Duration::from_secs_f64(self.seconds())
    }

    pub fn max_catch_up_ticks(self) -> u32 {
        self.tick_hz.div_ceil(4)
    }

    pub fn alpha(self, elapsed: f64) -> f32 {
        (elapsed / self.seconds()).clamp(0., 1.) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_derives_step_schedule_and_interpolation() {
        for hz in [4, 30, 60, 144, 1000] {
            let timing = SimulationTiming::new(hz).unwrap();
            assert_eq!(timing.tick_hz(), hz);
            assert!((timing.seconds() * f64::from(hz) - 1.).abs() < 1e-12);
            assert!((timing.period().as_secs_f64() - timing.seconds()).abs() < 1e-9);
            assert_eq!(timing.alpha(timing.seconds() / 2.), 0.5);
            assert_eq!(timing.alpha(timing.seconds() * 10.), 1.);
            assert_eq!(timing.alpha(-1.), 0.);
        }
    }

    #[test]
    fn strict_config_rejects_invalid_rates_and_unknown_fields() {
        assert_eq!(
            serde_json::from_str::<SimulationTiming>("{}").unwrap(),
            SimulationTiming::default()
        );
        assert_eq!(
            serde_json::from_str::<SimulationTiming>(r#"{"tick_hz":144}"#)
                .unwrap()
                .tick_hz(),
            144
        );
        for json in [
            r#"{"tick_hz":0}"#,
            r#"{"tick_hz":3}"#,
            r#"{"tick_hz":1001}"#,
            r#"{"tick_hz":-1}"#,
            r#"{"tick_hz":144.5}"#,
            r#"{"tick_hz":null}"#,
            r#"{"tick_hz":"144"}"#,
            r#"{"tick_hz":144,"adaptive":true}"#,
        ] {
            assert!(
                serde_json::from_str::<SimulationTiming>(json).is_err(),
                "{json}"
            );
        }
    }
}
