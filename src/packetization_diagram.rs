use crate::qlog_data::QlogData;
use crate::utils::{self, FrameType};
use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use qlog::events::{quic::QuicFrame, EventData};

const LANE_HEIGHT: f32 = 30.0;
const TOTAL_LANES: f32 = 4.0;

pub struct PacketizationDiagram {
    // Cached data
    sent_data: Option<ByteStreamData>,
    received_data: Option<ByteStreamData>,
    cache_valid: bool,
    // View state
    compression: f32, // Bytes per pixel (higher = more compressed)
    show_sent: bool,
    show_received: bool,
}

impl PacketizationDiagram {
    pub fn new() -> Self {
        Self {
            sent_data: None,
            received_data: None,
            cache_valid: false,
            compression: 10.0, // 10 bytes per pixel default
            show_sent: true,
            show_received: true,
        }
    }

    pub fn invalidate_cache(&mut self) {
        self.cache_valid = false;
        self.sent_data = None;
        self.received_data = None;
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        qlog_data: &QlogData,
        selected_event_idx: &mut Option<usize>,
    ) {
        // Extract and cache data
        if !self.cache_valid {
            let (sent, received) = Self::extract_byte_stream_data(qlog_data);
            self.sent_data = Some(sent);
            self.received_data = Some(received);
            self.cache_valid = true;
        }

        // Header
        ui.horizontal(|ui| {
            ui.heading("Packetization Diagram");
            ui.separator();
            ui.checkbox(&mut self.show_received, "Received");
            ui.checkbox(&mut self.show_sent, "Sent");
        });

        // Compression slider
        ui.horizontal(|ui| {
            ui.label("Compression:");
            ui.add(
                egui::Slider::new(&mut self.compression, 1.0..=500.0)
                    .logarithmic(true)
                    .text("bytes/px"),
            );
            if ui.button("1:1").clicked() {
                self.compression = 1.0;
            }
            if ui.button("10:1").clicked() {
                self.compression = 10.0;
            }
            if ui.button("100:1").clicked() {
                self.compression = 100.0;
            }
        });
        ui.separator();

        let bytes_per_pixel = self.compression as f64;

        // Show received data
        if self.show_received {
            if let Some(ref data) = self.received_data {
                ui.label(format!(
                    "Bytes received: {} bytes, {} packets",
                    data.total_bytes, data.packet_count
                ));
                self.render_byte_stream(ui, "received", data, bytes_per_pixel, selected_event_idx);
                ui.add_space(10.0);
            }
        }

        // Show sent data
        if self.show_sent {
            if let Some(ref data) = self.sent_data {
                ui.label(format!(
                    "Bytes sent: {} bytes, {} packets",
                    data.total_bytes, data.packet_count
                ));
                self.render_byte_stream(ui, "sent", data, bytes_per_pixel, selected_event_idx);
            }
        }
    }

