#[derive(Debug, Clone)]
pub struct TimeGap {
    pub start_time: f64,
    pub end_time: f64,
    pub compressed_amount: f64,
}

pub const COMPRESSED_GAP_HEIGHT: f32 = 30.0;

pub fn time_to_compressed_y(
    time: f64,
    min_time: f64,
    gaps: &[TimeGap],
    pixels_per_ms: f32,
    top_y: f32,
) -> f32 {
    let mut total_compression = 0.0;
    for gap in gaps {
        if time > gap.end_time {
            total_compression += gap.compressed_amount;
        } else if time > gap.start_time {
            let within_gap = time - gap.start_time;
            let gap_duration = gap.end_time - gap.start_time;
            total_compression += within_gap / gap_duration * gap.compressed_amount;
        }
    }
    let adjusted_time = time - min_time - total_compression;
    top_y
        + (adjusted_time as f32 * pixels_per_ms)
        + (gaps.iter().filter(|g| time > g.end_time).count() as f32 * COMPRESSED_GAP_HEIGHT)
}

pub fn compute_compressed_height(time_range: f64, gaps: &[TimeGap], pixels_per_ms: f32) -> f32 {
    let total_compression: f64 = gaps.iter().map(|g| g.compressed_amount).sum();
    let compressed_time = time_range - total_compression;
    (compressed_time as f32 * pixels_per_ms) + (gaps.len() as f32 * COMPRESSED_GAP_HEIGHT) + 100.0
}

pub struct TimeContext<'a> {
    pub min_time: f64,
    pub max_time: f64,
    pub gaps: &'a [TimeGap],
    pub pixels_per_ms: f32,
}
