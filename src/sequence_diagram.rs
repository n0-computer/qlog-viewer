use crate::app::LoadedFile;
use crate::packet_correlation::PacketCorrelation;
use crate::qlog_data::QlogData;
use crate::utils::{self, FrameType};
use egui::{Color32, Painter, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Vec2};
use qlog::events::EventData;
use std::collections::{BTreeSet, HashMap};

const HEADER_HEIGHT: f32 = 40.0;
const DUAL_LEFT_MARGIN: f32 = 180.0;
const DUAL_RIGHT_MARGIN: f32 = 180.0;
const GAP_INDICATOR_MARGIN: f32 = 120.0;
const SENT_COLOR: Color32 = Color32::from_rgb(0, 150, 255);
const RECEIVED_COLOR: Color32 = Color32::from_rgb(220, 80, 80);
const LOSS_COLOR: Color32 = Color32::RED;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualizationMode {
    ColorCoded,
    VerticalLanes,
}

// Path color palette for multipath visualization
const PATH_COLORS: &[(u8, u8, u8)] = &[
    (41, 128, 185), // Blue (path 0)
    (39, 174, 96),  // Green (path 1)
    (230, 126, 34), // Orange (path 2)
    (142, 68, 173), // Purple (path 3)
    (192, 57, 43),  // Red (path 4)
    (22, 160, 133), // Teal (path 5)
];