    fn render_byte_stream(
        &self,
        ui: &mut egui::Ui,
        id: &str,
        data: &ByteStreamData,
        bytes_per_pixel: f64,
        selected_event_idx: &mut Option<usize>,
    ) {
        let content_width = (data.total_bytes as f64 / bytes_per_pixel) as f32 + 100.0;
        let content_height = LANE_HEIGHT * TOTAL_LANES + 40.0;
        let left_margin = 80.0;

        // Add extra height for scrollbar
        let scroll_area_height = content_height + 20.0;

        ui.allocate_ui(Vec2::new(ui.available_width(), scroll_area_height), |ui| {
            egui::ScrollArea::horizontal()
                .id_salt(id)
                .auto_shrink([false, false])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .show_viewport(ui, |ui, viewport| {
                    let (response, painter) = ui.allocate_painter(
                        Vec2::new(content_width, content_height),
                        Sense::click_and_drag(),
                    );
                    let rect = response.rect;

                    // Calculate visible byte range from viewport
                    let visible_start_byte =
                        ((viewport.left() - left_margin).max(0.0) as f64 * bytes_per_pixel) as u64;
                    let visible_end_byte = ((viewport.right()) as f64 * bytes_per_pixel) as u64;

                    // Add buffer for smooth scrolling
                    let buffer_bytes = (bytes_per_pixel * 100.0) as u64;
                    let render_start = visible_start_byte.saturating_sub(buffer_bytes);
                    let render_end = visible_end_byte + buffer_bytes;

                    let top_margin = 5.0;

                    // Lane labels (draw at fixed position relative to viewport)
                    let labels = ["Stream IDs", "HTTP/3", "QUIC frames", "QUIC packets"];
                    let label_x = rect.left() + viewport.left() + 5.0;
                    for (i, label) in labels.iter().enumerate() {
                        let y =
                            rect.top() + top_margin + (i as f32 * LANE_HEIGHT) + LANE_HEIGHT / 2.0;
                        // Background for label
                        painter.rect_filled(
                            Rect::from_min_size(
                                Pos2::new(label_x - 2.0, y - 8.0),
                                Vec2::new(75.0, 16.0),
                            ),
                            0.0,
                            ui.style().visuals.panel_fill,
                        );
                        painter.text(
                            Pos2::new(label_x, y),
                            egui::Align2::LEFT_CENTER,
                            *label,
                            egui::FontId::proportional(10.0),
                            Color32::LIGHT_GRAY,
                        );
                    }

                    let draw_left = rect.left() + left_margin;

                    // Binary search helper to find first packet in range
                    let first_packet_idx = data
                        .packets
                        .binary_search_by(|p| {
                            if p.end_byte < render_start {
                                std::cmp::Ordering::Less
                            } else {
                                std::cmp::Ordering::Greater
                            }
                        })
                        .unwrap_or_else(|i| i);

                    // Track hover info and click selection
                    let hover_pos = response.hover_pos();
                    let mut hover_text: Option<String> = None;
                    let mut clicked_event_idx: Option<usize> = None;
                    let click_pos = if response.clicked() {
                        response.interact_pointer_pos()
                    } else {
                        None
                    };

                    // Lane 3 (bottom): QUIC packets - only render visible ones
                    let lane_y = rect.top() + top_margin + 3.0 * LANE_HEIGHT;
                    for (idx, packet) in data.packets.iter().skip(first_packet_idx).enumerate() {
                        if packet.start_byte > render_end {
                            break;
                        }
                        let x_start =
                            draw_left + (packet.start_byte as f64 / bytes_per_pixel) as f32;
                        let x_end = draw_left + (packet.end_byte as f64 / bytes_per_pixel) as f32;
                        let width = (x_end - x_start).max(1.0);

                        let packet_rect = Rect::from_min_size(
                            Pos2::new(x_start, lane_y + 2.0),
                            Vec2::new(width, LANE_HEIGHT - 4.0),
                        );

                        painter.rect_filled(packet_rect, 1.0, Color32::from_rgb(30, 30, 30));
                        painter.rect_stroke(
                            packet_rect,
                            1.0,
                            Stroke::new(0.5, Color32::BLACK),
                            StrokeKind::Inside,
                        );

                        // Check click
                        if let Some(pos) = click_pos {
                            if packet_rect.contains(pos) && clicked_event_idx.is_none() {
                                clicked_event_idx = Some(packet.event_idx);
                            }
                        }

                        // Check hover
                        if let Some(pos) = hover_pos {
                            if packet_rect.contains(pos) {
                                hover_text = Some(format!(
                                    "QUIC Packet #{} : size {}\nPacket type: {}, Packet nr: {}",
                                    first_packet_idx + idx,
                                    packet.size,
                                    packet.packet_type,
                                    packet.packet_number
                                ));
                                // Highlight on hover
                                painter.rect_stroke(
                                    packet_rect,
                                    1.0,
                                    Stroke::new(2.0, Color32::WHITE),
                                    StrokeKind::Inside,
                                );
                            }
                        }
                    }

                    // Binary search for first frame in range
                    let first_frame_idx = data
                        .frames
                        .binary_search_by(|f| {
                            if f.end_byte < render_start {
                                std::cmp::Ordering::Less
                            } else {
                                std::cmp::Ordering::Greater
                            }
                        })
                        .unwrap_or_else(|i| i);

                    // Lane 2: QUIC frames - only render visible ones
                    let lane_y = rect.top() + top_margin + 2.0 * LANE_HEIGHT;
                    for frame in data.frames.iter().skip(first_frame_idx) {
                        if frame.start_byte > render_end {
                            break;
                        }
                        let x_start =
                            draw_left + (frame.start_byte as f64 / bytes_per_pixel) as f32;
                        let x_end = draw_left + (frame.end_byte as f64 / bytes_per_pixel) as f32;
                        let width = (x_end - x_start).max(1.0);
                        let color = frame.frame_type.packetization_color();

                        let frame_rect = Rect::from_min_size(
                            Pos2::new(x_start, lane_y + 2.0),
                            Vec2::new(width, LANE_HEIGHT - 4.0),
                        );

                        painter.rect_filled(frame_rect, 1.0, color);

                        // Check click for frames
                        if let Some(pos) = click_pos {
                            if frame_rect.contains(pos) && clicked_event_idx.is_none() {
                                clicked_event_idx = Some(frame.event_idx);
                            }
                        }

                        // Check hover for frames
                        if hover_text.is_none() {
                            if let Some(pos) = hover_pos {
                                if frame_rect.contains(pos) {
                                    let stream_info = frame
                                        .stream_id
                                        .map(|id| format!(", Stream: {}", id))
                                        .unwrap_or_default();
                                    hover_text = Some(format!(
                                        "QUIC Frame: {}\nSize: {} bytes{}",
                                        frame.frame_type.display_name(),
                                        frame.size,
                                        stream_info
                                    ));
                                    painter.rect_stroke(
                                        frame_rect,
                                        1.0,
                                        Stroke::new(2.0, Color32::WHITE),
                                        StrokeKind::Inside,
                                    );
                                }
                            }
                        }
                    }

                    // Lane 1: HTTP/3 - only render visible stream frames
                    let lane_y = rect.top() + top_margin + 1.0 * LANE_HEIGHT;
                    for frame in data.frames.iter().skip(first_frame_idx) {
                        if frame.start_byte > render_end {
                            break;
                        }
                        if frame.frame_type.is_stream() {
                            let x_start =
                                draw_left + (frame.start_byte as f64 / bytes_per_pixel) as f32;
                            let x_end =
                                draw_left + (frame.end_byte as f64 / bytes_per_pixel) as f32;
                            let width = (x_end - x_start).max(1.0);

                            let http3_rect = Rect::from_min_size(
                                Pos2::new(x_start, lane_y + 2.0),
                                Vec2::new(width, LANE_HEIGHT - 4.0),
                            );

                            painter.rect_filled(http3_rect, 1.0, Color32::from_rgb(255, 255, 0));

                            // Check click for HTTP/3 layer
                            if let Some(pos) = click_pos {
                                if http3_rect.contains(pos) && clicked_event_idx.is_none() {
                                    clicked_event_idx = Some(frame.event_idx);
                                }
                            }

                            // Check hover for HTTP/3 layer
                            if hover_text.is_none() {
                                if let Some(pos) = hover_pos {
                                    if http3_rect.contains(pos) {
                                        let stream_id = frame.stream_id.unwrap_or(0);
                                        hover_text = Some(format!(
                                            "HTTP/3 Data\nStream: {}, Size: {} bytes",
                                            stream_id, frame.size
                                        ));
                                        painter.rect_stroke(
                                            http3_rect,
                                            1.0,
                                            Stroke::new(2.0, Color32::BLACK),
                                            StrokeKind::Inside,
                                        );
                                    }
                                }
                            }
                        }
                    }

                    // Lane 0 (top): Stream IDs - filter by visible range
                    let lane_y = rect.top() + top_margin;
                    for (stream_id, ranges) in &data.stream_ranges {
                        let color = utils::stream_color(*stream_id);
                        for (start, end, evt_idx) in ranges {
                            // Skip if outside visible range
                            if *end < render_start || *start > render_end {
                                continue;
                            }
                            let x_start = draw_left + (*start as f64 / bytes_per_pixel) as f32;
                            let x_end = draw_left + (*end as f64 / bytes_per_pixel) as f32;
                            let width = (x_end - x_start).max(1.0);
                            let size = end - start;

                            let stream_rect = Rect::from_min_size(
                                Pos2::new(x_start, lane_y + 2.0),
                                Vec2::new(width, LANE_HEIGHT - 4.0),
                            );

                            painter.rect_filled(stream_rect, 1.0, color);

                            // Check click for Stream IDs
                            if let Some(pos) = click_pos {
                                if stream_rect.contains(pos) && clicked_event_idx.is_none() {
                                    clicked_event_idx = Some(*evt_idx);
                                }
                            }

                            // Check hover for Stream IDs
                            if hover_text.is_none() {
                                if let Some(pos) = hover_pos {
                                    if stream_rect.contains(pos) {
                                        hover_text = Some(format!(
                                            "Stream ID: {}\nSize: {} bytes",
                                            stream_id, size
                                        ));
                                        painter.rect_stroke(
                                            stream_rect,
                                            1.0,
                                            Stroke::new(2.0, Color32::WHITE),
                                            StrokeKind::Inside,
                                        );
                                    }
                                }
                            }
                        }
                    }

                    // X-axis labels - only render visible ones
                    let axis_y = rect.top() + top_margin + TOTAL_LANES * LANE_HEIGHT + 5.0;
                    let label_interval = (1000.0 * bytes_per_pixel / 50.0).max(500.0) as u64;
                    let start_label = (render_start / label_interval) * label_interval;
                    let mut byte_pos = start_label;
                    while byte_pos <= render_end.min(data.total_bytes) {
                        let x = draw_left + (byte_pos as f64 / bytes_per_pixel) as f32;
                        painter.text(
                            Pos2::new(x, axis_y),
                            egui::Align2::CENTER_TOP,
                            format!("{}", byte_pos),
                            egui::FontId::proportional(9.0),
                            Color32::GRAY,
                        );
                        painter.line_segment(
                            [Pos2::new(x, axis_y - 3.0), Pos2::new(x, axis_y)],
                            Stroke::new(1.0, Color32::GRAY),
                        );
                        byte_pos += label_interval;
                    }

                    // Progress indicator
                    let progress =
                        visible_start_byte as f32 / data.total_bytes.max(1) as f32 * 100.0;
                    painter.text(
                        Pos2::new(rect.left() + viewport.right() - 10.0, rect.top() + 10.0),
                        egui::Align2::RIGHT_TOP,
                        format!("{:.0}%", progress),
                        egui::FontId::proportional(10.0),
                        Color32::GRAY,
                    );

                    // Apply click selection
                    if let Some(evt_idx) = clicked_event_idx {
                        *selected_event_idx = Some(evt_idx);
                    }

                    // Show tooltip on hover
                    if let Some(text) = hover_text {
                        if let Some(pos) = hover_pos {
                            let tooltip_rect = Rect::from_min_size(
                                Pos2::new(pos.x + 10.0, pos.y - 50.0),
                                Vec2::new(250.0, 45.0),
                            );
                            painter.rect_filled(
                                tooltip_rect,
                                4.0,
                                Color32::from_rgba_unmultiplied(60, 60, 80, 240),
                            );
                            painter.rect_stroke(
                                tooltip_rect,
                                4.0,
                                Stroke::new(1.0, Color32::GRAY),
                                StrokeKind::Inside,
                            );

                            let lines: Vec<&str> = text.split('\n').collect();
                            for (i, line) in lines.iter().enumerate() {
                                painter.text(
                                    Pos2::new(
                                        tooltip_rect.left() + 8.0,
                                        tooltip_rect.top() + 12.0 + (i as f32 * 16.0),
                                    ),
                                    egui::Align2::LEFT_CENTER,
                                    *line,
                                    egui::FontId::proportional(12.0),
                                    Color32::WHITE,
                                );
                            }
                        }
                    }
                });
        });
    }

