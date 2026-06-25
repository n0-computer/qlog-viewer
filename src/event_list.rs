use crate::qlog_data::QlogData;
use crate::utils;
use egui::RichText;
use qlog::events::{quic::QuicFrame, Event, EventData};

pub struct EventListFilterOptions<'a> {
    pub available_stream_ids: &'a [u64],
    pub available_packet_types: &'a [String],
    pub available_path_ids: &'a [u64],
}

pub struct EventListFilters {
    pub text: String,
    pub stream_id: Option<u64>,
    pub packet_type: Option<String>,
    pub path_id: Option<u64>,
    pub show_packets: bool,
    pub show_metrics: bool,
    pub show_timers: bool,
    pub show_connection: bool,
    pub show_recovery: bool,
    pub show_streams: bool,
}

impl Default for EventListFilters {
    fn default() -> Self {
        Self {
            text: String::new(),
            stream_id: None,
            packet_type: None,
            path_id: None,
            show_packets: true,
            show_metrics: true,
            show_timers: true,
            show_connection: true,
            show_recovery: true,
            show_streams: true,
        }
    }
}

pub fn render_event_list(
    ui: &mut egui::Ui,
    data: &QlogData,
    filters: &mut EventListFilters,
    selected_event_idx: &mut Option<usize>,
    options: EventListFilterOptions<'_>,
) {
    // First row: text filter, packet type, stream ID, path ID
    ui.horizontal(|ui| {
        ui.label("🔍");
        ui.add(egui::TextEdit::singleline(&mut filters.text).desired_width(100.0));

        ui.separator();

        ui.label("Type:");
        let current_type_label = filters.packet_type.as_deref().unwrap_or("All");
        egui::ComboBox::from_id_salt("event_list_packet_type_filter")
            .selected_text(current_type_label)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(filters.packet_type.is_none(), "All")
                    .clicked()
                {
                    filters.packet_type = None;
                }
                for ptype in options.available_packet_types {
                    let selected = filters.packet_type.as_ref() == Some(ptype);
                    if ui.selectable_label(selected, ptype).clicked() {
                        filters.packet_type = Some(ptype.clone());
                    }
                }
            });

        ui.label("Stream:");
        let current_stream_label = filters
            .stream_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "All".to_string());
        egui::ComboBox::from_id_salt("event_list_stream_id_filter")
            .selected_text(&current_stream_label)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(filters.stream_id.is_none(), "All")
                    .clicked()
                {
                    filters.stream_id = None;
                }
                for &stream_id in options.available_stream_ids {
                    let selected = filters.stream_id == Some(stream_id);
                    if ui
                        .selectable_label(selected, stream_id.to_string())
                        .clicked()
                    {
                        filters.stream_id = Some(stream_id);
                    }
                }
            });

        if !options.available_path_ids.is_empty() {
            ui.label("Path:");
            let current_path_label = filters
                .path_id
                .map(|id| format!("{}", id))
                .unwrap_or_else(|| "All".to_string());
            egui::ComboBox::from_id_salt("event_list_path_id_filter")
                .selected_text(&current_path_label)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(filters.path_id.is_none(), "All")
                        .clicked()
                    {
                        filters.path_id = None;
                    }
                    for &path_id in options.available_path_ids {
                        let selected = filters.path_id == Some(path_id);
                        if ui.selectable_label(selected, path_id.to_string()).clicked() {
                            filters.path_id = Some(path_id);
                        }
                    }
                });
        }

        if ui.button("Clear").clicked() {
            filters.text.clear();
            filters.stream_id = None;
            filters.packet_type = None;
            filters.path_id = None;
        }
    });

    // Second row: category toggles
    ui.horizontal(|ui| {
        ui.toggle_value(&mut filters.show_packets, "Packets");
        ui.toggle_value(&mut filters.show_metrics, "Metrics");
        ui.toggle_value(&mut filters.show_timers, "Timers");
        ui.toggle_value(&mut filters.show_connection, "Connection");
        ui.toggle_value(&mut filters.show_recovery, "Recovery");
        ui.toggle_value(&mut filters.show_streams, "Streams");
    });

    ui.separator();

    let filter_text_lower = filters.text.to_lowercase();

    let filtered_events: Vec<(usize, &Event)> = data
        .events
        .iter()
        .enumerate()
        .filter(|(_, event)| {
            // Text filter
            if !filter_text_lower.is_empty() {
                let event_name = data.get_event_name(event);
                if !event_name.to_lowercase().contains(&filter_text_lower) {
                    return false;
                }
            }

            // Category filter
            let category = get_event_category(event);
            let category_ok = match category {
                "packet" => filters.show_packets,
                "metrics" => filters.show_metrics,
                "timer" => filters.show_timers,
                "connection" => filters.show_connection,
                "recovery" => filters.show_recovery,
                "stream" => filters.show_streams,
                _ => true,
            };
            if !category_ok {
                return false;
            }

            // Stream ID filter
            if let Some(stream_id) = filters.stream_id {
                let event_stream = get_event_stream_id(event);
                if event_stream != Some(stream_id) {
                    return false;
                }
            }

            // Packet type filter
            if let Some(ref pkt_type) = filters.packet_type {
                let event_pkt_type = get_event_packet_type(event);
                if event_pkt_type.as_ref() != Some(pkt_type) {
                    return false;
                }
            }

            // Path ID filter
            if let Some(path_id) = filters.path_id {
                let event_path = get_event_path_id(event);
                if event_path != Some(path_id) {
                    return false;
                }
            }

            true
        })
        .collect();

    let current_selection = *selected_event_idx;
    let mut new_selection = current_selection;

    // Handle arrow key navigation
    let (up_pressed, down_pressed) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::ArrowDown),
        )
    });

    if up_pressed || down_pressed {
        if let Some(current_idx) = current_selection {
            if let Some(current_pos) = filtered_events
                .iter()
                .position(|(idx, _)| *idx == current_idx)
            {
                if up_pressed && current_pos > 0 {
                    new_selection = Some(filtered_events[current_pos - 1].0);
                } else if down_pressed && current_pos + 1 < filtered_events.len() {
                    new_selection = Some(filtered_events[current_pos + 1].0);
                }
            }
        } else if !filtered_events.is_empty() && down_pressed {
            new_selection = Some(filtered_events[0].0);
        }
    }

    // Sticky header with same column widths as content
    let row_height = 22.0;
    let font_size = 13.0;
    let col_widths = [50.0, 80.0, 60.0, 220.0]; // #, Time, Delta, Event, then Summary

    let (header_rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_height),
        egui::Sense::hover(),
    );
    let header_color = egui::Color32::GRAY;
    let mut x = header_rect.min.x + 4.0;
    let y = header_rect.center().y;

    ui.painter().text(
        egui::pos2(x, y),
        egui::Align2::LEFT_CENTER,
        "#",
        egui::FontId::monospace(font_size),
        header_color,
    );
    x += col_widths[0];
    ui.painter().text(
        egui::pos2(x, y),
        egui::Align2::LEFT_CENTER,
        "Time",
        egui::FontId::proportional(font_size),
        header_color,
    );
    x += col_widths[1];
    ui.painter().text(
        egui::pos2(x, y),
        egui::Align2::LEFT_CENTER,
        "Δ",
        egui::FontId::proportional(font_size),
        header_color,
    );
    x += col_widths[2];
    ui.painter().text(
        egui::pos2(x, y),
        egui::Align2::LEFT_CENTER,
        "Event",
        egui::FontId::proportional(font_size),
        header_color,
    );
    x += col_widths[3];
    ui.painter().text(
        egui::pos2(x, y),
        egui::Align2::LEFT_CENTER,
        "Summary",
        egui::FontId::proportional(font_size),
        header_color,
    );

    // Status line showing filtered count
    let shown = filtered_events.len();
    let total = data.events.len();
    ui.horizontal(|ui| {
        if shown < total {
            ui.label(
                RichText::new(format!(
                    "Showing {} of {} events ({} filtered out)",
                    shown,
                    total,
                    total - shown
                ))
                .weak()
                .size(11.0),
            );
        } else {
            ui.label(
                RichText::new(format!("Showing all {} events", total))
                    .weak()
                    .size(11.0),
            );
        }
    });

    ui.separator();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, row_height, filtered_events.len(), |ui, row_range| {
            let mut prev_time: Option<f64> = if row_range.start > 0 {
                filtered_events
                    .get(row_range.start - 1)
                    .map(|(_, e)| e.time)
            } else {
                None
            };

            for row_idx in row_range.clone() {
                if let Some((event_idx, event)) = filtered_events.get(row_idx) {
                    let is_selected = current_selection == Some(*event_idx);

                    let category_color = get_event_row_color(event);

                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), row_height),
                        egui::Sense::click(),
                    );

                    // Draw category color background
                    ui.painter().rect_filled(rect, 0.0, category_color);

                    // Hover/selection highlight
                    if is_selected {
                        ui.painter().rect_filled(
                            rect,
                            0.0,
                            egui::Color32::from_rgba_unmultiplied(100, 150, 255, 60),
                        );
                    } else if response.hovered() {
                        ui.painter().rect_filled(
                            rect,
                            0.0,
                            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 20),
                        );
                    }

                    if response.clicked() {
                        new_selection = Some(*event_idx);
                    }

                    // Delta time
                    let delta_str = if let Some(prev) = prev_time {
                        let delta = event.time - prev;
                        if delta < 0.001 {
                            "+0".into()
                        } else {
                            format!("+{:.2}", delta)
                        }
                    } else {
                        "-".into()
                    };
                    prev_time = Some(event.time);

                    // Draw text content
                    let mut x = rect.min.x + 4.0;
                    let y = rect.center().y;
                    let text_color = if is_selected {
                        egui::Color32::WHITE
                    } else {
                        egui::Color32::LIGHT_GRAY
                    };

                    // Column 1: Index
                    ui.painter().text(
                        egui::pos2(x, y),
                        egui::Align2::LEFT_CENTER,
                        format!("{}", event_idx),
                        egui::FontId::monospace(font_size),
                        text_color,
                    );
                    x += col_widths[0];

                    // Column 2: Time
                    ui.painter().text(
                        egui::pos2(x, y),
                        egui::Align2::LEFT_CENTER,
                        data.format_time(event),
                        egui::FontId::proportional(font_size),
                        text_color,
                    );
                    x += col_widths[1];

                    // Column 3: Delta
                    ui.painter().text(
                        egui::pos2(x, y),
                        egui::Align2::LEFT_CENTER,
                        &delta_str,
                        egui::FontId::proportional(font_size),
                        egui::Color32::GRAY,
                    );
                    x += col_widths[2];

                    // Column 4: Event name
                    ui.painter().text(
                        egui::pos2(x, y),
                        egui::Align2::LEFT_CENTER,
                        data.get_event_name(event),
                        egui::FontId::proportional(font_size),
                        text_color,
                    );
                    x += col_widths[3];

                    // Column 5: Summary
                    ui.painter().text(
                        egui::pos2(x, y),
                        egui::Align2::LEFT_CENTER,
                        data.get_event_summary(event),
                        egui::FontId::proportional(font_size),
                        text_color,
                    );
                }
            }
        });

    *selected_event_idx = new_selection;
}