fn path_color(path_id: u64) -> Color32 {
    if let Some(&(r, g, b)) = PATH_COLORS.get(path_id as usize) {
        Color32::from_rgb(r, g, b)
    } else {
        // Golden ratio hashing for path_id > 5
        let hue = ((path_id as f32 * 137.508) % 360.0) / 360.0;
        hsv_to_rgb(hue, 0.7, 0.8)
    }
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Color32 {
    let c = v * s;
    let x = c * (1.0 - ((h * 6.0) % 2.0 - 1.0).abs());
    let m = v - c;

    let (r, g, b) = if h < 1.0 / 6.0 {
        (c, x, 0.0)
    } else if h < 2.0 / 6.0 {
        (x, c, 0.0)
    } else if h < 3.0 / 6.0 {
        (0.0, c, x)
    } else if h < 4.0 / 6.0 {
        (0.0, x, c)
    } else if h < 5.0 / 6.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    Color32::from_rgb(
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

pub struct SequenceDiagram {
    selected_packet_idx: Option<usize>,
    cached_packets: Vec<PacketInfo>,
    cache_valid: bool,
    show_loss_markers: bool,
    dual_time_scale: f32,
    files_swapped: bool,
    compress_gaps: bool,
    pub visualization_mode: VisualizationMode,
}

const GAP_THRESHOLD_MS: f64 = 5.0;
const COMPRESSED_GAP_HEIGHT: f32 = 30.0;

#[derive(Clone)]
struct TimeGap {
    start_time: f64,
    end_time: f64,
    compressed_amount: f64,
}

struct DualArrow {
    send_time: f64,
    recv_time: f64,
    from_left: bool,
    packet_type_short: String,
    packet_number: u64,
    path_id: Option<u64>,
    frames: Vec<FrameType>,
    is_lost: bool,
    sent_event_idx: usize,         // Index in the sending file
    recv_event_idx: Option<usize>, // Index in the receiving file (None if not received)
}

struct DiagramLayout {
    left_x: f32,
    right_x: f32,
    top_y: f32,
    rect: Rect,
}

struct PathLaneLayout {
    left_lanes: HashMap<u64, f32>,  // path_id -> x position
    right_lanes: HashMap<u64, f32>, // path_id -> x position
    #[allow(dead_code)]
    center_x: f32,
    use_lanes: bool, // false if too many paths (>4), falls back to color mode
}

struct TimeContext<'a> {
    min_time: f64,
    max_time: f64,
    gaps: &'a [TimeGap],
    pixels_per_ms: f32,
}

impl SequenceDiagram {
    pub fn new() -> Self {
        Self {
            selected_packet_idx: None,
            cached_packets: Vec::new(),
            cache_valid: false,
            show_loss_markers: true,
            dual_time_scale: 30.0,
            files_swapped: false,
            compress_gaps: true,
            visualization_mode: VisualizationMode::ColorCoded,
        }
    }

    pub fn invalidate_cache(&mut self) {
        self.cache_valid = false;
        self.cached_packets.clear();
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        qlog_data: &QlogData,
        _correlation: &PacketCorrelation,
        selected_event_idx: &mut Option<usize>,
    ) {
        self.ensure_cache(qlog_data);

        if self.cached_packets.is_empty() {
            ui.label("No packet events found in qlog");
            return;
        }

        ui.horizontal(|ui| ui.heading("Sequence Diagram"));

        // Convert all packets to dual arrows (with implied second endpoint)
        let arrows = Self::build_single_file_arrows(
            &self.cached_packets,
            &(0..self.cached_packets.len()).collect::<Vec<_>>(),
        );

        if arrows.is_empty() {
            ui.label("No packet events found");
            return;
        }

        let (min_time, _max_time, time_range) = Self::compute_time_bounds(&arrows);
        let gaps = if self.compress_gaps {
            Self::detect_gaps(&arrows, min_time, self.dual_time_scale)
        } else {
            Vec::new()
        };

        self.render_dual_controls(ui, &arrows, time_range);

        // Use the same dual rendering for single file (with dummy recv_selected_event_idx)
        let mut recv_selected_event_idx = None;
        let mut selected_file_idx = 0;
        self.render_dual_file_diagram(
            ui,
            &arrows,
            min_time,
            time_range,
            &gaps,
            selected_event_idx,
            &mut recv_selected_event_idx,
            &mut selected_file_idx,
            "Client",
            "Server",
        );
    }

    pub fn show_dual(
        &mut self,
        ui: &mut egui::Ui,
        file_a: &LoadedFile,
        file_b: &LoadedFile,
        selected_event_idx: &mut Option<usize>,
        recv_selected_event_idx: &mut Option<usize>,
        selected_file_idx: &mut usize,
    ) {
        let (left_file, right_file) = if self.files_swapped {
            (file_b, file_a)
        } else {
            (file_a, file_b)
        };

        let arrows = Self::build_dual_arrows(left_file, right_file);

        if arrows.is_empty() {
            ui.label("No packet events found in either file");
            return;
        }

        ui.horizontal(|ui| ui.heading("Sequence Diagram (Dual File)"));

        let (min_time, _max_time, time_range) = Self::compute_time_bounds(&arrows);
        let gaps = if self.compress_gaps {
            Self::detect_gaps(&arrows, min_time, self.dual_time_scale)
        } else {
            Vec::new()
        };

        self.render_dual_controls(ui, &arrows, time_range);

        self.render_dual_file_diagram(
            ui,
            &arrows,
            min_time,
            time_range,
            &gaps,
            selected_event_idx,
            recv_selected_event_idx,
            selected_file_idx,
            &left_file.label,
            &right_file.label,
        );
    }

    fn ensure_cache(&mut self, qlog_data: &QlogData) {
        if self.cache_valid {
            return;
        }
        let extracted = Self::extract_packets(qlog_data);
        self.cached_packets = extracted.packets;
        self.cache_valid = true;
    }

    fn calculate_lane_layout(
        &self,
        rect: &Rect,
        left_path_ids: &[u64],
        right_path_ids: &[u64],
    ) -> PathLaneLayout {
        let max_paths = left_path_ids.len().max(right_path_ids.len());

        // Fallback to color mode if >4 paths per side
        if max_paths > 4 {
            return PathLaneLayout {
                left_lanes: HashMap::new(),
                right_lanes: HashMap::new(),
                center_x: rect.center().x,
                use_lanes: false,
            };
        }

        let available_width = rect.width();
        let center_x = rect.center().x;

        // Allocate 35% of width to each side, 30% in middle for arrows
        let left_width = available_width * 0.35;
        let right_width = available_width * 0.35;

        let left_start = rect.left() + 50.0; // Some margin from edge
        let right_end = rect.right() - 50.0;

        let mut left_lanes = HashMap::new();
        let mut right_lanes = HashMap::new();

        // Calculate left lane positions
        if !left_path_ids.is_empty() {
            let lane_spacing = left_width / (left_path_ids.len() as f32 + 1.0);
            for (i, &path_id) in left_path_ids.iter().enumerate() {
                let x = left_start + lane_spacing * (i + 1) as f32;
                left_lanes.insert(path_id, x);
            }
        }

        // Calculate right lane positions
        if !right_path_ids.is_empty() {
            let lane_spacing = right_width / (right_path_ids.len() as f32 + 1.0);
            for (i, &path_id) in right_path_ids.iter().enumerate() {
                let x = right_end - right_width + lane_spacing * (i + 1) as f32;
                right_lanes.insert(path_id, x);
            }
        }

        PathLaneLayout {
            left_lanes,
            right_lanes,
            center_x,
            use_lanes: true,
        }
    }

    fn build_single_file_arrows(
        packets: &[PacketInfo],
        filtered_indices: &[usize],
    ) -> Vec<DualArrow> {
        // Convert single-file packets to dual arrows
        // Sent packets go left->right with implied receiver
        // Received packets go right->left with implied sender
        filtered_indices
            .iter()
            .filter_map(|&idx| packets.get(idx))
            .map(|p| {
                let (from_left, recv_time, recv_event_idx) = match p.direction {
                    PacketDirection::Sent => {
                        // Sent: left->right, receiver is implied (no recv event)
                        (true, p.time + 1.0, None) // Add 1ms for visual arrow length
                    }
                    PacketDirection::Received => {
                        // Received: right->left, this IS the receive event
                        // Sender is implied, so we show it as sent 1ms earlier
                        (false, p.time, Some(p.event_idx))
                    }
                };

                DualArrow {
                    send_time: if from_left { p.time } else { p.time - 1.0 },
                    recv_time,
                    from_left,
                    packet_type_short: p.packet_type_short.clone(),
                    packet_number: p.packet_number,
                    path_id: p.path_id,
                    frames: p.frames.clone(),
                    is_lost: false, // In single-file, we don't know if it's truly lost
                    sent_event_idx: p.event_idx,
                    recv_event_idx,
                }
            })
            .collect()
    }

    fn build_dual_arrows(left_file: &LoadedFile, right_file: &LoadedFile) -> Vec<DualArrow> {
        let left_packets = Self::extract_packets(&left_file.qlog_data).packets;
        let right_packets = Self::extract_packets(&right_file.qlog_data).packets;

        let recv_info = |packets: &[PacketInfo]| -> HashMap<(String, u64, u64), (f64, usize)> {
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

        let left_recv = recv_info(&left_packets);
        let right_recv = recv_info(&right_packets);

        let mut arrows: Vec<DualArrow> = Vec::new();

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
                    send_time: p.time,
                    recv_time,
                    from_left,
                    packet_type_short: p.packet_type_short.clone(),
                    packet_number: p.packet_number,
                    path_id: p.path_id,
                    frames: p.frames.clone(),
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

    fn compute_time_bounds(arrows: &[DualArrow]) -> (f64, f64, f64) {
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

    fn detect_gaps(arrows: &[DualArrow], min_time: f64, pixels_per_ms: f32) -> Vec<TimeGap> {
        if arrows.is_empty() {
            return Vec::new();
        }

        let mut event_times: Vec<f64> = arrows
            .iter()
            .flat_map(|a| [a.send_time, a.recv_time])
            .collect();
        event_times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        event_times.dedup();

        // Minimum visual height threshold for compression (in pixels)
        // Only compress if the gap would be taller than this
        const MIN_VISUAL_HEIGHT_PX: f32 = 100.0;
        let min_gap_ms = (MIN_VISUAL_HEIGHT_PX / pixels_per_ms) as f64;

        let mut gaps = Vec::new();
        let mut prev_time = min_time;

        for &t in &event_times {
            let gap_size = t - prev_time;

            // Only consider gaps that would be visually significant
            if gap_size > min_gap_ms {
                // Check if there are any arrows in flight during this gap
                let has_in_flight = arrows.iter().any(|arrow| {
                    let arrow_start = arrow.send_time.min(arrow.recv_time);
                    let arrow_end = arrow.send_time.max(arrow.recv_time);
                    // Arrow overlaps with gap if it starts before gap ends AND ends after gap starts
                    arrow_start < t && arrow_end > prev_time
                });

                // Only compress if no arrows in flight
                if !has_in_flight {
                    // Leave a small amount uncompressed on each side for visual continuity
                    let keep_visible = GAP_THRESHOLD_MS.min(gap_size * 0.1);
                    let compressed = gap_size - (keep_visible * 2.0);

                    if compressed > 0.0 {
                        gaps.push(TimeGap {
                            start_time: prev_time + keep_visible,
                            end_time: t - keep_visible,
                            compressed_amount: compressed,
                        });
                    }
                }
            }
            prev_time = t;
        }

        gaps
    }

    fn time_to_compressed_y(
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

    fn compute_compressed_height(time_range: f64, gaps: &[TimeGap], pixels_per_ms: f32) -> f32 {
        let total_compression: f64 = gaps.iter().map(|g| g.compressed_amount).sum();
        let compressed_time = time_range - total_compression;
        (compressed_time as f32 * pixels_per_ms)
            + (gaps.len() as f32 * COMPRESSED_GAP_HEIGHT)
            + 100.0
    }

    fn render_dual_controls(&mut self, ui: &mut egui::Ui, arrows: &[DualArrow], time_range: f64) {
        let total_loss_count = arrows.iter().filter(|a| a.is_lost).count();

        ui.horizontal(|ui| {
            ui.label(format!(
                "{} arrows | 0.00 - {:.2} ms ({:.1}s)",
                arrows.len(),
                time_range,
                time_range / 1000.0
            ));

            ui.separator();
            ui.checkbox(&mut self.show_loss_markers, "Show Lost");
            if self.show_loss_markers && total_loss_count > 0 {
                ui.label(format!("({} lost)", total_loss_count));
            }

            ui.separator();
            ui.label("Time Scale:");
            ui.add_sized(
                [350.0, 20.0],
                egui::Slider::new(&mut self.dual_time_scale, 0.5..=2500.0)
                    .logarithmic(true)
                    .suffix(" px/ms")
                    .min_decimals(1)
                    .max_decimals(1),
            );

            ui.separator();
            ui.checkbox(&mut self.compress_gaps, "Compress Gaps");

            ui.separator();
            if ui.button("Swap").clicked() {
                self.files_swapped = !self.files_swapped;
            }
        });
        ui.separator();
    }

    #[allow(clippy::too_many_arguments)]
    fn render_dual_file_diagram(
        &mut self,
        ui: &mut egui::Ui,
        arrows: &[DualArrow],
        min_time: f64,
        time_range: f64,
        gaps: &[TimeGap],
        selected_event_idx: &mut Option<usize>,
        recv_selected_event_idx: &mut Option<usize>,
        selected_file_idx: &mut usize,
        left_label: &str,
        right_label: &str,
    ) {
        let pixels_per_ms = self.dual_time_scale;
        let total_content_height = if gaps.is_empty() {
            (time_range as f32 * pixels_per_ms) + 100.0
        } else {
            Self::compute_compressed_height(time_range, gaps, pixels_per_ms)
        };

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                let available_width = ui.available_width();
                let desired_size = Vec2::new(available_width, total_content_height);
                let (response, painter) =
                    ui.allocate_painter(desired_size, Sense::click_and_drag());
                let rect = response.rect;

                let layout = DiagramLayout {
                    left_x: rect.left() + DUAL_LEFT_MARGIN,
                    right_x: rect.right() - DUAL_RIGHT_MARGIN,
                    top_y: rect.top() + HEADER_HEIGHT,
                    rect,
                };

                let time_to_y = |t: f64| -> f32 {
                    if gaps.is_empty() {
                        layout.top_y + ((t - min_time) as f32 * pixels_per_ms)
                    } else {
                        Self::time_to_compressed_y(t, min_time, gaps, pixels_per_ms, layout.top_y)
                    }
                };

                // Calculate lane layout if in lane mode
                let lane_layout = if self.visualization_mode == VisualizationMode::VerticalLanes {
                    // Collect unique path IDs from arrows by direction
                    let mut left_path_ids = BTreeSet::new();
                    let mut right_path_ids = BTreeSet::new();
                    for arrow in arrows.iter() {
                        if let Some(pid) = arrow.path_id {
                            if arrow.from_left {
                                left_path_ids.insert(pid);
                            } else {
                                right_path_ids.insert(pid);
                            }
                        }
                    }
                    let left_vec: Vec<u64> = left_path_ids.into_iter().collect();
                    let right_vec: Vec<u64> = right_path_ids.into_iter().collect();
                    Some(self.calculate_lane_layout(&rect, &left_vec, &right_vec))
                } else {
                    None
                };

                draw_header(
                    &painter,
                    &layout,
                    viewport,
                    available_width,
                    ui.style().visuals.panel_fill,
                    left_label,
                    right_label,
                    SENT_COLOR,
                    RECEIVED_COLOR,
                );

                // Draw timelines (need to draw for each lane if in lane mode)
                if let Some(ref lanes) = lane_layout {
                    if lanes.use_lanes {
                        draw_lane_timelines(
                            &painter,
                            &layout,
                            layout.top_y,
                            layout.top_y + total_content_height,
                            lanes,
                        );
                    } else {
                        draw_timelines(
                            &painter,
                            &layout,
                            layout.top_y,
                            layout.top_y + total_content_height,
                        );
                    }
                } else {
                    draw_timelines(
                        &painter,
                        &layout,
                        layout.top_y,
                        layout.top_y + total_content_height,
                    );
                }

                let time_ctx = TimeContext {
                    min_time,
                    max_time: min_time + time_range,
                    gaps,
                    pixels_per_ms,
                };
                draw_time_markers(
                    &painter,
                    &layout,
                    &viewport,
                    total_content_height,
                    &time_ctx,
                );

                if !gaps.is_empty() {
                    draw_gap_indicators(&painter, &layout, gaps, &time_to_y);
                }

                let send_time_counts = Self::count_simultaneous_sends(arrows);
                let arrow_rects = self.draw_dual_arrows(
                    &painter,
                    &layout,
                    arrows,
                    &send_time_counts,
                    &time_to_y,
                    lane_layout.as_ref(),
                );

                self.handle_dual_selection(
                    ui,
                    &response,
                    &arrow_rects,
                    selected_event_idx,
                    recv_selected_event_idx,
                    selected_file_idx,
                );

                self.draw_selection_indicator(&painter, &layout, viewport, arrow_rects.len());

                // Collect unique path_ids for legend
                let mut path_ids: Vec<u64> = arrows.iter().filter_map(|a| a.path_id).collect();
                path_ids.sort_unstable();
                path_ids.dedup();

                draw_path_legend(&painter, layout.rect, viewport, &path_ids);
            });
    }

    fn count_simultaneous_sends(arrows: &[DualArrow]) -> HashMap<i64, usize> {
        let mut counts: HashMap<i64, usize> = HashMap::new();
        for arrow in arrows {
            let key = (arrow.send_time * 1000.0) as i64;
            *counts.entry(key).or_insert(0) += 1;
        }
        counts
    }

    fn draw_dual_arrows(
        &self,
        painter: &Painter,
        layout: &DiagramLayout,
        arrows: &[DualArrow],
        send_time_counts: &HashMap<i64, usize>,
        time_to_y: &impl Fn(f64) -> f32,
        lane_layout: Option<&PathLaneLayout>,
    ) -> Vec<(usize, usize, Option<usize>, bool, Rect)> {
        let mut arrow_rects = Vec::new();
        let mut send_time_indices: HashMap<i64, usize> = HashMap::new();

        for (arrow_idx, arrow) in arrows.iter().enumerate() {
            let y_send = time_to_y(arrow.send_time);
            let y_recv = time_to_y(arrow.recv_time);

            // Determine start and end positions based on lane mode
            let (start_x, end_x) = if let Some(lanes) = lane_layout {
                if lanes.use_lanes {
                    // Use lane positions
                    let path_id = arrow.path_id.unwrap_or(0);
                    if arrow.from_left {
                        let start = lanes
                            .left_lanes
                            .get(&path_id)
                            .copied()
                            .unwrap_or(layout.left_x);
                        let end = lanes
                            .right_lanes
                            .get(&path_id)
                            .copied()
                            .unwrap_or(layout.right_x);
                        (start, end)
                    } else {
                        let start = lanes
                            .right_lanes
                            .get(&path_id)
                            .copied()
                            .unwrap_or(layout.right_x);
                        let end = lanes
                            .left_lanes
                            .get(&path_id)
                            .copied()
                            .unwrap_or(layout.left_x);
                        (start, end)
                    }
                } else {
                    // Fallback to color mode
                    if arrow.from_left {
                        (layout.left_x, layout.right_x)
                    } else {
                        (layout.right_x, layout.left_x)
                    }
                }
            } else {
                // Default positioning (color mode)
                if arrow.from_left {
                    (layout.left_x, layout.right_x)
                } else {
                    (layout.right_x, layout.left_x)
                }
            };

            // Use path color for multipath visualization
            let color = path_color(arrow.path_id.unwrap_or(0));

            let time_key = (arrow.send_time * 1000.0) as i64;
            let count = *send_time_counts.get(&time_key).unwrap_or(&1);
            let idx = *send_time_indices.entry(time_key).or_insert(0);
            *send_time_indices.get_mut(&time_key).unwrap() += 1;

            let fan_offset = if count > 1 {
                (idx as f32 - (count as f32 - 1.0) / 2.0) * 3.0
            } else {
                0.0
            };
            let y_send_adjusted = y_send + fan_offset;

            let (actual_end_x, actual_end_y, is_truncated) =
                if self.show_loss_markers && arrow.is_lost {
                    let mid_x = (start_x + end_x) / 2.0;
                    let mid_y = (y_send_adjusted + y_recv) / 2.0;
                    (mid_x, mid_y, true)
                } else {
                    (end_x, y_recv, false)
                };

            let is_selected = self.selected_packet_idx == Some(arrow_idx);
            let line_color = if is_selected { Color32::YELLOW } else { color };
            let line_width = if is_selected { 3.0 } else { 1.5 };
            let marker_size = if is_selected { 5.0 } else { 3.0 };

            painter.line_segment(
                [
                    Pos2::new(start_x, y_send_adjusted),
                    Pos2::new(actual_end_x, actual_end_y),
                ],
                Stroke::new(line_width, line_color),
            );

            let tick_half = 4.0;
            painter.line_segment(
                [
                    Pos2::new(start_x - tick_half, y_send_adjusted),
                    Pos2::new(start_x + tick_half, y_send_adjusted),
                ],
                Stroke::new(2.0, line_color),
            );

            painter.circle_filled(Pos2::new(start_x, y_send_adjusted), marker_size, line_color);
            if !is_truncated {
                painter.line_segment(
                    [
                        Pos2::new(actual_end_x - tick_half, actual_end_y),
                        Pos2::new(actual_end_x + tick_half, actual_end_y),
                    ],
                    Stroke::new(2.0, line_color),
                );

                draw_arrowhead(
                    painter,
                    Pos2::new(start_x, y_send_adjusted),
                    Pos2::new(actual_end_x, actual_end_y),
                    line_color,
                    12.0,
                );
            }

            let tag_x = (start_x + actual_end_x) / 2.0;
            let tag_y = (y_send_adjusted + actual_end_y) / 2.0;

            draw_frame_tags(painter, &arrow.frames, tag_x, tag_y, 32.0, 3);

            let info_offset = if arrow.from_left { 5.0 } else { -5.0 };
            let label = if let Some(pid) = arrow.path_id {
                format!(
                    "{}:{} p{}",
                    arrow.packet_type_short, arrow.packet_number, pid
                )
            } else {
                format!("{}:{}", arrow.packet_type_short, arrow.packet_number)
            };
            painter.text(
                Pos2::new(tag_x + info_offset, tag_y - 10.0),
                if arrow.from_left {
                    egui::Align2::LEFT_BOTTOM
                } else {
                    egui::Align2::RIGHT_BOTTOM
                },
                label,
                egui::FontId::proportional(8.0),
                Color32::LIGHT_GRAY,
            );

            if is_truncated {
                draw_loss_marker(painter, Pos2::new(actual_end_x, actual_end_y), 10.0);
            }

            if count > 1 && idx == 0 {
                let indicator_x = start_x + (if arrow.from_left { 8.0 } else { -8.0 });
                painter.text(
                    Pos2::new(indicator_x, y_send),
                    if arrow.from_left {
                        egui::Align2::LEFT_CENTER
                    } else {
                        egui::Align2::RIGHT_CENTER
                    },
                    format!("x{}", count),
                    egui::FontId::proportional(9.0),
                    Color32::YELLOW,
                );
            }

            let arrow_rect = Rect::from_two_pos(
                Pos2::new(
                    start_x.min(actual_end_x) - 5.0,
                    y_send_adjusted.min(actual_end_y) - 12.0,
                ),
                Pos2::new(
                    start_x.max(actual_end_x) + 5.0,
                    y_send_adjusted.max(actual_end_y) + 12.0,
                ),
            );
            arrow_rects.push((
                arrow_idx,
                arrow.sent_event_idx,
                arrow.recv_event_idx,
                arrow.from_left,
                arrow_rect,
            ));
        }

        arrow_rects
    }

    fn handle_dual_selection(
        &mut self,
        ui: &egui::Ui,
        response: &Response,
        arrow_rects: &[(usize, usize, Option<usize>, bool, Rect)],
        selected_event_idx: &mut Option<usize>,
        recv_selected_event_idx: &mut Option<usize>,
        selected_file_idx: &mut usize,
    ) {
        let total_arrows = arrow_rects.len();

        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                if let Some((idx, sent_event_idx, recv_event_idx, from_left, _)) = arrow_rects
                    .iter()
                    .find(|(_, _, _, _, rect)| rect.contains(pos))
                {
                    self.selected_packet_idx = Some(*idx);
                    *selected_event_idx = Some(*sent_event_idx);
                    *recv_selected_event_idx = *recv_event_idx;
                    *selected_file_idx = if *from_left { 0 } else { 1 };
                }
            }
        }

        if total_arrows == 0 {
            return;
        }

        let (up, down) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
            )
        });

        if up || down {
            let current_idx = self.selected_packet_idx.unwrap_or(0);
            let new_idx = if up {
                current_idx.saturating_sub(1)
            } else {
                (current_idx + 1).min(total_arrows - 1)
            };

            if new_idx != current_idx || self.selected_packet_idx.is_none() {
                self.selected_packet_idx = Some(new_idx);
                if let Some((_, sent_event_idx, recv_event_idx, from_left, _)) =
                    arrow_rects.iter().find(|(idx, _, _, _, _)| *idx == new_idx)
                {
                    *selected_event_idx = Some(*sent_event_idx);
                    *recv_selected_event_idx = *recv_event_idx;
                    *selected_file_idx = if *from_left { 0 } else { 1 };
                }
            }
        }
    }

    fn draw_selection_indicator(
        &self,
        painter: &Painter,
        layout: &DiagramLayout,
        viewport: Rect,
        total_arrows: usize,
    ) {
        if let Some(idx) = self.selected_packet_idx {
            painter.text(
                Pos2::new(
                    layout.rect.left() + 10.0,
                    layout.rect.top() + viewport.top() + 25.0,
                ),
                egui::Align2::LEFT_CENTER,
                format!("arrow {} of {}", idx + 1, total_arrows),
                egui::FontId::proportional(11.0),
                Color32::YELLOW,
            );
        }
    }

    /// Extract path_id from event - handles both multipath and regular formats
    fn extract_path_id(event: &qlog::events::Event, header_path_id: Option<u64>) -> Option<u64> {
        // Priority 1: header.path_id (multipath format, u64)
        if let Some(pid) = header_path_id {
            return Some(pid);
        }

        // Priority 2: event.ex_data["path_id"] (regular format, string or number)
        if let Some(value) = event.ex_data.get("path_id") {
            // Try as string first (common format: "0", "1", etc.)
            if let Some(s) = value.as_str() {
                if let Ok(pid) = s.parse::<u64>() {
                    return Some(pid);
                }
            }
            // Try as number
            if let Some(n) = value.as_u64() {
                return Some(n);
            }
        }

        // Default: treat as path 0
        Some(0)
    }

    fn extract_packets(qlog_data: &QlogData) -> ExtractedPackets {
        let mut result = ExtractedPackets::default();

        for (idx, event) in qlog_data.events.iter().enumerate() {
            let (header, direction, frames_iter): (_, _, Box<dyn Iterator<Item = _>>) =
                match &event.data {
                    EventData::PacketSent(d) => (
                        &d.header,
                        PacketDirection::Sent,
                        Box::new(d.frames.iter().flatten()) as Box<dyn Iterator<Item = _>>,
                    ),
                    EventData::PacketReceived(d) => (
                        &d.header,
                        PacketDirection::Received,
                        Box::new(d.frames.iter().flatten()) as Box<dyn Iterator<Item = _>>,
                    ),
                    _ => continue,
                };

            let packet_type_raw = format!("{:?}", header.packet_type);
            let packet_type = utils::full_packet_type(&packet_type_raw);
            let packet_type_short = utils::short_packet_type(&packet_type_raw);

            let frames: Vec<FrameType> = frames_iter
                .map(|frame| {
                    if let Some(sid) = utils::get_frame_stream_id(frame) {
                        result.stream_ids.insert(sid);
                    }
                    FrameType::from_quic_frame(frame)
                })
                .collect();

            result.packet_types.insert(packet_type.clone());

            // Unified path_id extraction from both formats
            let path_id = Self::extract_path_id(event, header.path_id);

            result.packets.push(PacketInfo {
                time: event.time as f64,
                direction,
                packet_type,
                packet_type_short,
                packet_number: header.packet_number.unwrap_or(0),
                path_id,
                frames,
                event_idx: idx,
            });
        }

        result
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_header(
    painter: &Painter,
    layout: &DiagramLayout,
    viewport: Rect,
    width: f32,
    fill: Color32,
    left_label: &str,
    right_label: &str,
    left_color: Color32,
    right_color: Color32,
) {
    let header_y = layout.rect.top() + viewport.top() + 18.0;
    painter.rect_filled(
        Rect::from_min_size(
            Pos2::new(layout.rect.left(), layout.rect.top() + viewport.top()),
            Vec2::new(width, 40.0),
        ),
        0.0,
        fill,
    );
    painter.text(
        Pos2::new(layout.left_x, header_y),
        egui::Align2::CENTER_CENTER,
        left_label,
        egui::FontId::proportional(14.0),
        left_color,
    );
    painter.text(
        Pos2::new(layout.right_x, header_y),
        egui::Align2::CENTER_CENTER,
        right_label,
        egui::FontId::proportional(14.0),
        right_color,
    );
}

fn draw_timelines(painter: &Painter, layout: &DiagramLayout, top: f32, bottom: f32) {
    for x in [layout.left_x, layout.right_x] {
        painter.line_segment(
            [Pos2::new(x, top), Pos2::new(x, bottom)],
            Stroke::new(2.0, Color32::GRAY),
        );
    }
}

fn draw_lane_timelines(
    painter: &Painter,
    _layout: &DiagramLayout,
    top: f32,
    bottom: f32,
    lanes: &PathLaneLayout,
) {
    // Draw timelines and labels for each lane
    for (&path_id, &x) in lanes.left_lanes.iter() {
        painter.line_segment(
            [Pos2::new(x, top), Pos2::new(x, bottom)],
            Stroke::new(1.5, Color32::from_rgb(100, 100, 100)),
        );
        // Draw path label at top
        painter.text(
            Pos2::new(x, top - 15.0),
            egui::Align2::CENTER_BOTTOM,
            format!("Path {}", path_id),
            egui::FontId::proportional(10.0),
            Color32::LIGHT_GRAY,
        );
    }
    for (&path_id, &x) in lanes.right_lanes.iter() {
        painter.line_segment(
            [Pos2::new(x, top), Pos2::new(x, bottom)],
            Stroke::new(1.5, Color32::from_rgb(100, 100, 100)),
        );
        // Draw path label at top
        painter.text(
            Pos2::new(x, top - 15.0),
            egui::Align2::CENTER_BOTTOM,
            format!("Path {}", path_id),
            egui::FontId::proportional(10.0),
            Color32::LIGHT_GRAY,
        );
    }
}

fn draw_gap_indicators<F>(
    painter: &Painter,
    layout: &DiagramLayout,
    gaps: &[TimeGap],
    time_to_y: &F,
) where
    F: Fn(f64) -> f32,
{
    let gap_color = Color32::from_rgb(180, 160, 80);
    let text_color = Color32::from_rgb(150, 150, 150);

    for gap in gaps {
        let y_pos = time_to_y(gap.start_time);
        let end_y = y_pos + COMPRESSED_GAP_HEIGHT;
        let center_y = y_pos + COMPRESSED_GAP_HEIGHT / 2.0;

        // Draw a subtle background rect spanning between the two timelines
        let bg_rect = Rect::from_min_max(
            Pos2::new(layout.left_x, y_pos),
            Pos2::new(layout.right_x, end_y),
        );
        painter.rect_filled(
            bg_rect,
            0.0,
            Color32::from_rgba_unmultiplied(180, 160, 80, 20),
        );

        // Draw highlighted sections on the timelines
        for x in [layout.left_x, layout.right_x] {
            painter.line_segment(
                [Pos2::new(x, y_pos), Pos2::new(x, end_y)],
                Stroke::new(6.0, gap_color),
            );
        }

        // Draw "compressed" label in the center
        let center_x = (layout.left_x + layout.right_x) / 2.0;
        let label = format!("compressed {:.1}ms", gap.compressed_amount);
        painter.text(
            Pos2::new(center_x, center_y),
            egui::Align2::CENTER_CENTER,
            &label,
            egui::FontId::proportional(12.0),
            text_color,
        );

        // Draw time range in right margin
        let right_text_x = layout.right_x + GAP_INDICATOR_MARGIN;
        let start_label = format!("{:.1}ms", gap.start_time);
        let end_label = format!("{:.1}ms", gap.end_time);
        painter.text(
            Pos2::new(right_text_x, y_pos + 5.0),
            egui::Align2::LEFT_TOP,
            &start_label,
            egui::FontId::proportional(9.0),
            Color32::from_rgb(120, 120, 120),
        );
        painter.text(
            Pos2::new(right_text_x, end_y - 5.0),
            egui::Align2::LEFT_BOTTOM,
            &end_label,
            egui::FontId::proportional(9.0),
            Color32::from_rgb(120, 120, 120),
        );
    }
}

fn draw_frame_tags(
    painter: &Painter,
    frames: &[FrameType],
    mid_x: f32,
    mid_y: f32,
    tag_width: f32,
    max_tags: usize,
) {
    let frames_to_show: Vec<&FrameType> = frames.iter().take(max_tags).collect();
    let tag_gap = 3.0;
    let tag_height = 16.0;
    let total_width = frames_to_show.len() as f32 * (tag_width + tag_gap);
    let start_x = mid_x - total_width / 2.0;

    for (i, frame) in frames_to_show.iter().enumerate() {
        let tag_x = start_x + (i as f32 * (tag_width + tag_gap));
        let tag_rect = Rect::from_min_size(
            Pos2::new(tag_x, mid_y - tag_height / 2.0),
            Vec2::new(tag_width, tag_height),
        );
        painter.rect_filled(tag_rect, 2.0, frame.color());
        painter.text(
            tag_rect.center(),
            egui::Align2::CENTER_CENTER,
            frame.short_name(),
            egui::FontId::proportional(11.0),
            Color32::WHITE,
        );
    }
}

fn draw_time_markers(
    painter: &Painter,
    layout: &DiagramLayout,
    viewport: &Rect,
    total_height: f32,
    time_ctx: &TimeContext,
) {
    let target_markers = 6;
    let step_pixels = viewport.height() / target_markers as f32;
    let marker_color = Color32::from_rgb(140, 140, 140);
    let max_markers = (total_height / step_pixels).ceil() as usize + 1;

    for i in 0..max_markers {
        let visual_offset = i as f32 * step_pixels;
        let y = layout.top_y + visual_offset;

        let mut time = time_ctx.min_time + (visual_offset / time_ctx.pixels_per_ms) as f64;

        let mut compression_before = 0.0;
        for gap in time_ctx.gaps {
            let gap_visual_start = (gap.start_time - time_ctx.min_time - compression_before) as f32
                * time_ctx.pixels_per_ms
                + (time_ctx
                    .gaps
                    .iter()
                    .take_while(|g| g.end_time <= gap.start_time)
                    .count() as f32
                    * COMPRESSED_GAP_HEIGHT);

            if visual_offset > gap_visual_start {
                time += gap.compressed_amount;
            }
            compression_before += gap.compressed_amount;
        }

        time = time.clamp(time_ctx.min_time, time_ctx.max_time);

        let decimals = if time < 1.0 {
            2
        } else if time < 100.0 {
            1
        } else {
            0
        };
        let label = format!("{:.prec$}", time, prec = decimals);

        painter.text(
            Pos2::new(layout.left_x - 15.0, y),
            egui::Align2::RIGHT_CENTER,
            &label,
            egui::FontId::proportional(10.0),
            marker_color,
        );

        painter.text(
            Pos2::new(layout.right_x + DUAL_RIGHT_MARGIN / 2.0, y),
            egui::Align2::CENTER_CENTER,
            &label,
            egui::FontId::proportional(10.0),
            marker_color,
        );
    }
}

fn draw_arrowhead(painter: &Painter, from: Pos2, to: Pos2, color: Color32, size: f32) {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return;
    }

    let ux = dx / len;
    let uy = dy / len;

    let px = -uy;
    let py = ux;

    let tip = to;
    let left = Pos2::new(
        tip.x - ux * size + px * size * 0.6,
        tip.y - uy * size + py * size * 0.6,
    );
    let right = Pos2::new(
        tip.x - ux * size - px * size * 0.6,
        tip.y - uy * size - py * size * 0.6,
    );

    painter.add(egui::Shape::convex_polygon(
        vec![tip, left, right],
        color,
        Stroke::NONE,
    ));
}

