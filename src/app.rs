use crate::congestion_graph::CongestionGraph;
use crate::multiplexing_diagram::MultiplexingDiagram;
use crate::packet_correlation::PacketCorrelation;
use crate::packetization_diagram::PacketizationDiagram;
use crate::qlog_data::QlogData;
use crate::sequence_diagram::{MetricsVisualizationMode, SequenceDiagram};
use crate::stats_view::StatsView;
use crate::utils::{self, FrameType};
use egui::text::LayoutJob;
use egui::{
    CentralPanel, CollapsingHeader, Context, FontFamily, FontId, SidePanel, TextFormat,
    TopBottomPanel,
};
use qlog::events::quic::PacketHeader;
use qlog::events::RawInfo;
use qlog::events::{quic::QuicFrame, Event, EventData};
use std::collections::BTreeSet;
use std::path::PathBuf;
use tracing::{error, info};

pub struct LoadedFile {
    pub path: PathBuf,
    pub label: String,
    pub qlog_data: QlogData,
    pub packet_correlation: PacketCorrelation,
    pub stream_ids: Vec<u64>,
    pub packet_types: Vec<String>,
}

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
}

pub struct QlogViewerApp {
    loaded_files: Vec<LoadedFile>,
    selected_file_idx: usize,
    loading: bool,
    error_message: Option<String>,
    selected_event_idx: Option<usize>,
    recv_selected_event_idx: Option<usize>,
    filter_text: String,
    filter_stream_id: Option<u64>,
    filter_packet_type: Option<String>,
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
            filter_text: String::new(),
            filter_stream_id: None,
            filter_packet_type: None,
            show_event_detail: true,
            view_mode: ViewMode::EventList,
            sequence_diagram: SequenceDiagram::new(),
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

                    ui.separator();
                    ui.label("Sequence Diagram Options:");

                    // Main toggle to show/hide all metrics events
                    ui.checkbox(
                        &mut self.sequence_diagram.show_metrics_events,
                        "Show Metrics Events",
                    );

