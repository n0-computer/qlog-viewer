use crate::congestion_graph::CongestionGraph;
use crate::event_detail_ui::{
    render_connection_started, render_ecn_state_updated, render_frame_with_prefix, render_header,
    render_parameters_restored, render_recovery_metrics_updated, render_recovery_parameters,
    render_section_header, render_timer_updated, render_transport_parameters,
    render_tuple_assigned,
};
use crate::multiplexing_diagram::MultiplexingDiagram;
use crate::packet_correlation::PacketCorrelation;
use crate::packetization_diagram::PacketizationDiagram;
use crate::qlog_data::QlogData;
use crate::sequence_diagram::SequenceDiagram;
use crate::stats_view::StatsView;
use crate::types::{CachingVisualization, LoadedFile};
use crate::utils;
use egui::{CentralPanel, CollapsingHeader, Context, RichText, SidePanel, TopBottomPanel};
use qlog::events::{quic::QuicFrame, Event, EventData};
use std::collections::BTreeSet;
use std::path::PathBuf;
use tracing::{error, info};

#[derive(PartialEq, Clone, Copy)]
enum ViewMode {
    EventList,
    SequenceDiagram,
    CongestionGraph,
    #[allow(dead_code)]
    MultiplexingDiagram,
    PacketizationDiagram,
    StatsView,
}

struct EventListFilterOptions<'a> {
    available_stream_ids: &'a [u64],
    available_packet_types: &'a [String],
    available_path_ids: &'a [u64],
}

struct EventListFilters {
    text: String,
    stream_id: Option<u64>,
    packet_type: Option<String>,
    path_id: Option<u64>,
    show_packets: bool,
    show_metrics: bool,
    show_timers: bool,
    show_connection: bool,
    show_recovery: bool,
    show_streams: bool,
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

pub struct QlogViewerApp {
    loaded_files: Vec<LoadedFile>,
    selected_file_idx: usize,
    loading: bool,
    error_message: Option<String>,
    selected_event_idx: Option<usize>,
    recv_selected_event_idx: Option<usize>,
    filters: EventListFilters,
    available_path_ids: Vec<u64>,
    show_event_detail: bool,
    view_mode: ViewMode,
    sequence_diagram: SequenceDiagram,
    congestion_graph: CongestionGraph,
    multiplexing_diagram: MultiplexingDiagram,
    packetization_diagram: PacketizationDiagram,
    stats_view: StatsView,
}

impl QlogViewerApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, initial_files: Vec<PathBuf>) -> Self {
        let mut app = Self {
            loaded_files: Vec::new(),
            selected_file_idx: 0,
            loading: false,
            error_message: None,
            selected_event_idx: None,
            recv_selected_event_idx: None,
            filters: EventListFilters::default(),
            available_path_ids: Vec::new(),
            show_event_detail: true,
            view_mode: ViewMode::EventList,
            sequence_diagram: SequenceDiagram::default(),
            congestion_graph: CongestionGraph::new(),
            multiplexing_diagram: MultiplexingDiagram::new(),
            packetization_diagram: PacketizationDiagram::new(),
            stats_view: StatsView::new(),
        };

        for path in initial_files {
            app.load_file(path);
        }