    fn extract_byte_stream_data(qlog_data: &QlogData) -> (ByteStreamData, ByteStreamData) {
        let mut sent = ByteStreamData::new();
        let mut received = ByteStreamData::new();

        let mut sent_offset: u64 = 0;
        let mut recv_offset: u64 = 0;

        for (event_idx, event) in qlog_data.events.iter().enumerate() {
            match &event.data {
                EventData::PacketSent(data) => {
                    let size = data.raw.as_ref().and_then(|r| r.length).unwrap_or(1200);
                    let start = sent_offset;
                    let end = sent_offset + size;
                    let packet_number = data.header.packet_number.unwrap_or(0);
                    let packet_type = format!("{:?}", data.header.packet_type);

                    sent.packets.push(PacketRange {
                        start_byte: start,
                        end_byte: end,
                        packet_number,
                        packet_type,
                        size,
                        event_idx,
                    });
                    sent.packet_count += 1;

                    // Extract frames
                    if let Some(ref frames) = data.frames {
                        let frame_count = frames.len();
                        let frame_size = if frame_count > 0 {
                            size / frame_count as u64
                        } else {
                            size
                        };
                        let mut frame_offset = start;

                        for frame in frames.iter() {
                            let actual_size = utils::get_frame_size(frame).unwrap_or(frame_size);
                            let frame_end = (frame_offset + actual_size).min(end);

                            sent.frames.push(FrameRange {
                                start_byte: frame_offset,
                                end_byte: frame_end,
                                frame_type: FrameType::from_quic_frame(frame),
                                size: actual_size,
                                stream_id: utils::get_frame_stream_id(frame),
                                event_idx,
                            });

                            if let QuicFrame::Stream {
                                stream_id, length, ..
                            } = frame
                            {
                                sent.stream_ranges.entry(*stream_id).or_default().push((
                                    frame_offset,
                                    frame_offset + *length,
                                    event_idx,
                                ));
                            }

                            frame_offset = frame_end;
                        }
                    }

                    sent_offset = end;
                    sent.total_bytes = end;
                }
                EventData::PacketReceived(data) => {
                    let size = data.raw.as_ref().and_then(|r| r.length).unwrap_or(1200);
                    let start = recv_offset;
                    let end = recv_offset + size;
                    let packet_number = data.header.packet_number.unwrap_or(0);
                    let packet_type = format!("{:?}", data.header.packet_type);

                    received.packets.push(PacketRange {
                        start_byte: start,
                        end_byte: end,
                        packet_number,
                        packet_type,
                        size,
                        event_idx,
                    });
                    received.packet_count += 1;

                    // Extract frames
                    if let Some(ref frames) = data.frames {
                        let frame_count = frames.len();
                        let frame_size = if frame_count > 0 {
                            size / frame_count as u64
                        } else {
                            size
                        };
                        let mut frame_offset = start;

                        for frame in frames.iter() {
                            let actual_size = utils::get_frame_size(frame).unwrap_or(frame_size);
                            let frame_end = (frame_offset + actual_size).min(end);

                            received.frames.push(FrameRange {
                                start_byte: frame_offset,
                                end_byte: frame_end,
                                frame_type: FrameType::from_quic_frame(frame),
                                size: actual_size,
                                stream_id: utils::get_frame_stream_id(frame),
                                event_idx,
                            });

                            if let QuicFrame::Stream {
                                stream_id, length, ..
                            } = frame
                            {
                                received.stream_ranges.entry(*stream_id).or_default().push((
                                    frame_offset,
                                    frame_offset + *length,
                                    event_idx,
                                ));
                            }

                            frame_offset = frame_end;
                        }
                    }

                    recv_offset = end;
                    received.total_bytes = end;
                }
                _ => {}
            }
        }

        (sent, received)
    }
}

struct ByteStreamData {
    packets: Vec<PacketRange>,
    frames: Vec<FrameRange>,
    stream_ranges: std::collections::HashMap<u64, Vec<(u64, u64, usize)>>, // (start, end, event_idx)
    total_bytes: u64,
    packet_count: usize,
}

impl ByteStreamData {
    fn new() -> Self {
        Self {
            packets: Vec::new(),
            frames: Vec::new(),
            stream_ranges: std::collections::HashMap::new(),
            total_bytes: 0,
            packet_count: 0,
        }
    }
}

struct PacketRange {
    start_byte: u64,
    end_byte: u64,
    packet_number: u64,
    packet_type: String,
    size: u64,
    event_idx: usize,
}

struct FrameRange {
    start_byte: u64,
    end_byte: u64,
    frame_type: FrameType,
    size: u64,
    stream_id: Option<u64>,
    event_idx: usize,
}
