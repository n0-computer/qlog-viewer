use crate::app::LoadedFile;
use crate::packet_correlation::PacketCorrelation;
use crate::qlog_data::QlogData;
use crate::utils::{self, FrameType};
use egui::epaint::Hsva;
use egui::{Color32, Painter, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Vec2};
use qlog::events::EventData;
use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

const HEADER_HEIGHT: f32 = 65.0;
const DUAL_LEFT_MARGIN: f32 = 180.0;
const DUAL_RIGHT_MARGIN: f32 = 180.0;
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
    Hsva::new(h, s, v, 1.0).into()
}

pub struct SequenceDiagram {
    cache: Option<Cache>,
    rebuild_arrows: bool,
    arrows: Vec<DualArrow>,
    selected_packet_idx: Option<usize>,
    show_loss_markers: bool,
    dual_time_scale: f32,
    files_swapped: bool,
    compress_gaps: bool,
    pub visualization_mode: VisualizationMode,
    // Metrics events visualization
    metrics_events: Vec<MetricsEvent>,
    pub show_metrics_events: bool,
    pub show_all_metrics: bool,
    pub metrics_visualization_mode: MetricsVisualizationMode,
    pub overlay_graphs: bool,
    selected_metrics_event: Option<usize>,
    last_metrics: Option<LastMetricsState>,
}

const GAP_THRESHOLD_MS: f64 = 5.0;
const COMPRESSED_GAP_HEIGHT: f32 = 30.0;

#[derive(Clone)]
struct TimeGap {
    start_time: f64,
    end_time: f64,
    compressed_amount: f64,
}

#[derive(Debug, Clone)]
struct LastMetricsState {
    smoothed_rtt: Option<f32>,
    bytes_in_flight: Option<u64>,
    congestion_window: Option<u64>,
}

/// Key for identifying a specific path in metrics data
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PathKey {
    file_idx: usize,
    path_id: u64,
}

