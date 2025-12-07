use super::types::{DualArrow, ExtractedPackets, PacketDirection, PacketInfo};
use crate::time_compression::TimeGap;
use qlog::events::EventData;
use std::collections::HashMap;
use std::sync::Arc;

pub fn build_single_file_arrows(
    packets: &[Arc<PacketInfo>],
    filtered_indices: &[usize],
) -> Vec<DualArrow> {
    filtered_indices
        .iter()
        .filter_map(|&idx| packets.get(idx))
        .map(|p| {
            let (from_left, recv_time, recv_event_idx) = match p.direction {
                PacketDirection::Sent => (true, p.time + 1.0, None),
                PacketDirection::Received => (false, p.time, Some(p.event_idx)),
            };

            DualArrow {
                packet: p.clone(),
                send_time: if from_left { p.time } else { p.time - 1.0 },
                recv_time,
                from_left,
                is_lost: false,
                sent_event_idx: p.event_idx,
                recv_event_idx,
            }
        })
        .collect()
}

pub fn build_dual_arrows(
    left_packets: &ExtractedPackets,
    right_packets: &ExtractedPackets,
) -> Vec<DualArrow> {
    let left_packets = &left_packets.packets;
    let right_packets = &right_packets.packets;

    let recv_info = |packets: &[Arc<PacketInfo>]| -> HashMap<(String, u64, u64), (f64, usize)> {
        packets
            .iter()
            .filter(|p| p.direction == PacketDirection::Received)
            .map(|p| {
                (
                    (
                        p.packet_type.clone(),
                        p.packet_number,
                        p.path_id.unwrap_or(0),
                    ),
                    (p.time, p.event_idx),
                )
            })
            .collect()
    };

    let left_recv = recv_info(left_packets);
    let right_recv = recv_info(right_packets);

    let mut arrows: Vec<DualArrow> = Vec::with_capacity(left_packets.len() + right_packets.len());

    for (packets, recv_info, from_left) in [
        (&left_packets, &right_recv, true),
        (&right_packets, &left_recv, false),
    ] {
        for p in packets
            .iter()
            .filter(|p| p.direction == PacketDirection::Sent)
        {
            let recv_data = recv_info
                .get(&(
                    p.packet_type.clone(),
                    p.packet_number,
                    p.path_id.unwrap_or(0),
                ))
                .copied();

            let (recv_time, recv_event_idx) = match recv_data {
                Some((time, idx)) => (time, Some(idx)),
                None => (p.time, None),
            };

            arrows.push(DualArrow {
                packet: p.clone(),
                send_time: p.time,
                recv_time,
                from_left,
                is_lost: recv_event_idx.is_none(),
                sent_event_idx: p.event_idx,
                recv_event_idx,
            });
        }
    }

    arrows.sort_by(|a, b| {
        a.send_time
            .partial_cmp(&b.send_time)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    arrows
}

pub fn compute_time_bounds(arrows: &[DualArrow]) -> (f64, f64, f64) {
    let min_time = arrows
        .iter()
        .map(|a| a.send_time.min(a.recv_time))
        .fold(f64::MAX, f64::min);
    let max_time = arrows
        .iter()
        .map(|a| a.send_time.max(a.recv_time))
        .fold(f64::MIN, f64::max);
    let time_range = (max_time - min_time).max(1.0);
    (min_time, max_time, time_range)
}

pub fn detect_gaps(arrows: &[DualArrow], min_time: f64, pixels_per_ms: f32) -> Vec<TimeGap> {
    if arrows.is_empty() {
        return Vec::new();
    }

    let mut event_times: Vec<f64> = arrows
        .iter()
        .flat_map(|a| [a.send_time, a.recv_time])
        .collect();
    event_times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    event_times.dedup();

    const MIN_VISUAL_HEIGHT_PX: f32 = 100.0;
    let min_gap_ms = (MIN_VISUAL_HEIGHT_PX / pixels_per_ms) as f64;

    let mut gaps = Vec::new();
    let mut prev_time = min_time;

    for &t in &event_times {
        let gap_size = t - prev_time;

        if gap_size > min_gap_ms {
            let arrows_in_flight = arrows
                .iter()
                .any(|a| a.send_time < t && a.recv_time > prev_time && !a.is_lost);

            if !arrows_in_flight {
                let margin = 5.0_f64;
                let gap_start = prev_time + margin;
                let gap_end = t - margin;
                let compressed_amount = gap_end - gap_start;

                if compressed_amount > min_gap_ms * 0.5 {
                    gaps.push(TimeGap {
                        start_time: gap_start,
                        end_time: gap_end,
                        compressed_amount,
                    });
                }
            }
        }
        prev_time = t;
    }

    gaps
}

pub fn count_simultaneous_sends(arrows: &[DualArrow]) -> HashMap<i64, usize> {
    let mut counts: HashMap<i64, usize> = HashMap::new();
    for arrow in arrows {
        let key = (arrow.send_time * 1000.0) as i64;
        *counts.entry(key).or_insert(0) += 1;
    }
    counts
}

pub fn point_to_line_segment_distance(px: f32, py: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;

    if len_sq == 0.0 {
        return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt();
    }

    let t = ((px - x1) * dx + (py - y1) * dy) / len_sq;
    let t = t.clamp(0.0, 1.0);

    let proj_x = x1 + t * dx;
    let proj_y = y1 + t * dy;

    ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt()
}

pub fn event_data_name(data: &EventData) -> String {
    let debug_str = format!("{:?}", data);
    debug_str
        .split(['(', '{'])
        .next()
        .unwrap_or("Unknown")
        .to_string()
}

pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{}B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KiB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1}MiB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1}GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