pub fn get_event_packet_type(event: &Event) -> Option<String> {
    match &event.data {
        EventData::QuicPacketSent(d) => Some(utils::full_packet_type(&format!(
            "{:?}",
            d.header.packet_type
        ))),
        EventData::QuicPacketReceived(d) => Some(utils::full_packet_type(&format!(
            "{:?}",
            d.header.packet_type
        ))),
        _ => None,
    }
}

pub fn get_event_stream_id(event: &Event) -> Option<u64> {
    match &event.data {
        EventData::QuicStreamStateUpdated(d) => Some(d.stream_id),
        EventData::QuicFramesProcessed(d) => {
            for frame in d.frames.iter() {
                if let QuicFrame::Stream { stream_id, .. } = frame {
                    return Some(*stream_id);
                }
            }
            None
        }
        _ => None,
    }
}

pub fn get_event_path_id(event: &Event) -> Option<u64> {
    match &event.data {
        EventData::QuicPacketSent(d) => d.header.path_id,
        EventData::QuicPacketReceived(d) => d.header.path_id,
        EventData::QuicPacketLost(d) => d.header.as_ref().and_then(|h| h.path_id),
        _ => None,
    }
}

pub fn get_event_category(event: &Event) -> &'static str {
    match &event.data {
        EventData::QuicPacketSent(_)
        | EventData::QuicPacketReceived(_)
        | EventData::QuicPacketLost(_)
        | EventData::QuicPacketsAcked(_) => "packet",
        EventData::QuicMetricsUpdated(_) | EventData::QuicCongestionStateUpdated(_) => "metrics",
        EventData::QuicTimerUpdated(_) => "timer",
        EventData::QuicConnectionStarted(_)
        | EventData::QuicConnectionStateUpdated(_)
        | EventData::QuicConnectionClosed(_)
        | EventData::QuicTupleAssigned(_) => "connection",
        EventData::QuicRecoveryParametersSet(_)
        | EventData::QuicParametersRestored(_)
        | EventData::QuicParametersSet(_)
        | EventData::QuicEcnStateUpdated(_) => "recovery",
        EventData::QuicStreamStateUpdated(_) | EventData::QuicFramesProcessed(_) => "stream",
        _ => "other",
    }
}

pub fn get_event_row_color(event: &Event) -> egui::Color32 {
    match get_event_category(event) {
        "packet" => egui::Color32::from_rgba_unmultiplied(52, 152, 219, 20),
        "metrics" => egui::Color32::from_rgba_unmultiplied(142, 68, 173, 20),
        "timer" => egui::Color32::from_rgba_unmultiplied(158, 158, 158, 15),
        "connection" => egui::Color32::from_rgba_unmultiplied(0, 150, 136, 20),
        "recovery" => egui::Color32::from_rgba_unmultiplied(255, 152, 0, 20),
        "stream" => egui::Color32::from_rgba_unmultiplied(231, 76, 60, 20),
        _ => egui::Color32::TRANSPARENT,
    }
}