/// Metric data points for a specific path
#[derive(Debug, Clone, Default)]
struct PathMetricData {
    bytes_in_flight: Vec<(f64, u64)>,
    rtt: Vec<(f64, f32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricsDisplayStyle {
    Box,    // Colored box in margin (for important events)
    Marker, // Small shape on timeline (for frequent events)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricsVisualizationMode {
    Events, // Show boxes and markers for events
    Graphs, // Show vertical line graphs for metrics
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricsEventType {
    ConnectionStarted,
    ConnectionStateUpdated,
    MetricsUpdated,
    CongestionStateUpdated,
    PacketLost,
}

impl MetricsEventType {
    fn color(&self) -> Color32 {
        match self {
            Self::ConnectionStarted => Color32::from_rgb(0, 150, 136), // Teal
            Self::ConnectionStateUpdated => Color32::from_rgb(0, 150, 136), // Teal
            Self::MetricsUpdated => Color32::from_rgb(142, 68, 173),   // Purple
            Self::CongestionStateUpdated => Color32::from_rgb(255, 152, 0), // Orange
            Self::PacketLost => Color32::from_rgb(244, 67, 54),        // Red
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::ConnectionStarted => "Connection Started",
            Self::ConnectionStateUpdated => "Connection State",
            Self::MetricsUpdated => "Metrics Updated",
            Self::CongestionStateUpdated => "Congestion State",
            Self::PacketLost => "Packet Lost",
        }
    }

    // Determine display style based on event type
    fn display_style(&self) -> MetricsDisplayStyle {
        match self {
            // Important events: display as boxes
            Self::ConnectionStarted
            | Self::ConnectionStateUpdated
            | Self::CongestionStateUpdated
            | Self::PacketLost => MetricsDisplayStyle::Box,
            // Frequent events: display as markers
            Self::MetricsUpdated => MetricsDisplayStyle::Marker,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MetricsEvent {
    pub time: f64,
    pub event_type: MetricsEventType,
    pub display_text: String,
    pub detail_text: String, // Full details for popup/tooltip
    pub color: Color32,
    pub event_idx: usize,
    pub display_style: MetricsDisplayStyle,
    pub file_idx: usize, // Which qlog file this event came from (0=left, 1=right)
    pub path_id: Option<u64>, // Path ID for multipath visualization
    // Metric values for graph rendering
    pub smoothed_rtt: Option<f32>,
    pub bytes_in_flight: Option<u64>,
}

struct DualArrow {
    packet: Arc<PacketInfo>,
    send_time: f64,
    recv_time: f64,
    from_left: bool,
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

impl Default for SequenceDiagram {
    fn default() -> Self {
        Self {
            cache: None,
            arrows: Vec::new(),
            rebuild_arrows: true,
            selected_packet_idx: None,
            show_loss_markers: true,
            dual_time_scale: 30.0,
            files_swapped: false,
            compress_gaps: true,
            visualization_mode: VisualizationMode::ColorCoded,
            metrics_events: Vec::new(),
            show_metrics_events: true, // Default: visible
            show_all_metrics: true,    // Default: show all
            metrics_visualization_mode: MetricsVisualizationMode::Graphs, // Default: show graphs
            overlay_graphs: true,      // Default: overlaid
            selected_metrics_event: None,
            last_metrics: None,
        }
    }
}

impl SequenceDiagram {
    pub fn invalidate_cache(&mut self) {
        self.cache = None;
        self.rebuild_arrows = true;
    }

    pub fn show_single(
        &mut self,
        ui: &mut egui::Ui,
        data: &QlogData,
        _correlation: &PacketCorrelation,
        selected_event_idx: &mut Option<usize>,
    ) {
        let cache = self.cache.get_or_insert_with(|| Cache::build(data, None));
        if cache.is_empty() {
            ui.label("No packet events found in qlog");
            return;
        }

        // Convert all packets to dual arrows (with implied second endpoint)
        if self.rebuild_arrows {
            self.rebuild_arrows = false;
            self.arrows = Self::build_single_file_arrows(
                &cache.left.packets,
                &(0..cache.left.packets.len()).collect::<Vec<_>>(),
            );
        }
        if self.arrows.is_empty() {
            ui.label("No packet events found");
            return;
        }

        let (min_time, _max_time, time_range) = Self::compute_time_bounds(&self.arrows);
        let gaps = if self.compress_gaps {
            Self::detect_gaps(&self.arrows, min_time, self.dual_time_scale)
        } else {
            Vec::new()
        };

        self.render_header(ui);
        self.render_controls(ui, time_range);

        // Use the same dual rendering for single file (with dummy recv_selected_event_idx)
        let mut recv_selected_event_idx = None;
        let mut selected_file_idx = 0;
        self.render_diagram(
            ui,
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
        let cache = self
            .cache
            .get_or_insert_with(|| Cache::build(&file_a.qlog_data, Some(&file_b.qlog_data)));
        let cache_a = &cache.left;
        let cache_b = cache.right.as_ref().unwrap();
        let ((file_left, cache_left), (file_right, cache_right)) = if self.files_swapped {
            ((file_a, cache_a), (file_b, cache_b))
        } else {
            ((file_b, cache_b), (file_a, cache_a))
        };

        if self.rebuild_arrows {
            self.rebuild_arrows = false;
            self.arrows = Self::build_dual_arrows(cache_left, cache_right);
            self.metrics_events.clear();
            self.extract_metrics_events(&file_left.qlog_data, false, 0);
            self.extract_metrics_events(&file_right.qlog_data, true, 1);
        }

        if self.arrows.is_empty() {
            ui.label("No packet events found in either file");
            return;
        }

        let (min_time, _max_time, time_range) = Self::compute_time_bounds(&self.arrows);
        let gaps = if self.compress_gaps {
            Self::detect_gaps(&self.arrows, min_time, self.dual_time_scale)
        } else {
            Vec::new()
        };

        self.render_header(ui);
        self.render_controls(ui, time_range);
        self.render_diagram(
            ui,
            min_time,
            time_range,
            &gaps,
            selected_event_idx,
            recv_selected_event_idx,
            selected_file_idx,
            &file_left.label,
            &file_right.label,
        );
    }

    fn render_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Sequence Diagram");
            ui.separator();
            ui.label("Visualization mode");
            egui::ComboBox::from_id_salt("visualization_mode")
                .selected_text(format!("{:?}", self.visualization_mode))
                .show_ui(ui, |ui| {
                    for mode in [
                        VisualizationMode::ColorCoded,
                        VisualizationMode::VerticalLanes,
                    ] {
                        ui.selectable_value(
                            &mut self.visualization_mode,
                            mode,
                            format!("{mode:?}"),
                        );
                    }
                });
        });
    }

    fn calculate_lane_layout(
        &self,
        layout: &DiagramLayout,
        left_path_ids: &[u64],
        right_path_ids: &[u64],
    ) -> PathLaneLayout {
        let max_paths = left_path_ids.len().max(right_path_ids.len());

        // Fallback to color mode if >4 paths per side
        if max_paths > 4 {
            return PathLaneLayout {
                left_lanes: HashMap::new(),
                right_lanes: HashMap::new(),
                center_x: (layout.left_x + layout.right_x) / 2.0,
                use_lanes: false,
            };
        }

        let center_x = (layout.left_x + layout.right_x) / 2.0;

        let mut left_lanes = HashMap::new();
        let mut right_lanes = HashMap::new();

        // Lanes should be positioned WITHIN the sequence diagram area,
        // between the left timeline and center, and between center and right timeline
        // This area is independent of graphs which are drawn outside the timelines

        // Calculate lane positions with proper spacing
        // - Edge margin: 15px from time labels
        // - Center gap: 2X the spacing between adjacent lanes
        // - All other gaps: X (uniform)
        const LANE_EDGE_MARGIN: f32 = 15.0;

        let num_paths = left_path_ids.len().max(right_path_ids.len());

        if num_paths > 0 {
            // Calculate base spacing unit
            // Layout: [margin] pN [...] p1 [X] p0 [2X] p0 [X] p1 [...] pN [margin]
            // For N paths: need N spacing units from center to outermost lane on each side
            // Total width available (excluding margins)
            let total_width =
                (layout.right_x - LANE_EDGE_MARGIN) - (layout.left_x + LANE_EDGE_MARGIN);
            // Divide by 2*N to get base spacing unit
            let base_spacing = total_width / (2.0 * num_paths as f32);

            // Position left lanes working from center outward (reverse order for mirroring)
            for (i, &path_id) in left_path_ids.iter().rev().enumerate() {
                // Position from center: p2 at -3*base (leftmost), p1 at -2*base, p0 at -1*base (closest)
                let offset = (left_path_ids.len() - i) as f32;
                let x = center_x - offset * base_spacing;
                left_lanes.insert(path_id, x);
            }

            // Position right lanes working from center outward
            for (i, &path_id) in right_path_ids.iter().enumerate() {
                // Position from center: p0 at +1*base, p1 at +2*base, p2 at +3*base
                let offset = (i + 1) as f32;
                let x = center_x + offset * base_spacing;
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
        packets: &[Arc<PacketInfo>],
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
                    packet: p.clone(),
                    send_time: if from_left { p.time } else { p.time - 1.0 },
                    recv_time,
                    from_left,
                    is_lost: false, // In single-file, we don't know if it's truly lost
                    sent_event_idx: p.event_idx,
                    recv_event_idx,
                }
            })
            .collect()
    }

    fn build_dual_arrows(
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

        let mut arrows: Vec<DualArrow> =
            Vec::with_capacity(left_packets.len() + right_packets.len());

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

    fn render_controls(&mut self, ui: &mut egui::Ui, time_range: f64) {
        let total_loss_count = self.arrows.iter().filter(|a| a.is_lost).count();

        ui.horizontal(|ui| {
            ui.label(format!(
                "{} arrows | 0.00 - {:.2} ms ({:.1}s)",
                self.arrows.len(),
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
                self.rebuild_arrows = true;
            }

            // Metrics options
            ui.separator();
            if ui
                .checkbox(&mut self.show_metrics_events, "Show Metrics")
                .changed()
            {
                self.invalidate_cache();
            }

            if self.show_metrics_events {
                ui.separator();
                if ui
                    .radio(
                        self.metrics_visualization_mode == MetricsVisualizationMode::Events,
                        "Events",
                    )
                    .clicked()
                {
                    self.metrics_visualization_mode = MetricsVisualizationMode::Events;
                }
                if ui
                    .radio(
                        self.metrics_visualization_mode == MetricsVisualizationMode::Graphs,
                        "Graphs",
                    )
                    .clicked()
                {
                    self.metrics_visualization_mode = MetricsVisualizationMode::Graphs;
                }

                // Show the "Overlay" toggle when in Graphs mode
                if self.metrics_visualization_mode == MetricsVisualizationMode::Graphs {
                    ui.checkbox(&mut self.overlay_graphs, "Overlay");
                }

                // Show "Show All Metrics Updates" when in Events mode
                if self.metrics_visualization_mode == MetricsVisualizationMode::Events
                    && ui
                        .checkbox(&mut self.show_all_metrics, "Show All Metrics Updates")
                        .changed()
                {
                    self.invalidate_cache();
                }
            }
        });
        ui.separator();
    }

    #[allow(clippy::too_many_arguments)]
    fn render_diagram(
        &mut self,
        ui: &mut egui::Ui,
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

                // Calculate margins based on graph requirements
                let (left_margin, right_margin) = if self.show_metrics_events
                    && self.metrics_visualization_mode == MetricsVisualizationMode::Graphs
                {
                    // Count paths for each side to determine space needed
                    let (left_path_count, right_path_count) = if self.overlay_graphs {
                        // In overlay mode, we just need 2 strips per side if there are any paths
                        let has_left = self.metrics_events.iter().any(|e| e.file_idx == 0);
                        let has_right = self.metrics_events.iter().any(|e| e.file_idx == 1);
                        (if has_left { 1 } else { 0 }, if has_right { 1 } else { 0 })
                    } else {
                        // In split mode, count unique paths per side
                        use std::collections::BTreeSet;
                        let mut left_paths = BTreeSet::new();
                        let mut right_paths = BTreeSet::new();
                        for event in &self.metrics_events {
                            if event.file_idx == 0 {
                                left_paths.insert(event.path_id);
                            } else {
                                right_paths.insert(event.path_id);
                            }
                        }
                        (left_paths.len(), right_paths.len())
                    };

                    const STRIP_WIDTH: f32 = 40.0;
                    const TIME_LABEL_WIDTH: f32 = 50.0;
                    const EDGE_PADDING: f32 = 10.0;
                    const STRIP_SPACING: f32 = 8.0;
                    const PATH_SET_SPACING: f32 = 12.0;

                    let left_graph_width = if left_path_count > 0 {
                        if self.overlay_graphs {
                            // 2 strips (in flight + RTT) + spacing between them
                            2.0 * STRIP_WIDTH + STRIP_SPACING
                        } else {
                            // Each path has 2 strips, with spacing between strips and paths
                            let strips_per_path = 2.0;
                            left_path_count as f32 * STRIP_WIDTH * strips_per_path
                                + left_path_count as f32 * STRIP_SPACING
                                + (left_path_count.saturating_sub(1)) as f32 * PATH_SET_SPACING
                        }
                    } else {
                        0.0
                    };

                    let right_graph_width = if right_path_count > 0 {
                        if self.overlay_graphs {
                            2.0 * STRIP_WIDTH + STRIP_SPACING
                        } else {
                            let strips_per_path = 2.0;
                            right_path_count as f32 * STRIP_WIDTH * strips_per_path
                                + right_path_count as f32 * STRIP_SPACING
                                + (right_path_count.saturating_sub(1)) as f32 * PATH_SET_SPACING
                        }
                    } else {
                        0.0
                    };

                    let left_total = EDGE_PADDING + left_graph_width + TIME_LABEL_WIDTH;
                    let right_total = TIME_LABEL_WIDTH + right_graph_width + EDGE_PADDING;
                    (left_total, right_total)
                } else {
                    // No graphs shown, use default margins
                    (DUAL_LEFT_MARGIN, DUAL_RIGHT_MARGIN)
                };

                let layout = DiagramLayout {
                    left_x: rect.left() + left_margin,
                    right_x: rect.right() - right_margin,
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
                    for arrow in self.arrows.iter() {
                        if let Some(pid) = arrow.packet.path_id {
                            if arrow.from_left {
                                left_path_ids.insert(pid);
                            } else {
                                right_path_ids.insert(pid);
                            }
                        }
                    }
                    let left_vec: Vec<u64> = left_path_ids.into_iter().collect();
                    let right_vec: Vec<u64> = right_path_ids.into_iter().collect();
                    Some(self.calculate_lane_layout(&layout, &left_vec, &right_vec))
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

                let send_time_counts = Self::count_simultaneous_sends(&self.arrows);
                let arrow_rects = self.draw_dual_arrows(
                    &painter,
                    &layout,
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
                let mut path_ids: Vec<u64> = self
                    .arrows
                    .iter()
                    .filter_map(|a| a.packet.path_id)
                    .collect();
                path_ids.sort_unstable();
                path_ids.dedup();

                draw_path_legend(&painter, layout.rect, viewport, &path_ids);

                // Draw metrics events (boxes and markers)
                self.draw_metrics_events(&painter, ui, &layout, time_to_y, &viewport);
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
        send_time_counts: &HashMap<i64, usize>,
        time_to_y: &impl Fn(f64) -> f32,
        lane_layout: Option<&PathLaneLayout>,
    ) -> Vec<(usize, usize, Option<usize>, bool, Rect)> {
        let arrows = &self.arrows;
        let mut arrow_rects = Vec::new();
        let mut send_time_indices: HashMap<i64, usize> = HashMap::new();

        for (arrow_idx, arrow) in arrows.iter().enumerate() {
            let y_send = time_to_y(arrow.send_time);
            let y_recv = time_to_y(arrow.recv_time);

            // Determine start and end positions based on lane mode
            let (start_x, end_x) = if let Some(lanes) = lane_layout {
                if lanes.use_lanes {
                    // Use lane positions
                    let path_id = arrow.packet.path_id.unwrap_or(0);
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
            let color = path_color(arrow.packet.path_id.unwrap_or(0));

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

            draw_frame_tags(painter, &arrow.packet.frames, tag_x, tag_y, 32.0, 3);

            let info_offset = if arrow.from_left { 5.0 } else { -5.0 };
            let label = if let Some(pid) = arrow.packet.path_id {
                format!(
                    "{}:{} p{}",
                    arrow.packet.packet_type_short, arrow.packet.packet_number, pid
                )
            } else {
                format!(
                    "{}:{}",
                    arrow.packet.packet_type_short, arrow.packet.packet_number
                )
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

        let packets = qlog_data
            .events
            .par_iter()
            .enumerate()
            .filter_map(|(idx, event)| {
                let (header, direction, frames_iter): (_, _, &mut dyn Iterator<Item = _>) =
                    match &event.data {
                        EventData::PacketSent(d) => (
                            &d.header,
                            PacketDirection::Sent,
                            &mut d.frames.iter().flatten() as &mut dyn Iterator<Item = _>,
                        ),
                        EventData::PacketReceived(d) => (
                            &d.header,
                            PacketDirection::Received,
                            &mut d.frames.iter().flatten() as &mut dyn Iterator<Item = _>,
                        ),
                        _ => return None,
                    };

                let packet_type_raw = format!("{:?}", header.packet_type);
                let packet_type = utils::full_packet_type(&packet_type_raw);
                let packet_type_short = utils::short_packet_type(&packet_type_raw);

                let frames: Vec<FrameType> = frames_iter.map(FrameType::from_quic_frame).collect();

                // Unified path_id extraction from both formats
                let path_id = Self::extract_path_id(event, header.path_id);

                Some(Arc::new(PacketInfo {
                    time: event.time as f64,
                    direction,
                    packet_type,
                    packet_type_short,
                    packet_number: header.packet_number.unwrap_or(0),
                    path_id,
                    frames,
                    event_idx: idx,
                }))
            })
            .collect();
        result.packets = packets;
        for packet in result.packets.iter() {
            if !result.packet_types.contains(&packet.packet_type) {
                result.packet_types.insert(packet.packet_type.clone());
            }
        }
        result
    }

    fn extract_metrics_events(&mut self, qlog: &QlogData, append: bool, file_idx: usize) {
        if !append {
            self.metrics_events.clear();
            self.last_metrics = None;
        }

        for (idx, event) in qlog.events.iter().enumerate() {
            let path_id = Self::extract_path_id(event, None);

            let metrics_event = match &event.data {
                EventData::ConnectionStarted(_) => {
                    let event_type = MetricsEventType::ConnectionStarted;
                    Some(MetricsEvent {
                        time: event.time as f64,
                        event_type,
                        display_text: "connection started".to_string(),
                        detail_text: format!("Connection initiated at {:.3}ms", event.time),
                        color: event_type.color(),
                        event_idx: idx,
                        display_style: event_type.display_style(),
                        file_idx,
                        path_id,
                        smoothed_rtt: None,
                        bytes_in_flight: None,
                    })
                }

                EventData::MetricsUpdated(data) => {
                    // Only create event if there's meaningful data
                    if data.smoothed_rtt.is_some() || data.bytes_in_flight.is_some() {
                        let event_type = MetricsEventType::MetricsUpdated;

                        // Format short display text
                        let mut parts = Vec::new();
                        if let Some(srtt) = data.smoothed_rtt {
                            parts.push(format!("srtt: {:.3}", srtt));
                        }
                        if let Some(bif) = data.bytes_in_flight {
                            parts.push(format!("in flight: {}", bif));
                        }
                        let display = if parts.len() > 2 {
                            format!("{}, ...", parts[..2].join(", "))
                        } else {
                            parts.join(", ")
                        };

                        // Format full detail text
                        let mut lines = Vec::new();
                        if let Some(srtt) = data.smoothed_rtt {
                            lines.push(format!("Smoothed RTT: {:.3}ms", srtt));
                        }
                        if let Some(latest_rtt) = data.latest_rtt {
                            lines.push(format!("Latest RTT: {:.3}ms", latest_rtt));
                        }
                        if let Some(min_rtt) = data.min_rtt {
                            lines.push(format!("Min RTT: {:.3}ms", min_rtt));
                        }
                        if let Some(cwnd) = data.congestion_window {
                            lines.push(format!("Congestion Window: {} bytes", cwnd));
                        }
                        if let Some(bif) = data.bytes_in_flight {
                            lines.push(format!("Bytes in Flight: {}", bif));
                        }
                        if let Some(pif) = data.packets_in_flight {
                            lines.push(format!("Packets in Flight: {}", pif));
                        }
                        let detail = lines.join("\n");

                        // Check if this is significant change (if in "significant only" mode)
                        let current_state = LastMetricsState {
                            smoothed_rtt: data.smoothed_rtt,
                            bytes_in_flight: data.bytes_in_flight,
                            congestion_window: data.congestion_window,
                        };
                        let is_significant = self.is_significant_metrics_change(
                            self.last_metrics.as_ref(),
                            &current_state,
                        );

                        // Store for next comparison
                        self.last_metrics = Some(current_state);

                        // Skip if not showing all and not significant
                        if !self.show_all_metrics && !is_significant {
                            None
                        } else {
                            Some(MetricsEvent {
                                time: event.time as f64,
                                event_type,
                                display_text: display,
                                detail_text: detail,
                                color: event_type.color(),
                                event_idx: idx,
                                display_style: event_type.display_style(),
                                file_idx,
                                path_id: data.path_id,
                                smoothed_rtt: data.smoothed_rtt,
                                bytes_in_flight: data.bytes_in_flight,
                            })
                        }
                    } else {
                        None
                    }
                }

                EventData::CongestionStateUpdated(data) => {
                    let event_type = MetricsEventType::CongestionStateUpdated;
                    Some(MetricsEvent {
                        time: event.time as f64,
                        event_type,
                        display_text: format!("{:?}", data.new),
                        detail_text: format!(
                            "Congestion state changed to {:?} at {:.3}ms",
                            data.new, event.time
                        ),
                        color: event_type.color(),
                        event_idx: idx,
                        display_style: event_type.display_style(),
                        file_idx,
                        path_id: data.path_id,
                        smoothed_rtt: None,
                        bytes_in_flight: None,
                    })
                }

                EventData::ConnectionStateUpdated(data) => {
                    let event_type = MetricsEventType::ConnectionStateUpdated;
                    Some(MetricsEvent {
                        time: event.time as f64,
                        event_type,
                        display_text: format!("{:?}", data.new),
                        detail_text: format!("Connection state: {:?}", data.new),
                        color: event_type.color(),
                        event_idx: idx,
                        display_style: event_type.display_style(),
                        file_idx,
                        path_id,
                        smoothed_rtt: None,
                        bytes_in_flight: None,
                    })
                }

                EventData::PacketLost(data) => {
                    if let Some(header) = &data.header {
                        if let Some(pn) = header.packet_number {
                            let event_type = MetricsEventType::PacketLost;
                            Some(MetricsEvent {
                                time: event.time as f64,
                                event_type,
                                display_text: format!("lost PN {}", pn),
                                detail_text: format!("Packet {} lost at {:.3}ms", pn, event.time),
                                color: event_type.color(),
                                event_idx: idx,
                                display_style: event_type.display_style(),
                                file_idx,
                                path_id,
                                smoothed_rtt: None,
                                bytes_in_flight: None,
                            })
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }

                _ => None,
            };

            if let Some(event) = metrics_event {
                self.metrics_events.push(event);
            }
        }

        let num_boxes = self
            .metrics_events
            .iter()
            .filter(|e| e.display_style == MetricsDisplayStyle::Box)
            .count();
        let num_markers = self
            .metrics_events
            .iter()
            .filter(|e| e.display_style == MetricsDisplayStyle::Marker)
            .count();

        tracing::info!(
            "Extracted {} metrics events from {} total events ({} boxes, {} markers)",
            self.metrics_events.len(),
            qlog.events.len(),
            num_boxes,
            num_markers
        );
    }

    fn is_significant_metrics_change(
        &self,
        prev: Option<&LastMetricsState>,
        curr: &LastMetricsState,
    ) -> bool {
        // If no previous metrics, it's significant (first occurrence)
        let Some(prev) = prev else {
            return true;
        };

        // Check if bytes_in_flight changed significantly
        if let (Some(prev_bif), Some(curr_bif)) = (prev.bytes_in_flight, curr.bytes_in_flight) {
            if (curr_bif as i64 - prev_bif as i64).abs() > 1000 {
                return true;
            }
        }

        // Check if cwnd changed by >20%
        if let (Some(prev_cwnd), Some(curr_cwnd)) = (prev.congestion_window, curr.congestion_window)
        {
            if prev_cwnd > 0 {
                let change_pct = ((curr_cwnd as f64 - prev_cwnd as f64) / prev_cwnd as f64).abs();
                if change_pct > 0.2 {
                    return true;
                }
            }
        }

        // Check if RTT changed by >10%
        if let (Some(prev_rtt), Some(curr_rtt)) = (prev.smoothed_rtt, curr.smoothed_rtt) {
            if prev_rtt > 0.0 {
                let change_pct = ((curr_rtt - prev_rtt) / prev_rtt).abs();
                if change_pct > 0.1 {
                    return true;
                }
            }
        }

        false
    }

    fn draw_metrics_events(
        &mut self,
        painter: &Painter,
        ui: &mut egui::Ui,
        layout: &DiagramLayout,
        time_to_y: impl Fn(f64) -> f32,
        viewport: &Rect,
    ) {
        if !self.show_metrics_events {
            tracing::debug!("Metrics events hidden by toggle");
            return;
        }

        if self.metrics_events.is_empty() {
            tracing::debug!("No metrics events to display");
            return;
        }

        tracing::debug!("Drawing {} metrics events", self.metrics_events.len());

        match self.metrics_visualization_mode {
            MetricsVisualizationMode::Events => {
                // Separate events by display style - clone to avoid borrow issues
                let boxes: Vec<MetricsEvent> = self
                    .metrics_events
                    .iter()
                    .filter(|e| e.display_style == MetricsDisplayStyle::Box)
                    .cloned()
                    .collect();
                let markers: Vec<MetricsEvent> = self
                    .metrics_events
                    .iter()
                    .filter(|e| e.display_style == MetricsDisplayStyle::Marker)
                    .cloned()
                    .collect();

                // Draw markers first (behind boxes)
                Self::draw_metrics_markers_static(&markers, painter, ui, layout, &time_to_y);

                // Draw boxes on top
                self.draw_metrics_boxes_mut(&boxes, painter, ui, layout, &time_to_y);
            }
            MetricsVisualizationMode::Graphs => {
                self.draw_metrics_graphs(painter, ui, layout, &time_to_y, viewport);
            }
        }
    }

    fn draw_metrics_boxes_mut(
        &mut self,
        events: &[MetricsEvent],
        painter: &Painter,
        ui: &mut egui::Ui,
        layout: &DiagramLayout,
        time_to_y: &impl Fn(f64) -> f32,
    ) {
        const BOX_WIDTH: f32 = 140.0;
        const BOX_PADDING: f32 = 6.0;
        const MARGIN_FROM_TIMELINE: f32 = 50.0;

        for event in events {
            let y = time_to_y(event.time);

            // Draw colored box
            let text_galley = painter.layout_no_wrap(
                event.display_text.clone(),
                egui::FontId::proportional(10.0),
                Color32::WHITE,
            );

            let box_height = text_galley.size().y + BOX_PADDING * 2.0;

            // Position boxes to the left of the left timeline
            let box_x = layout.left_x - BOX_WIDTH - MARGIN_FROM_TIMELINE;

            let rect = Rect::from_min_size(
                Pos2::new(box_x, y - box_height / 2.0),
                Vec2::new(BOX_WIDTH, box_height),
            );

            painter.rect_filled(rect, 4.0, event.color);
            painter.rect_stroke(
                rect,
                4.0,
                Stroke::new(1.0, Color32::WHITE.linear_multiply(0.5)),
                StrokeKind::Inside,
            );

            // Draw text
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                &event.display_text,
                egui::FontId::proportional(10.0),
                Color32::WHITE,
            );

            // Add click interaction
            let response = ui.interact(
                rect,
                ui.id().with(("metrics_box", event.event_idx)),
                Sense::click(),
            );
            if response.clicked() {
                self.selected_metrics_event = Some(event.event_idx);
            }
        }

        // Draw popup if an event is selected
        if let Some(selected_idx) = self.selected_metrics_event {
            if let Some(event) = self
                .metrics_events
                .iter()
                .find(|e| e.event_idx == selected_idx)
            {
                egui::Window::new(event.event_type.name())
                    .fixed_pos(ui.cursor().min)
                    .show(ui.ctx(), |ui| {
                        ui.label(&event.detail_text);
                        if ui.button("Close").clicked() {
                            self.selected_metrics_event = None;
                        }
                    });
            }
        }
    }

    fn draw_metrics_markers_static(
        events: &[MetricsEvent],
        painter: &Painter,
        ui: &mut egui::Ui,
        layout: &DiagramLayout,
        time_to_y: &impl Fn(f64) -> f32,
    ) {
        const MARKER_SIZE: f32 = 6.0;
        const MARKER_OFFSET: f32 = 12.0;

        for event in events {
            let y = time_to_y(event.time);
            let x = layout.left_x - MARKER_OFFSET;

            let center = Pos2::new(x, y);

            // Draw diamond shape for metrics markers
            let points = vec![
                Pos2::new(x, y - MARKER_SIZE), // Top
                Pos2::new(x + MARKER_SIZE, y), // Right
                Pos2::new(x, y + MARKER_SIZE), // Bottom
                Pos2::new(x - MARKER_SIZE, y), // Left
            ];
            painter.add(egui::Shape::convex_polygon(
                points,
                event.color,
                Stroke::NONE,
            ));

            // Add hover interaction for tooltip
            let rect = Rect::from_center_size(center, Vec2::splat(MARKER_SIZE * 2.0));
            let response = ui.interact(
                rect,
                ui.id().with(("metrics_marker", event.event_idx)),
                Sense::hover(),
            );

            if response.hovered() {
                // Use Area for instant tooltip (no delay)
                egui::Area::new(ui.id().with(("marker_tooltip", event.event_idx)))
                    .fixed_pos(
                        ui.ctx().pointer_latest_pos().unwrap_or(center) + Vec2::new(10.0, 10.0),
                    )
                    .order(egui::Order::Tooltip)
                    .show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.label(&event.display_text);
                            ui.label(format!("Time: {:.3}ms", event.time));
                        });
                    });
            }
        }
    }

    fn draw_metrics_graphs(
        &self,
        painter: &Painter,
        ui: &mut egui::Ui,
        layout: &DiagramLayout,
        time_to_y: &impl Fn(f64) -> f32,
        viewport: &Rect,
    ) {
        const STRIP_SPACING: f32 = 8.0;
        const PATH_SET_SPACING: f32 = 12.0;
        const STRIP_WIDTH: f32 = 40.0;
        const EDGE_PADDING: f32 = 10.0;

        if self.overlay_graphs {
            // Overlay mode: group by (file_idx, path_id), then overlay paths per file
            let mut path_metrics: HashMap<PathKey, PathMetricData> = HashMap::new();

            for event in &self.metrics_events {
                if event.event_type == MetricsEventType::MetricsUpdated {
                    let pid = event.path_id.unwrap_or(0);
                    let key = PathKey {
                        file_idx: event.file_idx,
                        path_id: pid,
                    };
                    let entry = path_metrics.entry(key).or_default();

                    if let Some(bif) = event.bytes_in_flight {
                        entry.bytes_in_flight.push((event.time, bif));
                    }
                    if let Some(rtt) = event.smoothed_rtt {
                        entry.rtt.push((event.time, rtt));
                    }
                }
            }

            // Separate paths by file
            let mut left_paths: Vec<u64> = Vec::new();
            let mut right_paths: Vec<u64> = Vec::new();

            for key in path_metrics.keys() {
                if key.file_idx == 0 {
                    if !left_paths.contains(&key.path_id) {
                        left_paths.push(key.path_id);
                    }
                } else if !right_paths.contains(&key.path_id) {
                    right_paths.push(key.path_id);
                }
            }

            left_paths.sort_unstable();
            right_paths.sort_unstable();

            // Draw left side overlaid graphs
            if !left_paths.is_empty() {
                let bif_x = layout.rect.min.x + EDGE_PADDING;
                let rtt_x = bif_x + STRIP_WIDTH + STRIP_SPACING;

                // Collect all in-flight and RTT data for left file paths
                let mut left_bif: Vec<(u64, Vec<(f64, u64)>)> = Vec::new();
                let mut left_rtt: Vec<(u64, Vec<(f64, f32)>)> = Vec::new();

                for &path_id in &left_paths {
                    let key = PathKey {
                        file_idx: 0,
                        path_id,
                    };
                    if let Some(data) = path_metrics.get(&key) {
                        if !data.bytes_in_flight.is_empty() {
                            left_bif.push((path_id, data.bytes_in_flight.clone()));
                        }
                        if !data.rtt.is_empty() {
                            left_rtt.push((path_id, data.rtt.clone()));
                        }
                    }
                }

                Self::draw_metric_strip_overlaid(
                    painter,
                    "in flight",
                    &left_bif,
                    bif_x,
                    STRIP_WIDTH,
                    layout,
                    time_to_y,
                    ui,
                    viewport,
                );

                Self::draw_metric_strip_f32_overlaid(
                    painter,
                    "RTT",
                    &left_rtt,
                    rtt_x,
                    STRIP_WIDTH,
                    layout,
                    time_to_y,
                    ui,
                    viewport,
                );
            }

            // Draw right side overlaid graphs
            if !right_paths.is_empty() {
                let rtt_x = layout.rect.max.x - EDGE_PADDING - STRIP_WIDTH * 2.0 - STRIP_SPACING;
                let bif_x = rtt_x + STRIP_WIDTH + STRIP_SPACING;

                // Collect all in-flight and RTT data for right file paths
                let mut right_bif: Vec<(u64, Vec<(f64, u64)>)> = Vec::new();
                let mut right_rtt: Vec<(u64, Vec<(f64, f32)>)> = Vec::new();

                for &path_id in &right_paths {
                    let key = PathKey {
                        file_idx: 1,
                        path_id,
                    };
                    if let Some(data) = path_metrics.get(&key) {
                        if !data.bytes_in_flight.is_empty() {
                            right_bif.push((path_id, data.bytes_in_flight.clone()));
                        }
                        if !data.rtt.is_empty() {
                            right_rtt.push((path_id, data.rtt.clone()));
                        }
                    }
                }

                Self::draw_metric_strip_overlaid(
                    painter,
                    "in flight",
                    &right_bif,
                    bif_x,
                    STRIP_WIDTH,
                    layout,
                    time_to_y,
                    ui,
                    viewport,
                );

                Self::draw_metric_strip_f32_overlaid(
                    painter,
                    "RTT",
                    &right_rtt,
                    rtt_x,
                    STRIP_WIDTH,
                    layout,
                    time_to_y,
                    ui,
                    viewport,
                );
            }
        } else {
            // Split mode: group by (file_idx, path_id)
            let mut path_metrics: HashMap<PathKey, PathMetricData> = HashMap::new();

            for event in &self.metrics_events {
                if event.event_type == MetricsEventType::MetricsUpdated {
                    let pid = event.path_id.unwrap_or(0);
                    let key = PathKey {
                        file_idx: event.file_idx,
                        path_id: pid,
                    };
                    let entry = path_metrics.entry(key).or_default();

                    if let Some(bif) = event.bytes_in_flight {
                        entry.bytes_in_flight.push((event.time, bif));
                    }
                    if let Some(rtt) = event.smoothed_rtt {
                        entry.rtt.push((event.time, rtt));
                    }
                }
            }

            let mut left_paths: Vec<u64> = Vec::new();
            let mut right_paths: Vec<u64> = Vec::new();

            for key in path_metrics.keys() {
                if key.file_idx == 0 {
                    if !left_paths.contains(&key.path_id) {
                        left_paths.push(key.path_id);
                    }
                } else if !right_paths.contains(&key.path_id) {
                    right_paths.push(key.path_id);
                }
            }

            left_paths.sort_unstable();
            right_paths.sort_unstable();

            let strips_per_path = 2;

            let mut draw_side =
                |paths: &[u64], file_idx: usize, start_x: f32, going_right: bool| {
                    if paths.is_empty() {
                        return;
                    }

                    let num_paths = paths.len();
                    let mut current_x = start_x;

                    for &path_id in paths {
                        let (bif_x, rtt_x) = if going_right {
                            (current_x, current_x + STRIP_WIDTH + STRIP_SPACING)
                        } else {
                            let rtt_x =
                                current_x - STRIP_WIDTH * strips_per_path as f32 - STRIP_SPACING;
                            let bif_x = rtt_x + STRIP_WIDTH + STRIP_SPACING;
                            (bif_x, rtt_x)
                        };

                        let color = path_color(path_id);

                        let key = PathKey { file_idx, path_id };
                        if let Some(data) = path_metrics.get(&key) {
                            let label_prefix = if num_paths > 1 {
                                format!("P{} ", path_id)
                            } else {
                                String::new()
                            };

                            Self::draw_metric_strip(
                                painter,
                                &format!("{}in flight", label_prefix),
                                &data.bytes_in_flight,
                                bif_x,
                                STRIP_WIDTH,
                                layout,
                                time_to_y,
                                color,
                                ui,
                                viewport,
                            );

                            Self::draw_metric_strip_f32(
                                painter,
                                &format!("{}RTT", label_prefix),
                                &data.rtt,
                                rtt_x,
                                STRIP_WIDTH,
                                layout,
                                time_to_y,
                                color,
                                ui,
                                viewport,
                            );
                        }

                        if going_right {
                            current_x += STRIP_WIDTH * strips_per_path as f32
                                + STRIP_SPACING * (strips_per_path - 1) as f32
                                + PATH_SET_SPACING;
                        } else {
                            current_x -= STRIP_WIDTH * strips_per_path as f32
                                + STRIP_SPACING * (strips_per_path - 1) as f32
                                + PATH_SET_SPACING;
                        }
                    }
                };

            draw_side(&left_paths, 0, layout.rect.min.x + EDGE_PADDING, true);
            draw_side(&right_paths, 1, layout.rect.max.x - EDGE_PADDING, false);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_metric_strip_overlaid(
        painter: &Painter,
        label: &str,
        path_data: &[(u64, Vec<(f64, u64)>)],
        x: f32,
        width: f32,
        layout: &DiagramLayout,
        time_to_y: &impl Fn(f64) -> f32,
        _ui: &mut egui::Ui,
        _viewport: &Rect,
    ) {
        if path_data.is_empty() {
            return;
        }

        let rect = egui::Rect::from_min_max(
            egui::Pos2::new(x, layout.top_y),
            egui::Pos2::new(x + width, layout.rect.max.y),
        );

        painter.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(0, 0, 0, 30));
        painter.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, Color32::DARK_GRAY),
            egui::epaint::StrokeKind::Inside,
        );

        painter.text(
            egui::Pos2::new(x + width / 2.0, layout.top_y + 5.0),
            egui::Align2::CENTER_TOP,
            label,
            egui::FontId::proportional(9.0),
            Color32::LIGHT_GRAY,
        );

        // Find global min/max across all paths
        let mut all_values = Vec::new();
        for (_, points) in path_data {
            for (_, v) in points {
                all_values.push(*v);
            }
        }

        if all_values.is_empty() {
            return;
        }

        let max_value = *all_values.iter().max().unwrap();
        let min_value = *all_values.iter().min().unwrap();
        let value_range = if max_value > min_value {
            max_value - min_value
        } else {
            1
        };

        // Draw each path in different color
        for &(path_id, ref points) in path_data {
            if points.is_empty() {
                continue;
            }

            let color = path_color(path_id);
            let mut prev_point: Option<egui::Pos2> = None;

            for (time, value) in points {
                let y = time_to_y(*time);
                let normalized = (*value - min_value) as f32 / value_range as f32;
                let point_x = x + 5.0 + normalized * (width - 10.0);
                let point = egui::Pos2::new(point_x, y);

                if let Some(prev) = prev_point {
                    painter.line_segment([prev, point], egui::Stroke::new(1.5, color));
                }

                painter.circle_filled(point, 2.5, color);
                prev_point = Some(point);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_metric_strip_f32_overlaid(
        painter: &Painter,
        label: &str,
        path_data: &[(u64, Vec<(f64, f32)>)],
        x: f32,
        width: f32,
        layout: &DiagramLayout,
        time_to_y: &impl Fn(f64) -> f32,
        _ui: &mut egui::Ui,
        _viewport: &Rect,
    ) {
        if path_data.is_empty() {
            return;
        }

        let rect = egui::Rect::from_min_max(
            egui::Pos2::new(x, layout.top_y),
            egui::Pos2::new(x + width, layout.rect.max.y),
        );

        painter.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(0, 0, 0, 30));
        painter.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, Color32::DARK_GRAY),
            egui::epaint::StrokeKind::Inside,
        );

        painter.text(
            egui::Pos2::new(x + width / 2.0, layout.top_y + 5.0),
            egui::Align2::CENTER_TOP,
            label,
            egui::FontId::proportional(9.0),
            Color32::LIGHT_GRAY,
        );

        // Find global min/max across all paths
        let mut all_values = Vec::new();
        for (_, points) in path_data {
            for (_, v) in points {
                all_values.push(*v);
            }
        }

        if all_values.is_empty() {
            return;
        }

        let max_value = *all_values
            .iter()
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap();
        let min_value = *all_values
            .iter()
            .min_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap();
        let value_range = if max_value > min_value {
            max_value - min_value
        } else {
            0.001
        };

        // Draw each path in different color
        for &(path_id, ref points) in path_data {
            if points.is_empty() {
                continue;
            }

            let color = path_color(path_id);
            let mut prev_point: Option<egui::Pos2> = None;

            for (time, value) in points {
                let y = time_to_y(*time);
                let normalized = (*value - min_value) / value_range;
                let point_x = x + 5.0 + normalized * (width - 10.0);
                let point = egui::Pos2::new(point_x, y);

                if let Some(prev) = prev_point {
                    painter.line_segment([prev, point], egui::Stroke::new(1.5, color));
                }

                painter.circle_filled(point, 2.5, color);
                prev_point = Some(point);
            }
        }
    }

    fn format_bytes(bytes: u64) -> String {
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

    #[allow(clippy::too_many_arguments)]
    fn draw_metric_strip(
        painter: &Painter,
        label: &str,
        points: &[(f64, u64)],
        x: f32,
        width: f32,
        layout: &DiagramLayout,
        time_to_y: &impl Fn(f64) -> f32,
        color: Color32,
        ui: &mut egui::Ui,
        viewport: &Rect,
    ) {
        if points.is_empty() {
            return;
        }

        // Draw background rectangle
        let rect = egui::Rect::from_min_max(
            egui::Pos2::new(x, layout.top_y),
            egui::Pos2::new(x + width, layout.rect.max.y),
        );

        painter.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(0, 0, 0, 30));
        painter.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, Color32::DARK_GRAY),
            egui::epaint::StrokeKind::Inside,
        );

        // Draw label at the top
        painter.text(
            egui::Pos2::new(x + width / 2.0, layout.top_y + 5.0),
            egui::Align2::CENTER_TOP,
            label,
            egui::FontId::proportional(9.0),
            Color32::LIGHT_GRAY,
        );

        // Find min/max values for normalization
        let max_value = points.iter().map(|(_, v)| *v).max().unwrap_or(1);
        let min_value = points.iter().map(|(_, v)| *v).min().unwrap_or(0);
        let value_range = if max_value > min_value {
            max_value - min_value
        } else {
            1
        };

        // Draw horizontal grid lines
        const NUM_GRID_LINES: usize = 4;
        for i in 0..=NUM_GRID_LINES {
            let value = min_value + (value_range * i as u64 / NUM_GRID_LINES as u64);
            let normalized = (value - min_value) as f32 / value_range as f32;
            let grid_x = x + 5.0 + normalized * (width - 10.0);
            painter.line_segment(
                [
                    egui::Pos2::new(grid_x, layout.top_y + 20.0),
                    egui::Pos2::new(grid_x, layout.rect.max.y - 30.0),
                ],
                egui::Stroke::new(0.5, Color32::from_rgba_unmultiplied(100, 100, 100, 50)),
            );
        }

        // Draw line graph and collect points for hover interaction
        let mut prev_point: Option<egui::Pos2> = None;
        let mut graph_points = Vec::new();
        for (time, value) in points {
            let y = time_to_y(*time);
            // Normalize value to strip width (with padding)
            let normalized = (*value - min_value) as f32 / value_range as f32;
            let point_x = x + 5.0 + normalized * (width - 10.0);
            let point = egui::Pos2::new(point_x, y);

            // Draw line connecting to previous point
            if let Some(prev) = prev_point {
                painter.line_segment([prev, point], egui::Stroke::new(1.5, color));
            }

            // Draw bigger circle at data point
            painter.circle_filled(point, 3.5, color);

            graph_points.push((point, *value));
            prev_point = Some(point);
        }

        // Add hover interaction for data points
        for (point, value) in graph_points {
            let hover_rect = Rect::from_center_size(point, Vec2::splat(10.0));
            let response = ui.interact(
                hover_rect,
                ui.id()
                    .with(("metric_point", point.x as i32, point.y as i32)),
                Sense::hover(),
            );

            if response.hovered() {
                // Use Area for instant tooltip (no delay)
                egui::Area::new(ui.id().with(("tooltip", point.x as i32, point.y as i32)))
                    .fixed_pos(
                        ui.ctx().pointer_latest_pos().unwrap_or(point) + Vec2::new(10.0, 10.0),
                    )
                    .order(egui::Order::Tooltip)
                    .show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.label(format!("{}: {}", label, Self::format_bytes(value)));
                        });
                    });
            }
        }

        // Draw max label stuck to bottom of viewport (min assumed to be 0)
        let label_y = layout.rect.top() + viewport.max.y - 12.0;
        painter.text(
            egui::Pos2::new(x + width - 8.0, label_y),
            egui::Align2::RIGHT_BOTTOM,
            Self::format_bytes(max_value),
            egui::FontId::proportional(10.0),
            Color32::LIGHT_GRAY,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_metric_strip_f32(
        painter: &Painter,
        label: &str,
        points: &[(f64, f32)],
        x: f32,
        width: f32,
        layout: &DiagramLayout,
        time_to_y: &impl Fn(f64) -> f32,
        color: Color32,
        ui: &mut egui::Ui,
        viewport: &Rect,
    ) {
        if points.is_empty() {
            return;
        }

        // Draw background rectangle
        let rect = egui::Rect::from_min_max(
            egui::Pos2::new(x, layout.top_y),
            egui::Pos2::new(x + width, layout.rect.max.y),
        );

        painter.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(0, 0, 0, 30));
        painter.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, Color32::DARK_GRAY),
            egui::epaint::StrokeKind::Inside,
        );

        // Draw label at the top
        painter.text(
            egui::Pos2::new(x + width / 2.0, layout.top_y + 5.0),
            egui::Align2::CENTER_TOP,
            label,
            egui::FontId::proportional(9.0),
            Color32::LIGHT_GRAY,
        );

        // Find min/max values for normalization
        let max_value = points
            .iter()
            .map(|(_, v)| *v)
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(1.0);
        let min_value = points
            .iter()
            .map(|(_, v)| *v)
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(0.0);
        let value_range = if max_value > min_value {
            max_value - min_value
        } else {
            1.0
        };

        // Draw horizontal grid lines
        const NUM_GRID_LINES: usize = 4;
        for i in 0..=NUM_GRID_LINES {
            let value = min_value + (value_range * i as f32 / NUM_GRID_LINES as f32);
            let normalized = (value - min_value) / value_range;
            let grid_x = x + 5.0 + normalized * (width - 10.0);
            painter.line_segment(
                [
                    egui::Pos2::new(grid_x, layout.top_y + 20.0),
                    egui::Pos2::new(grid_x, layout.rect.max.y - 30.0),
                ],
                egui::Stroke::new(0.5, Color32::from_rgba_unmultiplied(100, 100, 100, 50)),
            );
        }

        // Draw line graph and collect points for hover interaction
        let mut prev_point: Option<egui::Pos2> = None;
        let mut graph_points = Vec::new();
        for (time, value) in points {
            let y = time_to_y(*time);
            // Normalize value to strip width (with padding)
            let normalized = (*value - min_value) / value_range;
            let point_x = x + 5.0 + normalized * (width - 10.0);
            let point = egui::Pos2::new(point_x, y);

            // Draw line connecting to previous point
            if let Some(prev) = prev_point {
                painter.line_segment([prev, point], egui::Stroke::new(1.5, color));
            }

            // Draw bigger circle at data point
            painter.circle_filled(point, 3.5, color);

            graph_points.push((point, *value));
            prev_point = Some(point);
        }

        // Add hover interaction for data points
        for (point, value) in graph_points {
            let hover_rect = Rect::from_center_size(point, Vec2::splat(10.0));
            let response = ui.interact(
                hover_rect,
                ui.id()
                    .with(("metric_point_f32", point.x as i32, point.y as i32)),
                Sense::hover(),
            );

            if response.hovered() {
                // Use Area for instant tooltip (no delay)
                egui::Area::new(
                    ui.id()
                        .with(("tooltip_f32", point.x as i32, point.y as i32)),
                )
                .fixed_pos(ui.ctx().pointer_latest_pos().unwrap_or(point) + Vec2::new(10.0, 10.0))
                .order(egui::Order::Tooltip)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.label(format!("{}: {:.2}ms", label, value));
                    });
                });
            }
        }

        // Draw max label stuck to bottom of viewport (min assumed to be 0)
        let label_y = layout.rect.top() + viewport.max.y - 12.0;
        painter.text(
            egui::Pos2::new(x + width - 8.0, label_y),
            egui::Align2::RIGHT_BOTTOM,
            format!("{:.1}ms", max_value),
            egui::FontId::proportional(10.0),
            Color32::LIGHT_GRAY,
        );
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

        // Draw time range inside sequence diagram on right side
        let right_text_x = layout.right_x - 10.0;
        let start_label = format!("{:.1}ms", gap.start_time);
        let end_label = format!("{:.1}ms", gap.end_time);
        painter.text(
            Pos2::new(right_text_x, y_pos + 5.0),
            egui::Align2::RIGHT_TOP,
            &start_label,
            egui::FontId::proportional(9.0),
            Color32::from_rgb(120, 120, 120),
        );
        painter.text(
            Pos2::new(right_text_x, end_y - 5.0),
            egui::Align2::RIGHT_BOTTOM,
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
            Pos2::new(layout.right_x + 15.0, y),
            egui::Align2::LEFT_CENTER,
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
    packets: Vec<Arc<PacketInfo>>,
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

struct Cache {
    left: ExtractedPackets,
    right: Option<ExtractedPackets>,
}

impl Cache {
    fn build(left: &QlogData, right: Option<&QlogData>) -> Self {
        Self {
            left: SequenceDiagram::extract_packets(left),
            right: right.map(SequenceDiagram::extract_packets),
        }
    }
    fn is_empty(&self) -> bool {
        self.left.packets.is_empty()
    }
}
