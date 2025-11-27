use crate::app::LoadedFile;
use crate::packet_correlation::PacketCorrelation;
use crate::qlog_data::QlogData;
use crate::utils::{self, FrameType};
use egui::{Color32, Painter, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use qlog::events::EventData;
use std::collections::{BTreeSet, HashMap};

const PACKET_HEIGHT: f32 = 35.0;
const VISIBLE_BUFFER: usize = 10;
const HEADER_HEIGHT: f32 = 40.0;
const LEFT_MARGIN: f32 = 110.0;
const RIGHT_MARGIN: f32 = 120.0;
const SENT_COLOR: Color32 = Color32::from_rgb(0, 150, 255);
const RECEIVED_COLOR: Color32 = Color32::from_rgb(220, 80, 80);
const LOSS_COLOR: Color32 = Color32::RED;

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
            dual_time_scale: 5.0,
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
        if !self.cache_valid {
            let extracted = Self::extract_packets(qlog_data);
            self.cached_packets = extracted.packets;
            self.available_stream_ids = extracted.stream_ids.into_iter().collect();
            self.available_packet_types = extracted.packet_types.into_iter().collect();
            self.cache_valid = true;
        }

        if self.cached_packets.is_empty() {
            ui.label("No packet events found in qlog");
            return;
        }

        ui.horizontal(|ui| {
            ui.heading("Sequence Diagram");
        });

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

        let filtered_packets: Vec<&PacketInfo> = self
            .cached_packets
            .iter()
            .filter(|p| {
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
            .collect();

        let min_time = filtered_packets.first().map(|p| p.time).unwrap_or(0.0);
        let max_time = filtered_packets.last().map(|p| p.time).unwrap_or(1000.0);
        let total_time = max_time - min_time;

        ui.horizontal(|ui| {
            ui.label(format!(
                "{} packets (of {}) | {:.0} - {:.0} ms ({:.1}s)",
                filtered_packets.len(),
                self.cached_packets.len(),
                min_time,
                max_time,
                total_time / 1000.0
            ));
        });
        ui.separator();

        if filtered_packets.is_empty() {
            ui.label("No packets match the current filters");
            return;
        }

        let total_content_height = (filtered_packets.len() as f32 * PACKET_HEIGHT) + 100.0;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                let available_width = ui.available_width();
                let desired_size = Vec2::new(available_width, total_content_height);

                let (response, painter) =
                    ui.allocate_painter(desired_size, Sense::click_and_drag());
                let rect = response.rect;

                let visible_start_y = viewport.top();
                let visible_end_y = viewport.bottom();

                let first_visible_idx = ((visible_start_y - HEADER_HEIGHT) / PACKET_HEIGHT)
                    .floor()
                    .max(0.0) as usize;
                let first_visible_idx = first_visible_idx.saturating_sub(VISIBLE_BUFFER);

                let last_visible_idx = ((visible_end_y - HEADER_HEIGHT) / PACKET_HEIGHT)
                    .ceil()
                    .max(0.0) as usize;
                let last_visible_idx =
                    (last_visible_idx + VISIBLE_BUFFER).min(filtered_packets.len());

                let client_x = rect.left() + LEFT_MARGIN;
                let server_x = rect.right() - RIGHT_MARGIN;
                let top_y = rect.top() + HEADER_HEIGHT;

                let header_y = rect.top() + viewport.top() + 18.0;
                painter.rect_filled(
                    Rect::from_min_size(
                        Pos2::new(rect.left(), rect.top() + viewport.top()),
                        Vec2::new(available_width, 40.0),
                    ),
                    0.0,
                    ui.style().visuals.panel_fill,
                );
                painter.text(
                    Pos2::new(client_x, header_y),
                    egui::Align2::CENTER_CENTER,
                    "Client",
                    egui::FontId::proportional(16.0),
                    Color32::WHITE,
                );
                painter.text(
                    Pos2::new(server_x, header_y),
                    egui::Align2::CENTER_CENTER,
                    "Server",
                    egui::FontId::proportional(16.0),
                    Color32::WHITE,
                );

                let timeline_top = (top_y + first_visible_idx as f32 * PACKET_HEIGHT).max(top_y);
                let timeline_bottom = top_y + last_visible_idx as f32 * PACKET_HEIGHT;
                painter.line_segment(
                    [
                        Pos2::new(client_x, timeline_top),
                        Pos2::new(client_x, timeline_bottom),
                    ],
                    Stroke::new(2.0, Color32::GRAY),
                );
                painter.line_segment(
                    [
                        Pos2::new(server_x, timeline_top),
                        Pos2::new(server_x, timeline_bottom),
                    ],
                    Stroke::new(2.0, Color32::GRAY),
                );

                for idx in first_visible_idx..last_visible_idx {
                    if idx >= filtered_packets.len() {
                        break;
                    }
                    let packet = filtered_packets[idx];
                    let y_start = top_y + (idx as f32 * PACKET_HEIGHT) + PACKET_HEIGHT / 2.0;

                    let diagonal_offset = PACKET_HEIGHT * 0.4;

                    let (start_x, end_x, y_end, color) = match packet.direction {
                        PacketDirection::Sent => {
                            (client_x, server_x, y_start + diagonal_offset, SENT_COLOR)
                        }
                        PacketDirection::Received => (
                            server_x,
                            client_x,
                            y_start + diagonal_offset,
                            RECEIVED_COLOR,
                        ),
                    };

                    let time_line_half_width = 20.0;
                    painter.line_segment(
                        [
                            Pos2::new(start_x - time_line_half_width, y_start),
                            Pos2::new(start_x + time_line_half_width, y_start),
                        ],
                        Stroke::new(1.5, color),
                    );
                    painter.line_segment(
                        [
                            Pos2::new(end_x - time_line_half_width, y_end),
                            Pos2::new(end_x + time_line_half_width, y_end),
                        ],
                        Stroke::new(1.5, color),
                    );

                    painter.line_segment(
                        [Pos2::new(start_x, y_start), Pos2::new(end_x, y_end)],
                        Stroke::new(2.0, color),
                    );

                    painter.circle_filled(Pos2::new(start_x, y_start), 4.0, color);
                    painter.circle_filled(Pos2::new(end_x, y_end), 4.0, color);

                    painter.text(
                        Pos2::new(rect.left() + 8.0, y_start),
                        egui::Align2::LEFT_CENTER,
                        format!("{:.2}", packet.time),
                        egui::FontId::proportional(11.0),
                        Color32::GRAY,
                    );

                    let mid_x = (start_x + end_x) / 2.0;
                    let mid_y = (y_start + y_end) / 2.0;
                    let frames_to_show: Vec<&FrameType> = packet.frames.iter().take(4).collect();
                    let tag_width = 38.0;
                    let tag_gap = 2.0;
                    let total_tags_width = frames_to_show.len() as f32 * (tag_width + tag_gap);
                    let tag_start_x = mid_x - total_tags_width / 2.0;

                    for (fi, frame) in frames_to_show.iter().enumerate() {
                        let tag_x = tag_start_x + (fi as f32 * (tag_width + tag_gap));
                        let tag_rect = Rect::from_min_size(
                            Pos2::new(tag_x, mid_y - 10.0),
                            Vec2::new(tag_width, 14.0),
                        );
                        painter.rect_filled(tag_rect, 2.0, frame.color());
                        painter.text(
                            tag_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            frame.short_name(),
                            egui::FontId::proportional(9.0),
                            Color32::WHITE,
                        );
                    }

                    painter.text(
                        Pos2::new(rect.right() - 10.0, y_end),
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
                    painter.text(
                        Pos2::new(info_x, (y_start + y_end) / 2.0),
                        egui::Align2::LEFT_CENTER,
                        format!("{}:{}", packet.packet_type_short, packet.packet_number),
                        egui::FontId::proportional(10.0),
                        Color32::LIGHT_GRAY,
                    );

                    let arrow_rect = Rect::from_two_pos(
                        Pos2::new(start_x.min(end_x) - 5.0, y_start.min(y_end) - 12.0),
                        Pos2::new(start_x.max(end_x) + 5.0, y_start.max(y_end) + 12.0),
                    );
                    if response.clicked() {
                        if let Some(pos) = response.interact_pointer_pos() {
                            if arrow_rect.contains(pos) {
                                self.selected_packet_idx = Some(idx);
                                *selected_event_idx = Some(packet.event_idx);
                            }
                        }
                    }

                    if self.selected_packet_idx == Some(idx) {
                        painter.rect_stroke(
                            arrow_rect,
                            2.0,
                            Stroke::new(2.0, Color32::YELLOW),
                            StrokeKind::Inside,
                        );
                    }

                    if self.show_loss_markers
                        && packet.direction == PacketDirection::Sent
                        && correlation.is_packet_lost(packet.packet_number)
                    {
                        draw_loss_marker(&painter, Pos2::new(mid_x, mid_y + 18.0), 14.0);
                    }

                    if self.show_rtt && packet.direction == PacketDirection::Sent {
                        if let Some(rtt) = correlation.get_rtt(packet.packet_number) {
                            let rtt_text = format!("{:.1}ms", rtt);
                            painter.text(
                                Pos2::new(mid_x + 5.0, y_start - 2.0),
                                egui::Align2::LEFT_BOTTOM,
                                rtt_text,
                                egui::FontId::proportional(9.0),
                                Color32::from_rgb(100, 200, 100),
                            );
                        }
                    }
                }

                if self.show_reordering {
                    let visible_start = filtered_packets
                        .get(first_visible_idx)
                        .map(|p| p.time as f32)
                        .unwrap_or(0.0);
                    let visible_end = filtered_packets
                        .get(last_visible_idx.saturating_sub(1))
                        .map(|p| p.time as f32)
                        .unwrap_or(f32::MAX);

                    let pn_to_y: HashMap<u64, f32> = (first_visible_idx..last_visible_idx)
                        .filter_map(|idx| filtered_packets.get(idx))
                        .filter(|p| p.direction == PacketDirection::Received)
                        .enumerate()
                        .map(|(i, p)| {
                            (
                                p.packet_number,
                                top_y
                                    + (first_visible_idx + i) as f32 * PACKET_HEIGHT
                                    + PACKET_HEIGHT / 2.0,
                            )
                        })
                        .collect();

                    let reorder_color = Color32::from_rgb(255, 140, 0);
                    let line_x = server_x + 15.0;

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

                let current_packet = first_visible_idx + VISIBLE_BUFFER;
                let progress = current_packet as f32 / filtered_packets.len() as f32 * 100.0;
                painter.text(
                    Pos2::new(rect.right() - 10.0, rect.top() + viewport.top() + 25.0),
                    egui::Align2::RIGHT_CENTER,
                    format!(
                        "#{} ({:.0}%)",
                        current_packet.min(filtered_packets.len()),
                        progress
                    ),
                    egui::FontId::proportional(10.0),
                    Color32::GRAY,
                );
            });
    }

    pub fn show_dual(
        &mut self,
        ui: &mut egui::Ui,
        left_file: &LoadedFile,
        right_file: &LoadedFile,
        selected_event_idx: &mut Option<usize>,
        selected_file_idx: &mut usize,
    ) {
        let left_packets = Self::extract_packets(&left_file.qlog_data).packets;
        let right_packets = Self::extract_packets(&right_file.qlog_data).packets;

        let left_recv_times: HashMap<u64, f64> = left_packets
            .iter()
            .filter(|p| p.direction == PacketDirection::Received)
            .map(|p| (p.packet_number, p.time))
            .collect();
        let right_recv_times: HashMap<u64, f64> = right_packets
            .iter()
            .filter(|p| p.direction == PacketDirection::Received)
            .map(|p| (p.packet_number, p.time))
            .collect();

        struct Arrow {
            send_time: f64,
            recv_time: f64,
            from_left: bool,
            packet_type_short: String,
            packet_number: u64,
            frames: Vec<FrameType>,
            is_lost: bool,
            event_idx: usize,
        }

        let mut arrows: Vec<Arrow> = Vec::new();

        for (packets, recv_times, from_left) in [
            (&left_packets, &right_recv_times, true),
            (&right_packets, &left_recv_times, false),
        ] {
            for p in packets
                .iter()
                .filter(|p| p.direction == PacketDirection::Sent)
            {
                let recv_time = recv_times.get(&p.packet_number).copied();
                arrows.push(Arrow {
                    send_time: p.time,
                    recv_time: recv_time.unwrap_or(p.time),
                    from_left,
                    packet_type_short: p.packet_type_short.clone(),
                    packet_number: p.packet_number,
                    frames: p.frames.clone(),
                    is_lost: recv_time.is_none(),
                    event_idx: p.event_idx,
                });
            }
        }

        arrows.sort_by(|a, b| {
            a.send_time
                .partial_cmp(&b.send_time)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if arrows.is_empty() {
            ui.label("No packet events found in either file");
            return;
        }

        ui.horizontal(|ui| {
            ui.heading("Sequence Diagram (Dual File)");
        });

        let total_loss_count = arrows.iter().filter(|a| a.is_lost).count();

        let min_time = arrows
            .iter()
            .map(|a| a.send_time.min(a.recv_time))
            .fold(f64::MAX, f64::min);
        let max_time = arrows
            .iter()
            .map(|a| a.send_time.max(a.recv_time))
            .fold(f64::MIN, f64::max);
        let time_range = (max_time - min_time).max(1.0);

        ui.horizontal(|ui| {
            ui.label(format!(
                "{} arrows | {:.2} - {:.2} ms ({:.1}s)",
                arrows.len(),
                min_time,
                max_time,
                time_range / 1000.0
            ));

            ui.separator();
            ui.checkbox(&mut self.show_loss_markers, "Show Lost");
            if self.show_loss_markers && total_loss_count > 0 {
                ui.label(format!("({} lost)", total_loss_count));
            }

            ui.separator();
            ui.label("Time Scale:");
            ui.add(
                egui::Slider::new(&mut self.dual_time_scale, 0.5..=100.0)
                    .logarithmic(true)
                    .suffix(" px/ms"),
            );
        });
        ui.separator();

        let pixels_per_ms = self.dual_time_scale;
        let total_content_height = (time_range as f32 * pixels_per_ms) + 100.0;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                let available_width = ui.available_width();
                let desired_size = Vec2::new(available_width, total_content_height);

                let (response, painter) =
                    ui.allocate_painter(desired_size, Sense::click_and_drag());
                let rect = response.rect;

                let top_y_offset = 40.0;
                let left_margin = 60.0;
                let right_margin = 20.0;
                let left_x = rect.left() + left_margin;
                let right_x = rect.right() - right_margin;
                let top_y = rect.top() + top_y_offset;

                let time_to_y = |t: f64| -> f32 { top_y + ((t - min_time) as f32 * pixels_per_ms) };

                let header_y = rect.top() + viewport.top() + 18.0;
                painter.rect_filled(
                    Rect::from_min_size(
                        Pos2::new(rect.left(), rect.top() + viewport.top()),
                        Vec2::new(available_width, 40.0),
                    ),
                    0.0,
                    ui.style().visuals.panel_fill,
                );
                painter.text(
                    Pos2::new(left_x, header_y),
                    egui::Align2::CENTER_CENTER,
                    &left_file.label,
                    egui::FontId::proportional(14.0),
                    Color32::from_rgb(0, 150, 255),
                );
                painter.text(
                    Pos2::new(right_x, header_y),
                    egui::Align2::CENTER_CENTER,
                    &right_file.label,
                    egui::FontId::proportional(14.0),
                    Color32::from_rgb(220, 80, 80),
                );

                let visible_start_time =
                    min_time + ((viewport.top() - top_y_offset).max(0.0) / pixels_per_ms) as f64;
                let visible_end_time =
                    min_time + ((viewport.bottom() - top_y_offset) / pixels_per_ms) as f64;

                let timeline_top = time_to_y(visible_start_time.max(min_time));
                let timeline_bottom = time_to_y(visible_end_time.min(max_time));

                painter.line_segment(
                    [
                        Pos2::new(left_x, timeline_top),
                        Pos2::new(left_x, timeline_bottom),
                    ],
                    Stroke::new(2.0, Color32::GRAY),
                );
                painter.line_segment(
                    [
                        Pos2::new(right_x, timeline_top),
                        Pos2::new(right_x, timeline_bottom),
                    ],
                    Stroke::new(2.0, Color32::GRAY),
                );

                let time_step = if time_range > 1000.0 {
                    100.0
                } else if time_range > 100.0 {
                    10.0
                } else {
                    1.0
                };
                let first_marker = (visible_start_time / time_step).floor() * time_step;
                let mut marker_time = first_marker;
                while marker_time <= visible_end_time {
                    if marker_time >= min_time {
                        let y = time_to_y(marker_time);
                        painter.line_segment(
                            [Pos2::new(left_x - 5.0, y), Pos2::new(left_x, y)],
                            Stroke::new(1.0, Color32::DARK_GRAY),
                        );
                        painter.text(
                            Pos2::new(rect.left() + 5.0, y),
                            egui::Align2::LEFT_CENTER,
                            format!("{:.1}", marker_time),
                            egui::FontId::proportional(9.0),
                            Color32::GRAY,
                        );
                    }
                    marker_time += time_step;
                }

                let mut send_time_counts: HashMap<i64, usize> = HashMap::new();
                let mut send_time_indices: HashMap<i64, usize> = HashMap::new();
                for arrow in &arrows {
                    let key = (arrow.send_time * 1000.0) as i64;
                    *send_time_counts.entry(key).or_insert(0) += 1;
                }

                let mut arrow_rects: Vec<(usize, usize, bool, Rect)> = Vec::new();
                for (arrow_idx, arrow) in arrows.iter().enumerate() {
                    let y_send = time_to_y(arrow.send_time);
                    let y_recv = time_to_y(arrow.recv_time);

                    let (start_x, end_x, color) = if arrow.from_left {
                        (left_x, right_x, SENT_COLOR)
                    } else {
                        (right_x, left_x, RECEIVED_COLOR)
                    };

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

                    painter.line_segment(
                        [
                            Pos2::new(start_x, y_send_adjusted),
                            Pos2::new(actual_end_x, actual_end_y),
                        ],
                        Stroke::new(line_width, line_color),
                    );

                    let marker_size = if is_selected { 5.0 } else { 3.0 };
                    painter.circle_filled(
                        Pos2::new(start_x, y_send_adjusted),
                        marker_size,
                        line_color,
                    );
                    if !is_truncated {
                        painter.circle_filled(
                            Pos2::new(actual_end_x, actual_end_y),
                            marker_size,
                            line_color,
                        );
                    }

                    let tag_x_pos = (start_x + actual_end_x) / 2.0;
                    let tag_y_pos = (y_send_adjusted + actual_end_y) / 2.0;
                    let frames_to_show: Vec<&FrameType> = arrow.frames.iter().take(3).collect();
                    let tag_width = 32.0;
                    let tag_gap = 2.0;
                    let total_tags_width = frames_to_show.len() as f32 * (tag_width + tag_gap);
                    let tag_start_x = tag_x_pos - total_tags_width / 2.0;

                    for (fi, frame) in frames_to_show.iter().enumerate() {
                        let tag_x = tag_start_x + (fi as f32 * (tag_width + tag_gap));
                        let tag_rect = Rect::from_min_size(
                            Pos2::new(tag_x, tag_y_pos - 6.0),
                            Vec2::new(tag_width, 12.0),
                        );
                        painter.rect_filled(tag_rect, 2.0, frame.color());
                        painter.text(
                            tag_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            frame.short_name(),
                            egui::FontId::proportional(8.0),
                            Color32::WHITE,
                        );
                    }

                    let info_offset = if arrow.from_left { 5.0 } else { -5.0 };
                    painter.text(
                        Pos2::new(tag_x_pos + info_offset, tag_y_pos - 10.0),
                        if arrow.from_left {
                            egui::Align2::LEFT_BOTTOM
                        } else {
                            egui::Align2::RIGHT_BOTTOM
                        },
                        format!("{}:{}", arrow.packet_type_short, arrow.packet_number),
                        egui::FontId::proportional(8.0),
                        Color32::LIGHT_GRAY,
                    );

                    if is_truncated {
                        draw_loss_marker(&painter, Pos2::new(actual_end_x, actual_end_y), 10.0);
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
                            format!("×{}", count),
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
                    arrow_rects.push((arrow_idx, arrow.event_idx, arrow.from_left, arrow_rect));
                }

                let total_arrows = arrow_rects.len();

                if response.clicked() {
                    if let Some(pos) = response.interact_pointer_pos() {
                        if let Some((idx, event_idx, from_left, _)) = arrow_rects
                            .iter()
                            .find(|(_, _, _, rect)| rect.contains(pos))
                        {
                            self.selected_packet_idx = Some(*idx);
                            *selected_event_idx = Some(*event_idx);
                            *selected_file_idx = if *from_left { 0 } else { 1 };
                        }
                    }
                }

                if total_arrows > 0 {
                    let input = ui.input(|i| {
                        (
                            i.key_pressed(egui::Key::ArrowUp),
                            i.key_pressed(egui::Key::ArrowDown),
                        )
                    });

                    if input.0 || input.1 {
                        let current_idx = self.selected_packet_idx.unwrap_or(0);
                        let new_idx = if input.0 {
                            current_idx.saturating_sub(1)
                        } else {
                            (current_idx + 1).min(total_arrows - 1)
                        };

                        if new_idx != current_idx || self.selected_packet_idx.is_none() {
                            self.selected_packet_idx = Some(new_idx);
                            if let Some((_, event_idx, from_left, _)) =
                                arrow_rects.iter().find(|(idx, _, _, _)| *idx == new_idx)
                            {
                                *selected_event_idx = Some(*event_idx);
                                *selected_file_idx = if *from_left { 0 } else { 1 };
                            }
                        }
                    }

                    if let Some(idx) = self.selected_packet_idx {
                        painter.text(
                            Pos2::new(rect.left() + 10.0, rect.top() + viewport.top() + 25.0),
                            egui::Align2::LEFT_CENTER,
                            format!("↑↓ arrow {} of {}", idx + 1, total_arrows),
                            egui::FontId::proportional(11.0),
                            Color32::YELLOW,
                        );
                    }
                }

                let visible_time_pct =
                    ((visible_start_time - min_time) / time_range * 100.0) as f32;
                painter.text(
                    Pos2::new(rect.right() - 10.0, rect.top() + viewport.top() + 25.0),
                    egui::Align2::RIGHT_CENTER,
                    format!("{:.0}ms ({:.0}%)", visible_start_time, visible_time_pct),
                    egui::FontId::proportional(10.0),
                    Color32::GRAY,
                );
            });
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

            result.packets.push(PacketInfo {
                time: event.time as f64,
                direction,
                packet_type,
                packet_type_short,
                packet_number: header.packet_number.unwrap_or(0),
                frames,
                stream_ids: pkt_stream_ids,
                event_idx: idx,
            });
        }

        result
    }
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

struct PacketInfo {
    time: f64,
    direction: PacketDirection,
    packet_type: String,
    packet_type_short: String,
    packet_number: u64,
    frames: Vec<FrameType>,
    stream_ids: Vec<u64>,
    event_idx: usize,
}