                    // Secondary toggle for filtering (only enabled if metrics are shown)
                    ui.add_enabled_ui(self.sequence_diagram.show_metrics_events, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Visualization:");
                            if ui
                                .radio(
                                    self.sequence_diagram.metrics_visualization_mode
                                        == MetricsVisualizationMode::Events,
                                    "Events",
                                )
                                .clicked()
                            {
                                self.sequence_diagram.metrics_visualization_mode =
                                    MetricsVisualizationMode::Events;
                            }
                            if ui
                                .radio(
                                    self.sequence_diagram.metrics_visualization_mode
                                        == MetricsVisualizationMode::Graphs,
                                    "Graphs",
                                )
                                .clicked()
                            {
                                self.sequence_diagram.metrics_visualization_mode =
                                    MetricsVisualizationMode::Graphs;
                            }
                        });

                        // Only show the "Show All Metrics Updates" toggle when in Events mode
                        if self.sequence_diagram.metrics_visualization_mode
                            == MetricsVisualizationMode::Events
                        {
                            if ui
                                .checkbox(
                                    &mut self.sequence_diagram.show_all_metrics,
                                    "Show All Metrics Updates",
                                )
                                .changed()
                            {
                                // Re-extract events with new filter by invalidating cache
                                self.sequence_diagram.invalidate_cache();
                            }

                            if !self.sequence_diagram.show_all_metrics {
                                ui.label("(Showing significant changes only)");
                            }
                        }
                    });
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
                    let mut filter_text = self.filter_text.clone();
                    let mut filter_stream_id = self.filter_stream_id;
                    let mut filter_packet_type = self.filter_packet_type.clone();
                    let mut selected = self.selected_event_idx;

                    Self::render_event_list_static(
                        ui,
                        data,
                        &mut filter_text,
                        &mut filter_stream_id,
                        &mut filter_packet_type,
                        &mut selected,
                        EventListFilterOptions {
                            available_stream_ids: &stream_ids,
                            available_packet_types: &packet_types,
                        },
                    );

                    self.filter_text = filter_text;
                    self.filter_stream_id = filter_stream_id;
                    self.filter_packet_type = filter_packet_type;
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
                        self.sequence_diagram.show(
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
        filter_text: &mut String,
        filter_stream_id: &mut Option<u64>,
        filter_packet_type: &mut Option<String>,
        selected_event_idx: &mut Option<usize>,
        options: EventListFilterOptions<'_>,
    ) {
        ui.horizontal(|ui| {
            ui.label("🔍 Event:");
            ui.add(egui::TextEdit::singleline(filter_text).desired_width(120.0));

            ui.separator();

            ui.label("Packet Type:");
            let current_type_label = filter_packet_type
                .as_ref()
                .map(|s| s.as_str())
                .unwrap_or("All");
            egui::ComboBox::from_id_salt("event_list_packet_type_filter")
                .selected_text(current_type_label)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(filter_packet_type.is_none(), "All")
                        .clicked()
                    {
                        *filter_packet_type = None;
                    }
                    for ptype in options.available_packet_types {
                        let selected = filter_packet_type.as_ref() == Some(ptype);
                        if ui.selectable_label(selected, ptype).clicked() {
                            *filter_packet_type = Some(ptype.clone());
                        }
                    }
                });

            ui.separator();

            ui.label("Stream ID:");
            let current_stream_label = filter_stream_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "All".to_string());
            egui::ComboBox::from_id_salt("event_list_stream_id_filter")
                .selected_text(&current_stream_label)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(filter_stream_id.is_none(), "All")
                        .clicked()
                    {
                        *filter_stream_id = None;
                    }
                    for &stream_id in options.available_stream_ids {
                        let selected = *filter_stream_id == Some(stream_id);
                        if ui
                            .selectable_label(selected, stream_id.to_string())
                            .clicked()
                        {
                            *filter_stream_id = Some(stream_id);
                        }
                    }
                });

            ui.separator();

            if ui.button("Clear Filters").clicked() {
                filter_text.clear();
                *filter_stream_id = None;
                *filter_packet_type = None;
            }
        });

        ui.separator();

        let filter_text_lower = filter_text.to_lowercase();

        let filtered_events: Vec<(usize, &Event)> = data
            .events
            .iter()
            .enumerate()
            .filter(|(_, event)| {
                if !filter_text_lower.is_empty() {
                    let event_name = data.get_event_name(event);
                    if !event_name.to_lowercase().contains(&filter_text_lower) {
                        return false;
                    }
                }

                if let Some(stream_id) = filter_stream_id {
                    let event_stream = Self::get_event_stream_id(event);
                    if event_stream != Some(*stream_id) {
                        return false;
                    }
                }

                if let Some(ref pkt_type) = filter_packet_type {
                    let event_pkt_type = Self::get_event_packet_type(event);
                    if event_pkt_type.as_ref() != Some(pkt_type) {
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
                // Find the position of the current selection in filtered events
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
            } else if !filtered_events.is_empty() {
                // No selection yet, select first item on down arrow
                if down_pressed {
                    new_selection = Some(filtered_events[0].0);
                }
            }
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, 20.0, filtered_events.len(), |ui, row_range| {
                egui::Grid::new("event_grid")
                    .striped(true)
                    .num_columns(4)
                    .show(ui, |ui| {
                        for row_idx in row_range {
                            if let Some((actual_idx, event)) = filtered_events.get(row_idx) {
                                let is_selected = current_selection == Some(*actual_idx);

                                // Make entire row clickable by using horizontal layout with selectable_label
                                ui.horizontal(|ui| {
                                    let response =
                                        ui.selectable_label(is_selected, format!("{}", row_idx));
                                    if response.clicked() {
                                        new_selection = Some(*actual_idx);
                                    }
                                });

                                // Make each cell clickable
                                let time_response =
                                    ui.selectable_label(is_selected, data.format_time(event));
                                if time_response.clicked() {
                                    new_selection = Some(*actual_idx);
                                }

                                let name_response =
                                    ui.selectable_label(is_selected, data.get_event_name(event));
                                if name_response.clicked() {
                                    new_selection = Some(*actual_idx);
                                }

                                let summary_response =
                                    ui.selectable_label(is_selected, data.get_event_summary(event));
                                if summary_response.clicked() {
                                    new_selection = Some(*actual_idx);
                                }

                                ui.end_row();
                            }
                        }
                    });
            });

        *selected_event_idx = new_selection;

        ui.separator();
        ui.label(format!(
            "Showing {} of {} events",
            filtered_events.len(),
            data.events.len()
        ));
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
                        if let Some(file) = self.selected_file() {
                            let data = &file.qlog_data;
                            if let Some(idx) = self.selected_event_idx {
                                if let Some(event) = data.events.get(idx) {
                                    event_data = Some((idx, event, data));
                                }
                            }
                        }

                        let Some((idx, event, data)) = event_data else {
                            ui.heading("Select event to show details");
                            return;
                        };

                        self.render_single_event(ui, idx, event, data, "single");
                    }
                });
            });
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
        render_header(ui, &event.data);

        ui.separator();

        ui.label(format!("Time: {}", data.format_time(event)));

        if !event.ex_data.is_empty() {
            for (key, value) in &event.ex_data {
                ui.label(format!("[EX] {key}: {value}"));
            }
        }

        if event.data.contains_quic_frames().is_some() {
            ui.add_space(5.);
            ui.separator();
            ui.add_space(5.);
            ui.heading("Frames");
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

        CollapsingHeader::new("Raw JSON")
            .id_salt(format!("{}-raw-json-{}", id_prefix, idx))
            .show(ui, |ui| {
                let s = serde_json::to_string_pretty(&event).unwrap();
                ui.label(s);
            });
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

    fn load_file(&mut self, path: PathBuf) {
        info!("Loading file: {:?}", path);
        self.loading = true;
        self.error_message = None;

        match QlogData::from_file(&path) {
            Ok(data) => {
                info!("Successfully loaded {} events", data.events.len());
                let (stream_ids, packet_types) = Self::extract_filter_options(&data);
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

                self.loaded_files.push(loaded_file);
                self.selected_file_idx = self.loaded_files.len() - 1;
                self.loading = false;
                self.filter_stream_id = None;
                self.filter_packet_type = None;
                self.filter_text.clear();
                self.selected_event_idx = None;
                self.sequence_diagram.invalidate_cache();
                self.packetization_diagram.invalidate_cache();
            }
            Err(e) => {
                error!("Failed to load file: {}", e);
                self.error_message = Some(format!("{}", e));
                self.loading = false;
            }
        }
    }

    fn extract_filter_options(data: &QlogData) -> (Vec<u64>, Vec<String>) {
        let mut stream_ids: BTreeSet<u64> = BTreeSet::new();
        let mut packet_types: BTreeSet<String> = BTreeSet::new();

        for event in &data.events {
            match &event.data {
                EventData::PacketSent(d) => {
                    let pkt_type = utils::full_packet_type(&format!("{:?}", d.header.packet_type));
                    packet_types.insert(pkt_type);

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

                    if let Some(ref frames) = d.frames {
                        for frame in frames.iter() {
                            if let Some(sid) = utils::get_frame_stream_id(frame) {
                                stream_ids.insert(sid);
                            }
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
        )
    }
}

impl eframe::App for QlogViewerApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Handle number key shortcuts for view switching
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

        self.render_menu_bar(ctx);
        self.render_event_detail(ctx);
        self.render_main_content(ctx);
    }
}

fn render_frame_with_prefix(
    ui: &mut egui::Ui,
    prefix: &str,
    event_id: usize,
    frame_id: usize,
    frame: &QuicFrame,
) {
    use QuicFrame::*;

    let ty = FrameType::from_quic_frame(frame);
    let mut heading = LayoutJob::default();
    heading.append(
        &format!("[{}]", ty.short_name()),
        0.,
        TextFormat {
            color: ty.color(),
            font_id: FontId::new(16.0, FontFamily::Proportional),
            ..Default::default()
        },
    );
    heading.append(
        &ty.display_name(),
        1.,
        TextFormat {
            font_id: FontId::new(16.0, FontFamily::Proportional),
            ..Default::default()
        },
    );

    CollapsingHeader::new(heading)
        .id_salt(format!(
            "{}-frame-{event_id}-{frame_id}-{}",
            prefix,
            ty.short_name()
        ))
        .show(ui, |ui| {
            egui::Grid::new(format!(
                "{}-frame-{event_id}-{frame_id}-{}",
                prefix,
                ty.short_name()
            ))
            .num_columns(2)
            .spacing([40.0, 4.0])
            .striped(true)
            .show(ui, |ui| match frame {
                Padding { raw } => {
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                Ping { raw } => {
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                Ack {
                    ack_delay,
                    acked_ranges,
                    ect1,
                    ect0,
                    ce,
                    raw,
                } => {
                    if let Some(ack_delay) = ack_delay {
                        ui.label("Ack Delay");
                        ui.label(format!("{ack_delay:?}"));
                        ui.end_row();
                    }
                    if let Some(acked_ranges) = acked_ranges {
                        ui.label("Acked Ranges");
                        ui.label(format!("{acked_ranges:?}"));
                        ui.end_row();
                    }
                    if let Some(ect0) = ect0 {
                        ui.label("ECT 0");
                        ui.label(format!("{ect0:?}"));
                        ui.end_row();
                    }
                    if let Some(ect1) = ect1 {
                        ui.label("ECT 1");
                        ui.label(format!("{ect1:?}"));
                        ui.end_row();
                    }
                    if let Some(ce) = ce {
                        ui.label("CE");
                        ui.label(format!("{ce:?}"));
                        ui.end_row();
                    }
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                ResetStream {
                    stream_id,
                    error,
                    error_code,
                    final_size,
                    raw,
                } => {
                    ui.label("Stream Id");
                    ui.label(format!("{stream_id}"));
                    ui.end_row();
                    ui.label("Error");
                    ui.label(format!("{error:?}"));
                    ui.end_row();
                    if let Some(code) = error_code {
                        ui.label("Error Code");
                        ui.label(format!("{code}"));
                        ui.end_row();
                    }
                    ui.label("Final Size");
                    ui.label(format!("{final_size}"));
                    ui.end_row();

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                StopSending {
                    stream_id,
                    error,
                    error_code,
                    raw,
                } => {
                    ui.label("Stream Id");
                    ui.label(format!("{stream_id}"));
                    ui.end_row();
                    ui.label("Error");
                    ui.label(format!("{error:?}"));
                    ui.end_row();
                    if let Some(code) = error_code {
                        ui.label("Error Code");
                        ui.label(format!("{code}"));
                        ui.end_row();
                    }

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                Crypto { offset, raw } => {
                    ui.label("Offset");
                    ui.label(format!("{offset}"));
                    ui.end_row();
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                NewToken { token, raw } => {
                    if let Some(ref ty) = token.ty {
                        ui.label("Token Type");
                        ui.label(format!("{ty:?}"));
                        ui.end_row();
                    }
                    if let Some(ref details) = token.details {
                        ui.label("Token Details");
                        ui.label(details);
                        ui.end_row();
                    }

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                Stream {
                    stream_id,
                    offset,
                    fin,
                    raw,
                } => {
                    ui.label("Stream Id");
                    ui.label(format!("{stream_id}"));
                    ui.end_row();
                    if let Some(offset) = offset {
                        ui.label("Offset");
                        ui.label(format!("{offset}"));
                        ui.end_row();
                    }
                    if let Some(fin) = fin {
                        ui.label("Fin");
                        ui.label(format!("{fin}"));
                        ui.end_row();
                    }
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                MaxData { maximum, raw } => {
                    ui.label("Maximum");
                    ui.label(format!("{maximum}"));
                    ui.end_row();
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                MaxStreamData {
                    stream_id,
                    maximum,
                    raw,
                } => {
                    ui.label("Stream Id");
                    ui.label(format!("{stream_id}"));
                    ui.end_row();
                    ui.label("Maximum");
                    ui.label(format!("{maximum}"));
                    ui.end_row();
                    ui.label("Max Stream Data");
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                MaxStreams {
                    stream_type,
                    maximum,
                    raw,
                } => {
                    ui.label("Stream Type");
                    ui.label(format!("{stream_type:?}"));
                    ui.end_row();
                    ui.label("Maximum");
                    ui.label(format!("{maximum}"));
                    ui.end_row();
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                DataBlocked { limit, raw } => {
                    ui.label("Limit");
                    ui.label(format!("{limit}"));
                    ui.end_row();

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                StreamDataBlocked {
                    stream_id,
                    limit,
                    raw,
                } => {
                    ui.label("Stream Id");
                    ui.label(format!("{stream_id}"));
                    ui.end_row();

                    ui.label("Limit");
                    ui.label(format!("{limit}"));
                    ui.end_row();

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                StreamsBlocked {
                    stream_type,
                    limit,
                    raw,
                } => {
                    ui.label("Stream Type");
                    ui.label(format!("{stream_type:?}"));
                    ui.end_row();

                    ui.label("Limit");
                    ui.label(format!("{limit}"));
                    ui.end_row();

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                NewConnectionId {
                    sequence_number,
                    retire_prior_to,
                    connection_id_length,
                    connection_id,
                    stateless_reset_token,
                    raw,
                } => {
                    ui.label("Connection Id");
                    ui.label(connection_id);
                    ui.end_row();

                    ui.label("Seq Number");
                    ui.label(format!("{sequence_number}"));
                    ui.end_row();
                    ui.label("Retire Prior To");
                    ui.label(format!("{retire_prior_to}"));
                    ui.end_row();
                    if let Some(len) = connection_id_length {
                        ui.label("Connection Id Length");
                        ui.label(format!("{len}"));
                        ui.end_row();
                    }
                    if let Some(token) = stateless_reset_token {
                        ui.label("Stateless Reset Token");
                        ui.label(token);
                        ui.end_row();
                    }

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                RetireConnectionId {
                    sequence_number,
                    raw,
                } => {
                    ui.label("Seq Number");
                    ui.label(format!("{sequence_number}"));
                    ui.end_row();
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                PathChallenge { data, raw } => {
                    if let Some(data) = data {
                        ui.label("Data");
                        ui.label(data);
                        ui.end_row();
                    }
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                PathResponse { data, raw } => {
                    if let Some(data) = data {
                        ui.label("Data");
                        ui.label(data);
                        ui.end_row();
                    }

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                ConnectionClose {
                    error_space,
                    error,
                    error_code,
                    reason,
                    reason_bytes,

                    trigger_frame_type,
                } => {
                    if let Some(error) = error {
                        ui.label("Error");
                        ui.label(format!("{error:?}"));
                        ui.end_row();
                    }
                    if let Some(error) = error_code {
                        ui.label("Error Code");
                        ui.label(format!("{error:?}"));
                        ui.end_row();
                    }
                    if let Some(error) = error_space {
                        ui.label("Error Space");
                        ui.label(format!("{error:?}"));
                        ui.end_row();
                    }
                    if let Some(reason) = reason {
                        ui.label("Reason");
                        ui.label(reason);
                        ui.end_row();
                    }
                    if let Some(reason) = reason_bytes {
                        ui.label("Reason Bytes");
                        ui.label(reason);
                        ui.end_row();
                    }
                    if let Some(ty) = trigger_frame_type {
                        ui.label("Trigger Frame Type");
                        ui.label(format!("{ty}"));
                        ui.end_row();
                    }
                }

                HandshakeDone { raw } => {
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }

                Datagram { raw } => {
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }

                PathAck {
                    path_id,
                    ack_delay,
                    acked_ranges,

                    ect1,
                    ect0,
                    ce,

                    raw,
                } => {
                    ui.label("Path Id");
                    ui.label(format!("{path_id}"));
                    ui.end_row();

                    if let Some(delay) = ack_delay {
                        ui.label("Ack Delay");
                        ui.label(format!("{delay}"));
                        ui.end_row();
                    }

                    if let Some(ranges) = acked_ranges {
                        ui.label("Acked Ranges");
                        ui.label(format!("{ranges:?}"));
                        ui.end_row();
                    }

                    if let Some(ect0) = ect0 {
                        ui.label("ECT 0");
                        ui.label(format!("{ect0:?}"));
                        ui.end_row();
                    }
                    if let Some(ect1) = ect1 {
                        ui.label("ECT 1");
                        ui.label(format!("{ect1:?}"));
                        ui.end_row();
                    }
                    if let Some(ce) = ce {
                        ui.label("CE");
                        ui.label(format!("{ce:?}"));
                        ui.end_row();
                    }

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                PathAbandon {
                    path_id,
                    error_code,
                    raw,
                } => {
                    ui.label("Path Id");
                    ui.label(format!("{path_id}"));
                    ui.end_row();

                    ui.label("Error Code");
                    ui.label(format!("{error_code}"));
                    ui.end_row();

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }

                PathStatusAvailable {
                    path_id,
                    path_status_sequence_number,
                    raw,
                } => {
                    ui.label("Path Id");
                    ui.label(format!("{path_id}"));
                    ui.end_row();

                    ui.label("Path Status Seq Number");
                    ui.label(format!("{path_status_sequence_number}"));
                    ui.end_row();

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }

                PathStatusBackup {
                    path_id,
                    path_status_sequence_number,
                    raw,
                } => {
                    ui.label("Path Id");
                    ui.label(format!("{path_id}"));
                    ui.end_row();

                    ui.label("Path Status Seq Number");
                    ui.label(format!("{path_status_sequence_number}"));
                    ui.end_row();

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }

                PathNewConnectionId {
                    path_id,
                    sequence_number,
                    retire_prior_to,
                    connection_id_length,
                    connection_id,
                    stateless_reset_token,
                    raw,
                } => {
                    ui.label("Connection Id");
                    ui.label(connection_id.to_string());
                    ui.end_row();

                    ui.label("Path Id");
                    ui.label(format!("{path_id}"));
                    ui.end_row();

                    ui.label("Seq Number");
                    ui.label(format!("{sequence_number}"));
                    ui.end_row();

                    ui.label("Retire Prior To");
                    ui.label(format!("{retire_prior_to}"));
                    ui.end_row();

                    if let Some(len) = connection_id_length {
                        ui.label("Connection Id Length");
                        ui.label(format!("{len}"));
                        ui.end_row();
                    }
                    if let Some(token) = stateless_reset_token {
                        ui.label("Stateless Reset Token");
                        ui.label(token.to_string());
                        ui.end_row();
                    }

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }

                PathRetireConnectionId {
                    path_id,
                    sequence_number,
                    raw,
                } => {
                    ui.label("Path Id");
                    ui.label(format!("{path_id}"));
                    ui.end_row();

                    ui.label("Seq Number");
                    ui.label(format!("{sequence_number}"));
                    ui.end_row();

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                MaxPathId {
                    maximum_path_id,
                    raw,
                } => {
                    ui.label("Maxium Path Id");
                    ui.label(format!("{maximum_path_id}"));
                    ui.end_row();

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                AckFrequency {
                    sequence_number,
                    ack_eliciting_threshold,
                    requested_max_ack_delay,
                    reordering_threshold,
                    raw,
                } => {
                    ui.label("Seq Number");
                    ui.label(format!("{sequence_number}"));
                    ui.end_row();
                    ui.label("Ack Eliciting Threshold");
                    ui.label(format!("{ack_eliciting_threshold}"));
                    ui.end_row();
                    ui.label("Requested Max Ack Delay");
                    ui.label(format!("{requested_max_ack_delay}"));
                    ui.end_row();
                    ui.label("Reordering Threshold");
                    ui.label(format!("{reordering_threshold}"));
                    ui.end_row();

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                ImmediateAck { raw } => {
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                ObservedAddress {
                    sequence_number,
                    ip_v4,
                    ip_v6,
                    port,
                    raw,
                } => {
                    ui.label("Seq Number");
                    ui.label(format!("{sequence_number}"));
                    ui.end_row();
                    if let Some(ip) = ip_v4 {
                        ui.label("IP V4");
                        ui.label(ip);
                        ui.end_row();
                    }
                    if let Some(ip) = ip_v6 {
                        ui.label("IP V6");
                        ui.label(ip);
                        ui.end_row();
                    }
                    ui.label("Port");
                    ui.label(format!("{port}"));
                    ui.end_row();
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                AddAddress {
                    sequence_number,
                    ip_v4,
                    ip_v6,
                    port,
                } => {
                    ui.label("Seq Number");
                    ui.label(format!("{sequence_number}"));
                    ui.end_row();
                    if let Some(ip) = ip_v4 {
                        ui.label("IP V4");
                        ui.label(ip);
                        ui.end_row();
                    }
                    if let Some(ip) = ip_v6 {
                        ui.label("IP V6");
                        ui.label(ip);
                        ui.end_row();
                    }
                    ui.label("Port");
                    ui.label(format!("{port}"));
                    ui.end_row();
                }
                ReachOut {
                    round,
                    ip_v4,
                    ip_v6,
                    port,
                } => {
                    ui.label("round");
                    ui.label(format!("{round}"));
                    ui.end_row();
                    if let Some(ip) = ip_v4 {
                        ui.label("IP V4");
                        ui.label(ip);
                        ui.end_row();
                    }
                    if let Some(ip) = ip_v6 {
                        ui.label("IP V6");
                        ui.label(ip);
                        ui.end_row();
                    }
                    ui.label("Port");
                    ui.label(format!("{port}"));
                    ui.end_row();
                }
                RemoveAddress { sequence_number } => {
                    ui.label("Seq Number");
                    ui.label(format!("{sequence_number}"));
                    ui.end_row();
                }
                PathsBlocked {
                    maximum_path_id,
                    raw,
                } => {
                    ui.label("Maxium Path Id");
                    ui.label(format!("{maximum_path_id}"));
                    ui.end_row();

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                PathCidsBlocked {
                    path_id,
                    next_sequence_number,
                    raw,
                } => {
                    ui.label("Path Id");
                    ui.label(format!("{path_id}"));
                    ui.end_row();

                    ui.label("Next Seq Number");
                    ui.label(format!("{next_sequence_number}"));
                    ui.end_row();

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                Unknown {
                    frame_type_bytes,
                    raw,
                } => {
                    if let Some(ty) = frame_type_bytes {
                        ui.label("Frame type");
                        ui.label(format!("{ty}"));
                        ui.end_row();
                    }
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
            });
        });
    ui.add_space(5.);
}

fn render_raw_info(ui: &mut egui::Ui, raw: &RawInfo) {
    if let Some(length) = raw.length {
        ui.label("Raw Info: Length");
        ui.label(format!("{length}"));
        ui.end_row();
    }
    if let Some(length) = raw.payload_length {
        ui.label("Raw Info: Payload Length");
        ui.label(format!("{length}"));
        ui.end_row();
    }
    if let Some(ref data) = raw.data {
        ui.label("Raw Info: Data");
        ui.label(data);
        ui.end_row();
    }
}

fn render_header(ui: &mut egui::Ui, event: &EventData) {
    match event {
        EventData::PacketSent(ref data) => {
            render_inner_header(ui, &data.header);
        }
        EventData::PacketReceived(ref data) => {
            render_inner_header(ui, &data.header);
        }
        EventData::PacketDropped(ref data) => {
            if let Some(ref header) = data.header {
                render_inner_header(ui, header);
            }
        }
        EventData::PacketBuffered(ref data) => {
            if let Some(ref header) = data.header {
                render_inner_header(ui, header);
            }
        }
        EventData::PacketLost(ref data) => {
            if let Some(ref header) = data.header {
                render_inner_header(ui, header);
            }
        }
        _ => {}
    }
}

fn render_inner_header(ui: &mut egui::Ui, header: &PacketHeader) {
    ui.label(format!("Packet Space: {:?}", header.packet_type));
    if let Some(pn) = header.packet_number {
        ui.label(format!("Packet Number: {pn}"));
    }
    if let Some(pid) = header.path_id {
        ui.label(format!("Path Id: {pid}"));
    }
}