        app
    }

    fn selected_file(&self) -> Option<&LoadedFile> {
        self.loaded_files.get(self.selected_file_idx)
    }

    fn invalidate_all_caches(&mut self) {
        self.sequence_diagram.invalidate_cache();
        self.packetization_diagram.invalidate_cache();
    }

    fn render_menu_bar(&mut self, ctx: &Context) {
        TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open...").clicked() {
                        if let Some(paths) = rfd::FileDialog::new()
                            .add_filter("qlog files", &["qlog", "json", "sqlog"])
                            .pick_files()
                        {
                            for path in paths {
                                self.load_file(path);
                            }
                        }
                        ui.close();
                    }

                    if !self.loaded_files.is_empty() {
                        ui.separator();
                        ui.menu_button("Close File", |ui| {
                            let mut file_to_close = None;
                            for (idx, file) in self.loaded_files.iter().enumerate() {
                                if ui.button(&file.label).clicked() {
                                    file_to_close = Some(idx);
                                    ui.close();
                                }
                            }
                            if let Some(idx) = file_to_close {
                                self.close_file(idx);
                            }
                        });
                        if ui.button("Close All").clicked() {
                            self.loaded_files.clear();
                            self.selected_file_idx = 0;
                            self.selected_event_idx = None;
                            self.sequence_diagram.invalidate_cache();
                            self.packetization_diagram.invalidate_cache();
                            ui.close();
                        }
                    }

                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("View", |ui| {
                    if ui
                        .radio(self.view_mode == ViewMode::EventList, "Event List")
                        .clicked()
                    {
                        self.view_mode = ViewMode::EventList;
                    }
                    if ui
                        .radio(
                            self.view_mode == ViewMode::SequenceDiagram,
                            "Sequence Diagram",
                        )
                        .clicked()
                    {
                        self.view_mode = ViewMode::SequenceDiagram;
                    }
                    if ui
                        .radio(
                            self.view_mode == ViewMode::CongestionGraph,
                            "Congestion Graph",
                        )
                        .clicked()
                    {
                        self.view_mode = ViewMode::CongestionGraph;
                    }
                    if ui
                        .radio(
                            self.view_mode == ViewMode::PacketizationDiagram,
                            "Packetization Diagram",
                        )
                        .clicked()
                    {
                        self.view_mode = ViewMode::PacketizationDiagram;
                    }
                    if ui
                        .radio(self.view_mode == ViewMode::StatsView, "Statistics")
                        .clicked()
                    {
                        self.view_mode = ViewMode::StatsView;
                    }
                    ui.separator();
                    ui.checkbox(&mut self.show_event_detail, "Show Event Detail Panel");
                });

                ui.separator();

                if self.loaded_files.len() > 1 && self.view_mode != ViewMode::SequenceDiagram {
                    ui.label("File:");
                    let current_label = self
                        .selected_file()
                        .map(|f| f.label.as_str())
                        .unwrap_or("None");
                    egui::ComboBox::from_id_salt("file_selector")
                        .selected_text(current_label)
                        .show_ui(ui, |ui| {
                            for (idx, file) in self.loaded_files.iter().enumerate() {
                                let selected = self.selected_file_idx == idx;
                                if ui.selectable_label(selected, &file.label).clicked() {
                                    self.selected_file_idx = idx;
                                    self.selected_event_idx = None;
                                    self.sequence_diagram.invalidate_cache();
                                    self.packetization_diagram.invalidate_cache();
                                    self.stats_view
                                        .update_stats(&file.qlog_data, &file.packet_correlation);
                                }
                            }
                        });
                    ui.separator();
                }

                if let Some(file) = self.selected_file() {
                    ui.label(format!("📁 {}", file.path.display()));
                    ui.separator();
                    ui.label(format!("Events: {}", file.qlog_data.events.len()));
                }

                if self.loaded_files.len() > 1 {
                    ui.separator();
                    ui.label(format!("{} files loaded", self.loaded_files.len()));
                }
            });
        });
    }

    fn close_file(&mut self, idx: usize) {
        if idx < self.loaded_files.len() {
            self.loaded_files.remove(idx);
            if self.selected_file_idx >= self.loaded_files.len() && !self.loaded_files.is_empty() {
                self.selected_file_idx = self.loaded_files.len() - 1;
            }
            self.selected_event_idx = None;
            self.sequence_diagram.invalidate_cache();
            self.packetization_diagram.invalidate_cache();
            if !self.loaded_files.is_empty() {
                let file = &self.loaded_files[self.selected_file_idx];
                self.stats_view
                    .update_stats(&file.qlog_data, &file.packet_correlation);
            }
        }
    }

    fn render_main_content(&mut self, ctx: &Context) {
        CentralPanel::default().show(ctx, |ui| {
            if self.loading {
                ui.centered_and_justified(|ui| {
                    ui.spinner();
                    ui.label("Loading qlog file...");
                });
                return;
            }

            if let Some(error) = &self.error_message {
                ui.centered_and_justified(|ui| {
                    ui.colored_label(egui::Color32::RED, format!("❌ Error: {}", error));
                });
                return;
            }

            if self.loaded_files.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("No file loaded. Use File → Open to load a qlog file.");
                });
                return;
            }

            let file = &self.loaded_files[self.selected_file_idx];
            let data = &file.qlog_data;
            let correlation = &file.packet_correlation;
            let stream_ids = file.stream_ids.clone();
            let packet_types = file.packet_types.clone();

            match self.view_mode {
                ViewMode::EventList => {
                    let mut selected = self.selected_event_idx;
                    Self::render_event_list_static(
                        ui,
                        data,
                        &mut self.filters,
                        &mut selected,
                        EventListFilterOptions {
                            available_stream_ids: &stream_ids,
                            available_packet_types: &packet_types,
                            available_path_ids: &self.available_path_ids,
                        },
                    );
                    self.selected_event_idx = selected;
                }
                ViewMode::SequenceDiagram => {
                    if self.loaded_files.len() >= 2 {
                        self.sequence_diagram.show_dual(
                            ui,
                            &self.loaded_files[0],
                            &self.loaded_files[1],
                            &mut self.selected_event_idx,
                            &mut self.recv_selected_event_idx,
                            &mut self.selected_file_idx,
                        );
                    } else {
                        self.sequence_diagram.show_single(
                            ui,
                            data,
                            correlation,
                            &mut self.selected_event_idx,
                        );
                    }
                }
                ViewMode::CongestionGraph => {
                    self.congestion_graph.show(ui, data, correlation);
                }
                ViewMode::MultiplexingDiagram => {
                    self.multiplexing_diagram.show(ui, data);
                }
                ViewMode::PacketizationDiagram => {
                    self.packetization_diagram
                        .show(ui, data, &mut self.selected_event_idx);
                }
                ViewMode::StatsView => {
                    self.stats_view.show(ui);
                }
            }
        });
    }

    fn render_event_list_static(
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
                let category = Self::get_event_category(event);
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
                    let event_stream = Self::get_event_stream_id(event);
                    if event_stream != Some(stream_id) {
                        return false;
                    }
                }

                // Packet type filter
                if let Some(ref pkt_type) = filters.packet_type {
                    let event_pkt_type = Self::get_event_packet_type(event);
                    if event_pkt_type.as_ref() != Some(pkt_type) {
                        return false;
                    }
                }

                // Path ID filter
                if let Some(path_id) = filters.path_id {
                    let event_path = Self::get_event_path_id(event);
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
                let mut prev_time: Option<f32> = if row_range.start > 0 {
                    filtered_events
                        .get(row_range.start - 1)
                        .map(|(_, e)| e.time)
                } else {
                    None
                };

                for row_idx in row_range.clone() {
                    if let Some((event_idx, event)) = filtered_events.get(row_idx) {
                        let is_selected = current_selection == Some(*event_idx);

                        let category_color = Self::get_event_row_color(event);

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

    fn get_event_packet_type(event: &Event) -> Option<String> {
        match &event.data {
            EventData::PacketSent(d) => Some(utils::full_packet_type(&format!(
                "{:?}",
                d.header.packet_type
            ))),
            EventData::PacketReceived(d) => Some(utils::full_packet_type(&format!(
                "{:?}",
                d.header.packet_type
            ))),
            _ => None,
        }
    }

    fn render_event_detail(&mut self, ctx: &Context) {
        if !self.show_event_detail {
            return;
        }

        let mut clicked_related: Option<usize> = None;

        SidePanel::right("event_detail")
            .resizable(true)
            .default_width(400.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    // In dual-file mode, show both sent and received events
                    if self.view_mode == ViewMode::SequenceDiagram && self.loaded_files.len() >= 2 {
                        self.render_dual_event_detail(ui);
                    } else {
                        // Single file mode - show only one event
                        let mut event_data = None;
                        let mut related_events = Vec::new();
                        if let Some(file) = self.selected_file() {
                            let data = &file.qlog_data;
                            if let Some(idx) = self.selected_event_idx {
                                if let Some(event) = data.events.get(idx) {
                                    event_data = Some((idx, event, data));
                                    related_events =
                                        file.packet_correlation.get_related_events(idx);
                                }
                            }
                        }

                        let Some((idx, event, data)) = event_data else {
                            ui.heading("Select event to show details");
                            return;
                        };

                        self.render_single_event(ui, idx, event, data, "single");

                        // Show related events
                        if !related_events.is_empty() {
                            ui.add_space(10.);
                            render_section_header(ui, "Related Events");
                            ui.add_space(4.0);

                            for (rel_idx, label) in &related_events {
                                if let Some(rel_event) = data.events.get(*rel_idx) {
                                    let rel_name = data.get_event_name(rel_event);
                                    let time_delta = rel_event.time - event.time;
                                    let delta_str = if time_delta >= 0.0 {
                                        format!("+{:.2}ms", time_delta)
                                    } else {
                                        format!("{:.2}ms", time_delta)
                                    };
                                    ui.horizontal(|ui| {
                                        if ui.link(format!("#{}", rel_idx)).clicked() {
                                            clicked_related = Some(*rel_idx);
                                        }
                                        ui.label(format!(
                                            "{} - {} ({})",
                                            label, rel_name, delta_str
                                        ));
                                    });
                                }
                            }
                        }
                    }
                });
            });

        // Handle clicks on related events (outside the closure)
        if let Some(new_idx) = clicked_related {
            self.selected_event_idx = Some(new_idx);
        }
    }

    fn render_single_event(
        &self,
        ui: &mut egui::Ui,
        idx: usize,
        event: &Event,
        data: &QlogData,
        id_prefix: &str,
    ) {
        ui.heading(format!("Event #{} - {}", idx, data.get_event_name(event)));
        render_header(ui, &event.data, id_prefix, idx);

        ui.separator();

        ui.label(format!("Time: {}", data.format_time(event)));

        if !event.ex_data.is_empty() {
            for (key, value) in &event.ex_data {
                ui.label(format!("[EX] {key}: {value}"));
            }
        }
        match event.data {
            EventData::ParametersSet(ref params) => {
                ui.add_space(5.);
                ui.separator();
                ui.add_space(5.);
                render_transport_parameters(ui, params, id_prefix, idx);
            }
            EventData::ParametersRestored(ref params) => {
                ui.add_space(5.);
                ui.separator();
                ui.add_space(5.);
                render_parameters_restored(ui, params);
            }
            EventData::RecoveryParametersSet(ref params) => {
                ui.add_space(5.);
                ui.separator();
                ui.add_space(5.);
                render_recovery_parameters(ui, params);
            }
            EventData::MetricsUpdated(ref params) => {
                ui.add_space(5.);
                ui.separator();
                ui.add_space(5.);
                render_recovery_metrics_updated(ui, params, id_prefix, idx);
            }
            EventData::TimerUpdated(ref timer) => {
                ui.add_space(5.);
                ui.separator();
                ui.add_space(5.);
                render_timer_updated(ui, timer, id_prefix, idx);
            }
            EventData::EcnStateUpdated(ref ecn) => {
                ui.add_space(5.);
                ui.separator();
                ui.add_space(5.);
                render_ecn_state_updated(ui, ecn, id_prefix, idx);
            }
            EventData::ConnectionStarted(ref started) => {
                ui.add_space(5.);
                ui.separator();
                ui.add_space(5.);
                render_connection_started(ui, started, id_prefix, idx)
            }
            EventData::TupleAssigned(ref tuple) => {
                ui.add_space(5.);
                ui.separator();
                ui.add_space(5.);
                render_tuple_assigned(ui, tuple, id_prefix, idx)
            }
            _ => {}
        }

        // Raw JSON toggle
        ui.add_space(5.);
        CollapsingHeader::new("Raw JSON")
            .id_salt(format!("{}-raw-json-{}", id_prefix, idx))
            .default_open(false)
            .show(ui, |ui| {
                let json = serde_json::to_string_pretty(&event)
                    .unwrap_or_else(|_| "Failed to serialize".into());
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut json.as_str())
                            .code_editor()
                            .desired_width(f32::INFINITY),
                    );
                });
            });

        if event.data.contains_quic_frames().is_some() {
            let frame_count = match &event.data {
                EventData::PacketSent(pkt) => pkt.frames.as_ref().map(|f| f.len()).unwrap_or(0),
                EventData::PacketReceived(pkt) => pkt.frames.as_ref().map(|f| f.len()).unwrap_or(0),
                EventData::PacketLost(pkt) => pkt.frames.as_ref().map(|f| f.len()).unwrap_or(0),
                EventData::MarkedForRetransmit(ev) => ev.frames.len(),
                EventData::FramesProcessed(ev) => ev.frames.len(),
                _ => 0,
            };
            ui.add_space(5.);
            ui.separator();
            ui.add_space(5.);
            render_section_header(ui, &format!("Frames ({})", frame_count));
            ui.add_space(4.0);
            match event.data {
                EventData::PacketSent(ref pkt) => {
                    if let Some(ref frames) = pkt.frames {
                        for (i, frame) in frames.iter().enumerate() {
                            render_frame_with_prefix(ui, id_prefix, idx, i, frame);
                        }
                    }
                }
                EventData::PacketReceived(ref pkt) => {
                    if let Some(ref frames) = pkt.frames {
                        for (i, frame) in frames.iter().enumerate() {
                            render_frame_with_prefix(ui, id_prefix, idx, i, frame);
                        }
                    }
                }
                EventData::PacketLost(ref pkt) => {
                    if let Some(ref frames) = pkt.frames {
                        for (i, frame) in frames.iter().enumerate() {
                            render_frame_with_prefix(ui, id_prefix, idx, i, frame);
                        }
                    }
                }
                EventData::MarkedForRetransmit(ref ev) => {
                    for (i, frame) in ev.frames.iter().enumerate() {
                        render_frame_with_prefix(ui, id_prefix, idx, i, frame);
                    }
                }
                EventData::FramesProcessed(ref ev) => {
                    for (i, frame) in ev.frames.iter().enumerate() {
                        render_frame_with_prefix(ui, id_prefix, idx, i, frame);
                    }
                }
                _ => {}
            }
        }
    }

    fn render_dual_event_detail(&self, ui: &mut egui::Ui) {
        let sent_idx = self.selected_event_idx;
        let recv_idx = self.recv_selected_event_idx;

        if sent_idx.is_none() {
            ui.heading("Select an arrow to show event details");
            return;
        }

        let sent_file_idx = self.selected_file_idx;
        let recv_file_idx = if sent_file_idx == 0 { 1 } else { 0 };

        // Show sent event
        if let Some(idx) = sent_idx {
            if let Some(sent_file) = self.loaded_files.get(sent_file_idx) {
                if let Some(event) = sent_file.qlog_data.events.get(idx) {
                    ui.heading(
                        egui::RichText::new(format!("📤 SENT Event #{}", idx))
                            .color(egui::Color32::from_rgb(100, 150, 255)),
                    );
                    ui.label(format!("File: {}", sent_file.label));
                    ui.separator();
                    self.render_single_event(ui, idx, event, &sent_file.qlog_data, "sent");
                }
            }
        }

        ui.add_space(20.);
        ui.separator();
        ui.add_space(10.);

        // Show received event
        if let Some(idx) = recv_idx {
            if let Some(recv_file) = self.loaded_files.get(recv_file_idx) {
                if let Some(event) = recv_file.qlog_data.events.get(idx) {
                    ui.heading(
                        egui::RichText::new(format!("📥 RECEIVED Event #{}", idx))
                            .color(egui::Color32::from_rgb(100, 255, 150)),
                    );
                    ui.label(format!("File: {}", recv_file.label));
                    ui.separator();
                    self.render_single_event(ui, idx, event, &recv_file.qlog_data, "recv");
                }
            }
        } else {
            ui.heading(egui::RichText::new("❌ PACKET LOST").color(egui::Color32::RED));
            ui.label("This packet was sent but never received by the other endpoint");
        }
    }

    fn get_event_stream_id(event: &Event) -> Option<u64> {
        match &event.data {
            EventData::StreamStateUpdated(d) => Some(d.stream_id),
            EventData::FramesProcessed(d) => {
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

    fn get_event_path_id(event: &Event) -> Option<u64> {
        match &event.data {
            EventData::PacketSent(d) => d.header.path_id,
            EventData::PacketReceived(d) => d.header.path_id,
            EventData::PacketLost(d) => d.header.as_ref().and_then(|h| h.path_id),
            _ => None,
        }
    }

    fn get_event_category(event: &Event) -> &'static str {
        match &event.data {
            EventData::PacketSent(_)
            | EventData::PacketReceived(_)
            | EventData::PacketLost(_)
            | EventData::PacketsAcked(_) => "packet",
            EventData::MetricsUpdated(_) | EventData::CongestionStateUpdated(_) => "metrics",
            EventData::TimerUpdated(_) => "timer",
            EventData::ConnectionStarted(_)
            | EventData::ConnectionStateUpdated(_)
            | EventData::ConnectionClosed(_)
            | EventData::TupleAssigned(_) => "connection",
            EventData::RecoveryParametersSet(_)
            | EventData::ParametersRestored(_)
            | EventData::ParametersSet(_)
            | EventData::EcnStateUpdated(_) => "recovery",
            EventData::StreamStateUpdated(_) | EventData::FramesProcessed(_) => "stream",
            _ => "other",
        }
    }

    fn get_event_row_color(event: &Event) -> egui::Color32 {
        match Self::get_event_category(event) {
            "packet" => egui::Color32::from_rgba_unmultiplied(52, 152, 219, 20),
            "metrics" => egui::Color32::from_rgba_unmultiplied(142, 68, 173, 20),
            "timer" => egui::Color32::from_rgba_unmultiplied(158, 158, 158, 15),
            "connection" => egui::Color32::from_rgba_unmultiplied(0, 150, 136, 20),
            "recovery" => egui::Color32::from_rgba_unmultiplied(255, 152, 0, 20),
            "stream" => egui::Color32::from_rgba_unmultiplied(231, 76, 60, 20),
            _ => egui::Color32::TRANSPARENT,
        }
    }

    fn load_file(&mut self, path: PathBuf) {
        info!("Loading file: {:?}", path);
        self.loading = true;
        self.error_message = None;

        match QlogData::from_file(&path) {
            Ok(data) => {
                info!("Successfully loaded {} events", data.events.len());
                let (stream_ids, packet_types, path_ids) = Self::extract_filter_options(&data);
                let correlation = PacketCorrelation::from_qlog(&data);
                info!(
                    "Computed correlation: {} sent packets, {} lost, {} reorderings, {} time gaps, {} congestion states",
                    correlation.sent_packets.len(),
                    correlation.lost_packets.len(),
                    correlation.reorderings.len(),
                    correlation.time_gaps.len(),
                    correlation.congestion_states.len()
                );

                let file_num = self.loaded_files.len() + 1;
                let label = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| format!("File {}", file_num));

                let loaded_file = LoadedFile {
                    path,
                    label,
                    qlog_data: data,
                    packet_correlation: correlation,
                    stream_ids,
                    packet_types,
                };

                self.stats_view
                    .update_stats(&loaded_file.qlog_data, &loaded_file.packet_correlation);

                self.available_path_ids = path_ids;
                self.loaded_files.push(loaded_file);
                self.selected_file_idx = self.loaded_files.len() - 1;
                self.loading = false;
                self.filters.stream_id = None;
                self.filters.packet_type = None;
                self.filters.path_id = None;
                self.filters.text.clear();
                self.selected_event_idx = None;
                self.invalidate_all_caches();
            }
            Err(e) => {
                error!("Failed to load file: {}", e);
                self.error_message = Some(format!("{}", e));
                self.loading = false;
            }
        }
    }

    fn extract_filter_options(data: &QlogData) -> (Vec<u64>, Vec<String>, Vec<u64>) {
        let mut stream_ids: BTreeSet<u64> = BTreeSet::new();
        let mut packet_types: BTreeSet<String> = BTreeSet::new();
        let mut path_ids: BTreeSet<u64> = BTreeSet::new();

        for event in &data.events {
            match &event.data {
                EventData::PacketSent(d) => {
                    let pkt_type = utils::full_packet_type(&format!("{:?}", d.header.packet_type));
                    packet_types.insert(pkt_type);
                    if let Some(path_id) = d.header.path_id {
                        path_ids.insert(path_id);
                    }

                    if let Some(ref frames) = d.frames {
                        for frame in frames.iter() {
                            if let Some(sid) = utils::get_frame_stream_id(frame) {
                                stream_ids.insert(sid);
                            }
                        }
                    }
                }
                EventData::PacketReceived(d) => {
                    let pkt_type = utils::full_packet_type(&format!("{:?}", d.header.packet_type));
                    packet_types.insert(pkt_type);
                    if let Some(path_id) = d.header.path_id {
                        path_ids.insert(path_id);
                    }

                    if let Some(ref frames) = d.frames {
                        for frame in frames.iter() {
                            if let Some(sid) = utils::get_frame_stream_id(frame) {
                                stream_ids.insert(sid);
                            }
                        }
                    }
                }
                EventData::PacketLost(d) => {
                    if let Some(ref header) = d.header {
                        if let Some(path_id) = header.path_id {
                            path_ids.insert(path_id);
                        }
                    }
                }
                EventData::StreamStateUpdated(d) => {
                    stream_ids.insert(d.stream_id);
                }
                EventData::FramesProcessed(d) => {
                    for frame in d.frames.iter() {
                        if let Some(sid) = utils::get_frame_stream_id(frame) {
                            stream_ids.insert(sid);
                        }
                    }
                }
                _ => {}
            }
        }

        (
            stream_ids.into_iter().collect(),
            packet_types.into_iter().collect(),
            path_ids.into_iter().collect(),
        )
    }
}

impl eframe::App for QlogViewerApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Handle number key shortcuts for view switching (only when no text input is focused)
        if !ctx.wants_keyboard_input() {
            ctx.input(|i| {
                if i.key_pressed(egui::Key::Num1) {
                    self.view_mode = ViewMode::EventList;
                } else if i.key_pressed(egui::Key::Num2) {
                    self.view_mode = ViewMode::SequenceDiagram;
                } else if i.key_pressed(egui::Key::Num3) {
                    self.view_mode = ViewMode::CongestionGraph;
                } else if i.key_pressed(egui::Key::Num4) {
                    self.view_mode = ViewMode::PacketizationDiagram;
                } else if i.key_pressed(egui::Key::Num5) {
                    self.view_mode = ViewMode::StatsView;
                }
            });
        }

        self.render_menu_bar(ctx);
        self.render_event_detail(ctx);
        self.render_main_content(ctx);
    }
}
