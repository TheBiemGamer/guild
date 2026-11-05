//! Frame-time summary for the `LAUNCHER_BENCH` autoscroll run (the UI measures, this formats).

pub fn stats(frame_ms: &mut [f32], games: usize) -> String {
    frame_ms.sort_by(f32::total_cmp);
    let pct = |p: f32| frame_ms[((frame_ms.len() - 1) as f32 * p) as usize];
    let median = pct(0.5);
    let total: f32 = frame_ms.iter().sum();
    let slow = frame_ms.iter().filter(|&&ms| ms > median * 1.5).count();
    format!(
        "games {games} | frames {} | avg fps {:.1} | p50 {:.2}ms p95 {:.2}ms p99 {:.2}ms max {:.2}ms | >1.5x median: {slow}",
        frame_ms.len(),
        frame_ms.len() as f32 / total * 1000.0,
        median,
        pct(0.95),
        pct(0.99),
        pct(1.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_summarize_frames() {
        let mut ms = [10.0, 5.0, 5.0, 5.0];
        assert_eq!(
            stats(&mut ms, 3),
            "games 3 | frames 4 | avg fps 160.0 | p50 5.00ms p95 5.00ms p99 5.00ms max 10.00ms | >1.5x median: 1"
        );
    }
}