fn draw_loss_marker(painter: &Painter, center: Pos2, size: f32) {
    let box_rect = Rect::from_center_size(center, Vec2::new(size * 1.5, size));

    painter.rect_filled(
        box_rect,
        2.0,
        Color32::from_rgba_unmultiplied(255, 0, 0, 80),
    );

    let hatch_stroke = Stroke::new(1.0, Color32::from_rgb(180, 50, 50));
    let step = 3.0;
    let (left, right, top, bottom) = (
        box_rect.left(),
        box_rect.right(),
        box_rect.top(),
        box_rect.bottom(),
    );

    let mut x = left;
    while x < right + (bottom - top) {
        let x1 = x.max(left);
        let y1 = (top + (x1 - x)).clamp(top, bottom);
        let x2 = (x + (bottom - top)).min(right);
        let y2 = (bottom - (right - x2).max(0.0)).clamp(top, bottom);

        if x1 < right && x2 > left {
            painter.line_segment([Pos2::new(x1, y1), Pos2::new(x2, y2)], hatch_stroke);
        }
        x += step;
    }

    painter.rect_stroke(
        box_rect,
        2.0,
        Stroke::new(1.5, LOSS_COLOR),
        StrokeKind::Inside,
    );

    let x_size = size * 0.4;
    for (dx, dy) in [(1.0, 1.0), (1.0, -1.0)] {
        painter.line_segment(
            [
                Pos2::new(center.x - x_size, center.y - x_size * dy),
                Pos2::new(center.x + x_size, center.y + x_size * dy * dx),
            ],
            Stroke::new(2.0, LOSS_COLOR),
        );
    }
}

