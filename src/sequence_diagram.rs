use crate::qlog_data::QlogData;
use crate::utils::{self, FrameType};
use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use qlog::events::EventData;
use std::collections::BTreeSet;

const PACKET_HEIGHT: f32 = 35.0;
const VISIBLE_BUFFER: usize = 10;

pub struct SequenceDiagram {
    selected_packet_idx: Option<usize>,
    cached_packets: Vec<PacketInfo>,
    cache_valid: bool,
    available_stream_ids: Vec<u64>,
    available_packet_types: Vec<String>,
    filter_stream_id: Option<u64>,
    filter_packet_type: Option<String>,
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
        selected_event_idx: &mut Option<usize>,
    ) {
        if !self.cache_valid {
            let (packets, stream_ids, packet_types) =
                Self::extract_packet_data_with_options(qlog_data);
            self.cached_packets = packets;
            self.available_stream_ids = stream_ids;
            self.available_packet_types = packet_types;
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

                let top_y_offset = 40.0;
                let visible_start_y = viewport.top();
                let visible_end_y = viewport.bottom();

                let first_visible_idx = ((visible_start_y - top_y_offset) / PACKET_HEIGHT)
                    .floor()
                    .max(0.0) as usize;
                let first_visible_idx = first_visible_idx.saturating_sub(VISIBLE_BUFFER);

                let last_visible_idx = ((visible_end_y - top_y_offset) / PACKET_HEIGHT)
                    .ceil()
                    .max(0.0) as usize;
                let last_visible_idx =
                    (last_visible_idx + VISIBLE_BUFFER).min(filtered_packets.len());

                let left_margin = 110.0;
                let right_margin = 120.0;
                let client_x = rect.left() + left_margin;
                let server_x = rect.right() - right_margin;
                let top_y = rect.top() + top_y_offset;

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
                    let y = top_y + (idx as f32 * PACKET_HEIGHT) + PACKET_HEIGHT / 2.0;

                    let (start_x, end_x, color) = match packet.direction {
                        PacketDirection::Sent => {
                            (client_x, server_x, Color32::from_rgb(0, 150, 255))
                        }
                        PacketDirection::Received => {
                            (server_x, client_x, Color32::from_rgb(220, 80, 80))
                        }
                    };

                    painter.line_segment(
                        [Pos2::new(start_x, y), Pos2::new(end_x, y + 4.0)],
                        Stroke::new(2.5, color),
                    );

                    let arrow_dir = if start_x < end_x { 1.0 } else { -1.0 };
                    let tip = Pos2::new(end_x, y + 4.0);
                    painter.line_segment(
                        [tip, Pos2::new(end_x - arrow_dir * 12.0, y - 4.0)],
                        Stroke::new(2.5, color),
                    );
                    painter.line_segment(
                        [tip, Pos2::new(end_x - arrow_dir * 12.0, y + 12.0)],
                        Stroke::new(2.5, color),
                    );

                    painter.text(
                        Pos2::new(rect.left() + 8.0, y + 2.0),
                        egui::Align2::LEFT_CENTER,
                        format!("{:.2}", packet.time),
                        egui::FontId::proportional(12.0),
                        Color32::LIGHT_GRAY,
                    );

                    let mid_x = (start_x + end_x) / 2.0;
                    let frames_to_show: Vec<&FrameType> = packet.frames.iter().take(5).collect();
                    let tag_width = 40.0;
                    let tag_gap = 3.0;
                    let total_tags_width = frames_to_show.len() as f32 * (tag_width + tag_gap);
                    let tag_start_x = mid_x - total_tags_width / 2.0;

                    for (fi, frame) in frames_to_show.iter().enumerate() {
                        let tag_x = tag_start_x + (fi as f32 * (tag_width + tag_gap));
                        let tag_rect = Rect::from_min_size(
                            Pos2::new(tag_x, y - 16.0),
                            Vec2::new(tag_width, 18.0),
                        );
                        painter.rect_filled(tag_rect, 3.0, frame.color());
                        painter.text(
                            tag_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            frame.short_name(),
                            egui::FontId::proportional(10.0),
                            Color32::WHITE,
                        );
                    }

                    let info_x = if start_x < end_x {
                        end_x + 12.0
                    } else {
                        start_x + 12.0
                    };
                    painter.text(
                        Pos2::new(info_x, y + 2.0),
                        egui::Align2::LEFT_CENTER,
                        format!("{}:{}", packet.packet_type_short, packet.packet_number),
                        egui::FontId::proportional(12.0),
                        Color32::LIGHT_GRAY,
                    );

                    let arrow_rect = Rect::from_two_pos(
                        Pos2::new(start_x.min(end_x) - 5.0, y - 18.0),
                        Pos2::new(start_x.max(end_x) + 5.0, y + 18.0),
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

    fn extract_packet_data_with_options(
        qlog_data: &QlogData,
    ) -> (Vec<PacketInfo>, Vec<u64>, Vec<String>) {
        let mut packets = Vec::new();
        let mut stream_ids: BTreeSet<u64> = BTreeSet::new();
        let mut packet_types: BTreeSet<String> = BTreeSet::new();

        for (idx, event) in qlog_data.events.iter().enumerate() {
            let time = event.time as f64;

            match &event.data {
                EventData::PacketSent(data) => {
                    let packet_type_raw = format!("{:?}", data.header.packet_type);
                    let packet_type = utils::full_packet_type(&packet_type_raw);
                    let packet_type_short = utils::short_packet_type(&packet_type_raw);
                    let packet_number = data.header.packet_number.unwrap_or(0);

                    let mut pkt_stream_ids: Vec<u64> = Vec::new();
                    let frames: Vec<FrameType> = data
                        .frames
                        .as_ref()
                        .map(|f| {
                            f.iter()
                                .map(|frame| {
                                    if let Some(sid) = utils::get_frame_stream_id(frame) {
                                        pkt_stream_ids.push(sid);
                                        stream_ids.insert(sid);
                                    }
                                    FrameType::from_quic_frame(frame)
                                })
                                .collect()
                        })
                        .unwrap_or_default();

                    packet_types.insert(packet_type.clone());

                    packets.push(PacketInfo {
                        time,
                        direction: PacketDirection::Sent,
                        packet_type,
                        packet_type_short,
                        packet_number,
                        frames,
                        stream_ids: pkt_stream_ids,
                        event_idx: idx,
                    });
                }
                EventData::PacketReceived(data) => {
                    let packet_type_raw = format!("{:?}", data.header.packet_type);
                    let packet_type = utils::full_packet_type(&packet_type_raw);
                    let packet_type_short = utils::short_packet_type(&packet_type_raw);
                    let packet_number = data.header.packet_number.unwrap_or(0);

                    let mut pkt_stream_ids: Vec<u64> = Vec::new();
                    let frames: Vec<FrameType> = data
                        .frames
                        .as_ref()
                        .map(|f| {
                            f.iter()
                                .map(|frame| {
                                    if let Some(sid) = utils::get_frame_stream_id(frame) {
                                        pkt_stream_ids.push(sid);
                                        stream_ids.insert(sid);
                                    }
                                    FrameType::from_quic_frame(frame)
                                })
                                .collect()
                        })
                        .unwrap_or_default();

                    packet_types.insert(packet_type.clone());

                    packets.push(PacketInfo {
                        time,
                        direction: PacketDirection::Received,
                        packet_type,
                        packet_type_short,
                        packet_number,
                        frames,
                        stream_ids: pkt_stream_ids,
                        event_idx: idx,
                    });
                }
                _ => {}
            }
        }

        (
            packets,
            stream_ids.into_iter().collect(),
            packet_types.into_iter().collect(),
        )
    }
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
