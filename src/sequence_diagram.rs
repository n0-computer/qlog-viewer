use crate::app::LoadedFile;
use crate::packet_correlation::PacketCorrelation;
use crate::qlog_data::QlogData;
use crate::utils::{self, FrameType};
use egui::{Color32, Painter, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Vec2};
use qlog::events::EventData;
use std::collections::{BTreeSet, HashMap};

const PACKET_HEIGHT: f32 = 35.0;
const VISIBLE_BUFFER: usize = 10;
const HEADER_HEIGHT: f32 = 40.0;
const LEFT_MARGIN: f32 = 110.0;
const RIGHT_MARGIN: f32 = 120.0;
const DUAL_LEFT_MARGIN: f32 = 60.0;
const DUAL_RIGHT_MARGIN: f32 = 70.0;
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
    available_stream_ids: Vec<u64>,
    available_packet_types: Vec<String>,
    filter_stream_id: Option<u64>,
    filter_packet_type: Option<String>,
    show_loss_markers: bool,
    show_reordering: bool,
    show_rtt: bool,
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
            available_stream_ids: Vec::new(),
            available_packet_types: Vec::new(),
            filter_stream_id: None,
            filter_packet_type: None,
            show_loss_markers: true,
            show_reordering: false,
            show_rtt: false,
            dual_time_scale: 30.0,
            files_swapped: false,
            compress_gaps: true,
            visualization_mode: VisualizationMode::ColorCoded,
        }
    }

    pub fn invalidate_cache(&mut self) {
        self.cache_valid = false;
        self.cached_packets.clear();
        self.available_stream_ids.clear();
        self.available_packet_types.clear();
        self.filter_stream_id = None;
        self.filter_packet_type = None;
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        qlog_data: &QlogData,
        correlation: &PacketCorrelation,
        selected_event_idx: &mut Option<usize>,
    ) {
        self.ensure_cache(qlog_data);

        if self.cached_packets.is_empty() {
            ui.label("No packet events found in qlog");
            return;
        }

        ui.horizontal(|ui| ui.heading("Sequence Diagram"));
        self.render_filter_controls(ui, correlation);

        let filtered_indices = self.filter_packet_indices();
        self.render_packet_stats(ui, &filtered_indices);

        if filtered_indices.is_empty() {
            ui.label("No packets match the current filters");
            return;
        }

        self.render_single_file_diagram(ui, &filtered_indices, correlation, selected_event_idx);
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
            Self::detect_gaps(&arrows, min_time)
        } else {
            Vec::new()
        };

        self.render_dual_controls(ui, &arrows, time_range);

        self.render_dual_file_diagram(
            ui,
            &arrows,
            left_file,
            right_file,
            min_time,
            time_range,
            &gaps,
            selected_event_idx,
            recv_selected_event_idx,
            selected_file_idx,
        );
    }

    fn ensure_cache(&mut self, qlog_data: &QlogData) {
        if self.cache_valid {
            return;
        }
        let extracted = Self::extract_packets(qlog_data);
        self.cached_packets = extracted.packets;
        self.available_stream_ids = extracted.stream_ids.into_iter().collect();
        self.available_packet_types = extracted.packet_types.into_iter().collect();
        self.cache_valid = true;
    }

    fn render_filter_controls(&mut self, ui: &mut egui::Ui, correlation: &PacketCorrelation) {
        ui.horizontal(|ui| {
            ui.label("Packet Type:");
            let current_type_label = self.filter_packet_type.as_deref().unwrap_or("All");
            egui::ComboBox::from_id_salt("packet_type_filter")
                .selected_text(current_type_label)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.filter_packet_type.is_none(), "All")
                        .clicked()
                    {
                        self.filter_packet_type = None;
                    }
                    for ptype in &self.available_packet_types {
                        let selected = self.filter_packet_type.as_ref() == Some(ptype);
                        if ui.selectable_label(selected, ptype).clicked() {
                            self.filter_packet_type = Some(ptype.clone());
                        }
                    }
                });

            ui.separator();
            ui.label("Stream ID:");
            let current_stream_label = self
                .filter_stream_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "All".to_string());
            egui::ComboBox::from_id_salt("stream_id_filter")
                .selected_text(&current_stream_label)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.filter_stream_id.is_none(), "All")
                        .clicked()
                    {
                        self.filter_stream_id = None;
                    }
                    for &stream_id in &self.available_stream_ids {
                        let selected = self.filter_stream_id == Some(stream_id);
                        if ui
                            .selectable_label(selected, stream_id.to_string())
                            .clicked()
                        {
                            self.filter_stream_id = Some(stream_id);
                        }
                    }
                });

            ui.separator();
            if ui.button("Clear Filters").clicked() {
                self.filter_packet_type = None;
                self.filter_stream_id = None;
            }

            ui.separator();
            ui.checkbox(&mut self.show_loss_markers, "Show Lost");
            if self.show_loss_markers && correlation.loss_count() > 0 {
                ui.label(format!("({} lost)", correlation.loss_count()));
            }
            ui.separator();
            ui.checkbox(&mut self.show_reordering, "Show Reordering");
            if self.show_reordering && !correlation.reorderings.is_empty() {
                ui.label(format!("({} reorders)", correlation.reorderings.len()));
            }
            ui.separator();
            ui.checkbox(&mut self.show_rtt, "Show RTT");
        });
        ui.separator();
    }

    fn filter_packet_indices(&self) -> Vec<usize> {
        self.cached_packets
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                let type_match = self
                    .filter_packet_type
                    .as_ref()
                    .map(|t| &p.packet_type == t)
                    .unwrap_or(true);
                let stream_match = self
                    .filter_stream_id
                    .map(|id| p.stream_ids.contains(&id))
                    .unwrap_or(true);
                type_match && stream_match
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn render_packet_stats(&self, ui: &mut egui::Ui, filtered_indices: &[usize]) {
        let min_time = filtered_indices
            .first()
            .map(|&i| self.cached_packets[i].time)
            .unwrap_or(0.0);
        let max_time = filtered_indices
            .last()
            .map(|&i| self.cached_packets[i].time)
            .unwrap_or(1000.0);
        let total_time = max_time - min_time;

        ui.horizontal(|ui| {
            ui.label(format!(
                "{} packets (of {}) | {:.0} - {:.0} ms ({:.1}s)",
                filtered_indices.len(),
                self.cached_packets.len(),
                min_time,
                max_time,
                total_time / 1000.0
            ));
        });
        ui.separator();
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

    fn render_single_file_diagram(
        &mut self,
        ui: &mut egui::Ui,
        filtered_indices: &[usize],
        correlation: &PacketCorrelation,
        selected_event_idx: &mut Option<usize>,
    ) {
        let total_content_height = (filtered_indices.len() as f32 * PACKET_HEIGHT) + 100.0;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                let available_width = ui.available_width();
                let desired_size = Vec2::new(available_width, total_content_height);
                let (response, painter) =
                    ui.allocate_painter(desired_size, Sense::click_and_drag());
                let rect = response.rect;

                let layout = DiagramLayout {
                    left_x: rect.left() + LEFT_MARGIN,
                    right_x: rect.right() - RIGHT_MARGIN,
                    top_y: rect.top() + HEADER_HEIGHT,
                    rect,
                };

                let (first_vis, last_vis) = Self::visible_range(viewport, filtered_indices.len());

                // Calculate lane layout if in lane mode
                let lane_layout = if self.visualization_mode == VisualizationMode::VerticalLanes {
                    // Collect unique path IDs from visible packets
                    let mut left_path_ids = BTreeSet::new();
                    let mut right_path_ids = BTreeSet::new();
                    for &idx in filtered_indices.iter() {
                        let packet = &self.cached_packets[idx];
                        if let Some(pid) = packet.path_id {
                            match packet.direction {
                                PacketDirection::Sent => {
                                    left_path_ids.insert(pid);
                                }
                                PacketDirection::Received => {
                                    right_path_ids.insert(pid);
                                }
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
                    "Client",
                    "Server",
                    Color32::WHITE,
                    Color32::WHITE,
                );

                let timeline_top =
                    (layout.top_y + first_vis as f32 * PACKET_HEIGHT).max(layout.top_y);
                let timeline_bottom = layout.top_y + last_vis as f32 * PACKET_HEIGHT;

                // Draw timelines (need to draw for each lane if in lane mode)
                if let Some(ref lanes) = lane_layout {
                    if lanes.use_lanes {
                        draw_lane_timelines(
                            &painter,
                            &layout,
                            timeline_top,
                            timeline_bottom,
                            lanes,
                        );
                    } else {
                        draw_timelines(&painter, &layout, timeline_top, timeline_bottom);
                    }
                } else {
                    draw_timelines(&painter, &layout, timeline_top, timeline_bottom);
                }

                for vis_idx in first_vis..last_vis {
                    let Some(&pkt_idx) = filtered_indices.get(vis_idx) else {
                        break;
                    };
                    let packet = &self.cached_packets[pkt_idx];
                    let is_selected = self.selected_packet_idx == Some(vis_idx);

                    let arrow_rect = draw_single_packet(
                        &painter,
                        &layout,
                        packet,
                        vis_idx,
                        is_selected,
                        self.show_loss_markers,
                        self.show_rtt,
                        correlation,
                        lane_layout.as_ref(),
                    );

                    if response.clicked() {
                        if let Some(pos) = response.interact_pointer_pos() {
                            if arrow_rect.contains(pos) {
                                self.selected_packet_idx = Some(vis_idx);
                                *selected_event_idx = Some(packet.event_idx);
                            }
                        }
                    }
                }

                if self.show_reordering {
                    draw_reordering(
                        &painter,
                        &layout,
                        &self.cached_packets,
                        filtered_indices,
                        first_vis,
                        last_vis,
                        correlation,
                    );
                }

                draw_progress_indicator(
                    &painter,
                    &layout,
                    viewport,
                    first_vis,
                    filtered_indices.len(),
                );

                // Collect unique path_ids for legend
                let mut path_ids: Vec<u64> = filtered_indices
                    .iter()
                    .filter_map(|&idx| self.cached_packets.get(idx))
                    .filter_map(|p| p.path_id)
                    .collect();
                path_ids.sort_unstable();
                path_ids.dedup();

                draw_path_legend(&painter, layout.rect, viewport, &path_ids);
            });
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

    fn detect_gaps(arrows: &[DualArrow], min_time: f64) -> Vec<TimeGap> {
        if arrows.is_empty() {
            return Vec::new();
        }

        let mut event_times: Vec<f64> = arrows
            .iter()
            .flat_map(|a| [a.send_time, a.recv_time])
            .collect();
        event_times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        event_times.dedup();

        let mut gaps = Vec::new();
        let mut prev_time = min_time;

        for &t in &event_times {
            let gap_size = t - prev_time;
            if gap_size > GAP_THRESHOLD_MS {
                let compressed = gap_size - GAP_THRESHOLD_MS;
                gaps.push(TimeGap {
                    start_time: prev_time + GAP_THRESHOLD_MS / 2.0,
                    end_time: t - GAP_THRESHOLD_MS / 2.0,
                    compressed_amount: compressed,
                });
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
        left_file: &LoadedFile,
        right_file: &LoadedFile,
        min_time: f64,
        time_range: f64,
        gaps: &[TimeGap],
        selected_event_idx: &mut Option<usize>,
        recv_selected_event_idx: &mut Option<usize>,
        selected_file_idx: &mut usize,
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
                    &left_file.label,
                    &right_file.label,
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
                    8.0,
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

    fn visible_range(viewport: Rect, total_count: usize) -> (usize, usize) {
        let first = ((viewport.top() - HEADER_HEIGHT) / PACKET_HEIGHT)
            .floor()
            .max(0.0) as usize;
        let first = first.saturating_sub(VISIBLE_BUFFER);

        let last = ((viewport.bottom() - HEADER_HEIGHT) / PACKET_HEIGHT)
            .ceil()
            .max(0.0) as usize;
        let last = (last + VISIBLE_BUFFER).min(total_count);

        (first, last)
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

            let mut pkt_stream_ids = Vec::new();
            let frames: Vec<FrameType> = frames_iter
                .map(|frame| {
                    if let Some(sid) = utils::get_frame_stream_id(frame) {
                        pkt_stream_ids.push(sid);
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
                stream_ids: pkt_stream_ids,
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
        let center_x = (layout.left_x + layout.right_x) / 2.0;

        for x in [layout.left_x, layout.right_x] {
            painter.line_segment(
                [Pos2::new(x, y_pos), Pos2::new(x, end_y)],
                Stroke::new(5.0, gap_color),
            );
        }

        let label = format!("{:.1}ms compressed", gap.compressed_amount);
        painter.text(
            Pos2::new(center_x, y_pos + COMPRESSED_GAP_HEIGHT / 2.0),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(11.0),
            text_color,
        );
    }
}

fn draw_time_ticks(
    painter: &Painter,
    start_x: f32,
    y_start: f32,
    end_x: f32,
    y_end: f32,
    color: Color32,
) {
    let half_width = 20.0;
    painter.line_segment(
        [
            Pos2::new(start_x - half_width, y_start),
            Pos2::new(start_x + half_width, y_start),
        ],
        Stroke::new(1.5, color),
    );
    painter.line_segment(
        [
            Pos2::new(end_x - half_width, y_end),
            Pos2::new(end_x + half_width, y_end),
        ],
        Stroke::new(1.5, color),
    );
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
    let tag_gap = 2.0;
    let total_width = frames_to_show.len() as f32 * (tag_width + tag_gap);
    let start_x = mid_x - total_width / 2.0;

    for (i, frame) in frames_to_show.iter().enumerate() {
        let tag_x = start_x + (i as f32 * (tag_width + tag_gap));
        let tag_rect =
            Rect::from_min_size(Pos2::new(tag_x, mid_y - 6.0), Vec2::new(tag_width, 12.0));
        painter.rect_filled(tag_rect, 2.0, frame.color());
        painter.text(
            tag_rect.center(),
            egui::Align2::CENTER_CENTER,
            frame.short_name(),
            egui::FontId::proportional(8.0),
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

fn draw_progress_indicator(
    painter: &Painter,
    layout: &DiagramLayout,
    viewport: Rect,
    first_idx: usize,
    total: usize,
) {
    let current = first_idx + VISIBLE_BUFFER;
    let progress = current as f32 / total as f32 * 100.0;
    painter.text(
        Pos2::new(
            layout.rect.right() - 10.0,
            layout.rect.top() + viewport.top() + 25.0,
        ),
        egui::Align2::RIGHT_CENTER,
        format!("#{} ({:.0}%)", current.min(total), progress),
        egui::FontId::proportional(10.0),
        Color32::GRAY,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_single_packet(
    painter: &Painter,
    layout: &DiagramLayout,
    packet: &PacketInfo,
    idx: usize,
    is_selected: bool,
    show_loss_markers: bool,
    show_rtt: bool,
    correlation: &PacketCorrelation,
    lane_layout: Option<&PathLaneLayout>,
) -> Rect {
    let y_start = layout.top_y + (idx as f32 * PACKET_HEIGHT) + PACKET_HEIGHT / 2.0;
    let diagonal_offset = PACKET_HEIGHT * 0.4;

    // Determine start and end positions based on lane mode
    let (start_x, end_x, y_end) = if let Some(lanes) = lane_layout {
        if lanes.use_lanes {
            // Use lane positions
            let path_id = packet.path_id.unwrap_or(0);
            match packet.direction {
                PacketDirection::Sent => {
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
                    (start, end, y_start + diagonal_offset)
                }
                PacketDirection::Received => {
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
                    (start, end, y_start + diagonal_offset)
                }
            }
        } else {
            // Fallback to color mode
            match packet.direction {
                PacketDirection::Sent => (layout.left_x, layout.right_x, y_start + diagonal_offset),
                PacketDirection::Received => {
                    (layout.right_x, layout.left_x, y_start + diagonal_offset)
                }
            }
        }
    } else {
        // Default positioning (color mode)
        match packet.direction {
            PacketDirection::Sent => (layout.left_x, layout.right_x, y_start + diagonal_offset),
            PacketDirection::Received => (layout.right_x, layout.left_x, y_start + diagonal_offset),
        }
    };

    // Use path color for multipath visualization
    let color = path_color(packet.path_id.unwrap_or(0));

    draw_time_ticks(painter, start_x, y_start, end_x, y_end, color);
    painter.line_segment(
        [Pos2::new(start_x, y_start), Pos2::new(end_x, y_end)],
        Stroke::new(2.0, color),
    );
    painter.circle_filled(Pos2::new(start_x, y_start), 4.0, color);
    painter.circle_filled(Pos2::new(end_x, y_end), 4.0, color);

    painter.text(
        Pos2::new(layout.rect.left() + 8.0, y_start),
        egui::Align2::LEFT_CENTER,
        format!("{:.2}", packet.time),
        egui::FontId::proportional(11.0),
        Color32::GRAY,
    );

    let mid_x = (start_x + end_x) / 2.0;
    let mid_y = (y_start + y_end) / 2.0;

    draw_frame_tags(painter, &packet.frames, mid_x, mid_y, 38.0, 4);

    painter.text(
        Pos2::new(layout.rect.right() - 10.0, y_end),
        egui::Align2::RIGHT_CENTER,
        format!("{:.2}", packet.time),
        egui::FontId::proportional(11.0),
        Color32::GRAY,
    );

    let info_x = if start_x < end_x {
        end_x + 8.0
    } else {
        start_x + 8.0
    };
    let label = if let Some(pid) = packet.path_id {
        format!(
            "{}:{} p{}",
            packet.packet_type_short, packet.packet_number, pid
        )
    } else {
        format!("{}:{}", packet.packet_type_short, packet.packet_number)
    };
    painter.text(
        Pos2::new(info_x, mid_y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(10.0),
        Color32::LIGHT_GRAY,
    );

    let arrow_rect = Rect::from_two_pos(
        Pos2::new(start_x.min(end_x) - 5.0, y_start.min(y_end) - 12.0),
        Pos2::new(start_x.max(end_x) + 5.0, y_start.max(y_end) + 12.0),
    );

    if is_selected {
        painter.rect_stroke(
            arrow_rect,
            2.0,
            Stroke::new(2.0, Color32::YELLOW),
            StrokeKind::Inside,
        );
    }

    if show_loss_markers
        && packet.direction == PacketDirection::Sent
        && correlation.is_packet_lost(
            packet.path_id.unwrap_or(0),
            &packet.packet_type,
            packet.packet_number,
        )
    {
        draw_loss_marker(painter, Pos2::new(mid_x, mid_y + 18.0), 14.0);
    }

    if show_rtt && packet.direction == PacketDirection::Sent {
        if let Some(rtt) = correlation.get_rtt(
            packet.path_id.unwrap_or(0),
            &packet.packet_type,
            packet.packet_number,
        ) {
            painter.text(
                Pos2::new(mid_x + 5.0, y_start - 2.0),
                egui::Align2::LEFT_BOTTOM,
                format!("{:.1}ms", rtt),
                egui::FontId::proportional(9.0),
                Color32::from_rgb(100, 200, 100),
            );
        }
    }

    arrow_rect
}

#[allow(clippy::too_many_arguments)]
fn draw_reordering(
    painter: &Painter,
    layout: &DiagramLayout,
    all_packets: &[PacketInfo],
    filtered_indices: &[usize],
    first_vis: usize,
    last_vis: usize,
    correlation: &PacketCorrelation,
) {
    let visible_start = filtered_indices
        .get(first_vis)
        .map(|&i| all_packets[i].time as f32)
        .unwrap_or(0.0);
    let visible_end = filtered_indices
        .get(last_vis.saturating_sub(1))
        .map(|&i| all_packets[i].time as f32)
        .unwrap_or(f32::MAX);

    let pn_to_y: HashMap<u64, f32> = (first_vis..last_vis)
        .filter_map(|vis_idx| filtered_indices.get(vis_idx).map(|&i| &all_packets[i]))
        .filter(|p| p.direction == PacketDirection::Received)
        .enumerate()
        .map(|(i, p)| {
            (
                p.packet_number,
                layout.top_y + (first_vis + i) as f32 * PACKET_HEIGHT + PACKET_HEIGHT / 2.0,
            )
        })
        .collect();

    let reorder_color = Color32::from_rgb(255, 140, 0);
    let line_x = layout.right_x + 50.0;

    for reorder in correlation.visible_reorderings(visible_start, visible_end, 50) {
        let (Some(&y1), Some(&y2)) = (
            pn_to_y.get(&reorder.earlier_pn),
            pn_to_y.get(&reorder.later_pn),
        ) else {
            continue;
        };

        painter.line_segment(
            [Pos2::new(line_x, y1), Pos2::new(line_x, y2)],
            Stroke::new(2.0, reorder_color),
        );
        painter.circle_filled(Pos2::new(line_x, y1), 5.0, reorder_color);
        painter.circle_filled(Pos2::new(line_x, y2), 5.0, reorder_color);

        for (y, pn) in [(y1, reorder.earlier_pn), (y2, reorder.later_pn)] {
            painter.text(
                Pos2::new(line_x + 8.0, y),
                egui::Align2::LEFT_CENTER,
                format!("#{pn}"),
                egui::FontId::proportional(9.0),
                reorder_color,
            );
        }
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
        tip.x - ux * size + px * size * 0.5,
        tip.y - uy * size + py * size * 0.5,
    );
    let right = Pos2::new(
        tip.x - ux * size - px * size * 0.5,
        tip.y - uy * size - py * size * 0.5,
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
    stream_ids: Vec<u64>,
    event_idx: usize,
}