#[derive(Default)]
struct ExtractedPackets {
    packets: Vec<PacketInfo>,
    stream_ids: BTreeSet<u64>,
    packet_types: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum PacketDirection {
    Sent,
    Received,
}

fn draw_path_legend(painter: &Painter, rect: Rect, viewport: Rect, path_ids: &[u64]) {
    if path_ids.is_empty() || path_ids.len() == 1 {
        // Don't show legend for single path
        return;
    }

    let legend_width = 100.0;
    let legend_height = (path_ids.len() as f32 * 22.0) + 12.0;
    let margin = 10.0;

    // Position in top-right corner relative to viewport
    let legend_x = rect.right() + viewport.right() - legend_width - margin;
    let legend_y = rect.top() + viewport.top() + margin;

    let legend_rect = Rect::from_min_size(
        Pos2::new(legend_x, legend_y),
        Vec2::new(legend_width, legend_height),
    );

    // Background
    painter.rect_filled(
        legend_rect,
        4.0,
        Color32::from_rgba_unmultiplied(40, 40, 45, 230),
    );
    painter.rect_stroke(
        legend_rect,
        4.0,
        Stroke::new(1.0, Color32::from_rgb(80, 80, 85)),
        StrokeKind::Inside,
    );

    // Title
    painter.text(
        Pos2::new(legend_x + 8.0, legend_y + 8.0),
        egui::Align2::LEFT_TOP,
        "Paths",
        egui::FontId::proportional(10.0),
        Color32::LIGHT_GRAY,
    );

    // Path entries
    for (i, &path_id) in path_ids.iter().enumerate() {
        let y = legend_y + 24.0 + (i as f32 * 22.0);
        let color = path_color(path_id);

        // Color box
        let box_rect = Rect::from_min_size(Pos2::new(legend_x + 8.0, y), Vec2::new(14.0, 14.0));
        painter.rect_filled(box_rect, 2.0, color);
        painter.rect_stroke(
            box_rect,
            2.0,
            Stroke::new(1.0, Color32::from_rgb(60, 60, 65)),
            StrokeKind::Inside,
        );

        // Path ID label
        painter.text(
            Pos2::new(legend_x + 28.0, y + 7.0),
            egui::Align2::LEFT_CENTER,
            format!("Path {}", path_id),
            egui::FontId::proportional(10.0),
            Color32::LIGHT_GRAY,
        );
    }
}

struct PacketInfo {
    time: f64,
    direction: PacketDirection,
    packet_type: String,
    packet_type_short: String,
    packet_number: u64,
    path_id: Option<u64>,
    frames: Vec<FrameType>,
    event_idx: usize,
}
