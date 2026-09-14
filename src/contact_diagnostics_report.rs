use io_world::{ContactPassStats, CORRECTION_SPEED_BOUNDS, CORRECTION_SPEED_THRESHOLD};

#[derive(serde::Serialize)]
pub struct ContactReport {
    timings_include_diagnostic_overhead: bool,
    correction_speed_threshold: f64,
    histogram_upper_bounds: [f64; 5],
    passes: Vec<PassReport>,
    windows: Vec<WindowReport>,
}
#[derive(serde::Serialize)]
struct PassReport {
    iteration: usize,
    visits: u64,
    correction_histogram: [u64; 6],
    end_pass_histogram: [u64; 6],
    small_then_large: u64,
    small_but_unsettled: u64,
    supporting_small: u64,
    invalid_samples: u64,
    max_correction_speed: f64,
    max_end_pass_correction_speed: f64,
}
fn reports(passes: &[ContactPassStats]) -> Vec<PassReport> {
    passes
        .iter()
        .enumerate()
        .map(|(i, p)| PassReport {
            iteration: i + 1,
            visits: p.visits,
            correction_histogram: p.correction_histogram,
            end_pass_histogram: p.end_pass_histogram,
            small_then_large: p.small_then_large,
            small_but_unsettled: p.small_but_unsettled,
            supporting_small: p.supporting_small,
            invalid_samples: p.invalid_samples,
            max_correction_speed: p.max_correction_speed,
            max_end_pass_correction_speed: p.max_end_pass_correction_speed,
        })
        .collect()
}
#[derive(serde::Serialize)]
struct WindowReport {
    first_tick: usize,
    last_tick: usize,
    passes: Vec<PassReport>,
}
#[derive(Default)]
pub(crate) struct Builder {
    all: Vec<ContactPassStats>,
    window: Vec<ContactPassStats>,
    windows: Vec<WindowReport>,
    last_tick: usize,
    window_start: usize,
}
impl Builder {
    pub fn record(&mut self, tick: usize, passes: &[ContactPassStats]) {
        if self.all.is_empty() {
            self.all.resize(passes.len(), ContactPassStats::default());
        }
        if self.window.is_empty() {
            self.window
                .resize(passes.len(), ContactPassStats::default());
            self.window_start = tick;
        }
        assert_eq!(self.all.len(), passes.len());
        assert_eq!(self.window.len(), passes.len());
        for ((total, window), pass) in self.all.iter_mut().zip(&mut self.window).zip(passes) {
            total.accumulate(pass);
            window.accumulate(pass);
        }
        self.last_tick = tick;
        if tick % 30 == 0 {
            self.flush();
        }
    }
    fn flush(&mut self) {
        if !self.window.is_empty() {
            self.windows.push(WindowReport {
                first_tick: self.window_start,
                last_tick: self.last_tick,
                passes: reports(&self.window),
            });
            self.window.clear();
        }
    }
    pub fn finish(mut self) -> ContactReport {
        self.flush();
        ContactReport {
            timings_include_diagnostic_overhead: true,
            correction_speed_threshold: CORRECTION_SPEED_THRESHOLD,
            histogram_upper_bounds: CORRECTION_SPEED_BOUNDS,
            passes: reports(&self.all),
            windows: self.windows,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_cover_partial_tail_and_preserve_totals() {
        let mut builder = Builder::default();
        for tick in 1..=31 {
            builder.record(
                tick,
                &[ContactPassStats {
                    visits: 1,
                    correction_histogram: [1, 0, 0, 0, 0, 0],
                    end_pass_histogram: [1, 0, 0, 0, 0, 0],
                    ..Default::default()
                }],
            );
        }
        let report = builder.finish();
        assert_eq!(report.passes[0].visits, 31);
        assert_eq!(report.windows.len(), 2);
        assert_eq!(
            (report.windows[0].first_tick, report.windows[0].last_tick),
            (1, 30)
        );
        assert_eq!(
            (report.windows[1].first_tick, report.windows[1].last_tick),
            (31, 31)
        );
        assert_eq!(report.windows[0].passes[0].visits, 30);
        assert_eq!(report.windows[1].passes[0].visits, 1);
        assert_eq!(report.passes[0].correction_histogram[0], 31);
        let json = serde_json::to_value(report).unwrap();
        assert_eq!(json["timings_include_diagnostic_overhead"], true);
    }
}
