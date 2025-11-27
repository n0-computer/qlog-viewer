use crate::congestion_graph::CongestionGraph;
use crate::multiplexing_diagram::MultiplexingDiagram;
use crate::packet_correlation::PacketCorrelation;
use crate::packetization_diagram::PacketizationDiagram;
use crate::qlog_data::QlogData;
use crate::sequence_diagram::SequenceDiagram;
use crate::stats_view::StatsView;
use crate::utils;
use egui::{CentralPanel, Context, SidePanel, TopBottomPanel};
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
}

pub struct QlogViewerApp {
    loaded_file: Option<PathBuf>,
    qlog_data: Option<QlogData>,
    packet_correlation: Option<PacketCorrelation>,
    loading: bool,
    error_message: Option<String>,
    selected_event_idx: Option<usize>,
    filter_text: String,
    filter_stream_id: Option<u64>,
    filter_packet_type: Option<String>,
    available_stream_ids: Vec<u64>,
    available_packet_types: Vec<String>,
    show_event_detail: bool,
    view_mode: ViewMode,
    sequence_diagram: SequenceDiagram,
    congestion_graph: CongestionGraph,
    multiplexing_diagram: MultiplexingDiagram,
    packetization_diagram: PacketizationDiagram,
    stats_view: StatsView,
}

impl QlogViewerApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            loaded_file: None,
            qlog_data: None,
            packet_correlation: None,
            loading: false,
            error_message: None,
            selected_event_idx: None,
            filter_text: String::new(),
            filter_stream_id: None,
            filter_packet_type: None,
            available_stream_ids: Vec::new(),
            available_packet_types: Vec::new(),
            show_event_detail: true,
            view_mode: ViewMode::EventList,
            sequence_diagram: SequenceDiagram::new(),
            congestion_graph: CongestionGraph::new(),
            multiplexing_diagram: MultiplexingDiagram::new(),
            packetization_diagram: PacketizationDiagram::new(),
            stats_view: StatsView::new(),
        }
    }

    fn render_menu_bar(&mut self, ctx: &Context) {
        TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open...").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("qlog files", &["qlog", "json", "sqlog"])
                            .pick_file()
                        {
                            self.load_file(path);
                        }
                        ui.close();
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

                if let Some(file_path) = &self.loaded_file {
                    ui.label(format!("📁 {}", file_path.display()));
                }

                if let Some(data) = &self.qlog_data {
                    ui.separator();
                    ui.label(format!("Events: {}", data.events.len()));
                }
            });
        });
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

            match &self.qlog_data {
                Some(data) => match self.view_mode {
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
                                available_stream_ids: &self.available_stream_ids,
                                available_packet_types: &self.available_packet_types,
                            },
                        );

                        self.filter_text = filter_text;
                        self.filter_stream_id = filter_stream_id;
                        self.filter_packet_type = filter_packet_type;
                        self.selected_event_idx = selected;
                    }
                    ViewMode::SequenceDiagram => {
                        if let Some(ref correlation) = self.packet_correlation {
                            self.sequence_diagram.show(
                                ui,
                                data,
                                correlation,
                                &mut self.selected_event_idx,
                            );
                        } else {
                            ui.label("Loading correlation data...");
                        }
                    }
                    ViewMode::CongestionGraph => {
                        if let Some(ref correlation) = self.packet_correlation {
                            self.congestion_graph.show(ui, data, correlation);
                        } else {
                            ui.label("Loading correlation data...");
                        }
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
                },
                None => {
                    ui.centered_and_justified(|ui| {
                        ui.label("No file loaded. Use File → Open to load a qlog file.");
                    });
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

                                let response =
                                    ui.selectable_label(is_selected, format!("{}", row_idx));
                                if response.clicked() {
                                    new_selection = Some(*actual_idx);
                                }

                                ui.label(data.format_time(event));
                                ui.label(data.get_event_name(event));
                                ui.label(data.get_event_summary(event));
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
                ui.heading("Event Details");
                ui.separator();

                if let Some(data) = &self.qlog_data {
                    if let Some(idx) = self.selected_event_idx {
                        if let Some(event) = data.events.get(idx) {
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                ui.label(format!("Event #{}", idx));
                                ui.label(format!("Time: {}", data.format_time(event)));
                                ui.label(format!("Name: {}", data.get_event_name(event)));
                                ui.separator();

                                ui.label("Raw JSON:");
                                let json_str = serde_json::to_string_pretty(&event)
                                    .unwrap_or_else(|_| "Failed to serialize".to_string());
                                egui::ScrollArea::vertical().show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::multiline(&mut json_str.as_str())
                                            .font(egui::TextStyle::Monospace)
                                            .desired_width(f32::INFINITY),
                                    );
                                });
                            });
                        }
                    } else {
                        ui.label("Select an event to view details");
                    }
                } else {
                    ui.label("No data loaded");
                }
            });
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
        self.loaded_file = Some(path.clone());

        match QlogData::from_file(&path) {
            Ok(data) => {
                info!("Successfully loaded {} events", data.events.len());
                let (stream_ids, packet_types) = Self::extract_filter_options(&data);
                self.available_stream_ids = stream_ids;
                self.available_packet_types = packet_types;

                // Compute packet correlation data
                let correlation = PacketCorrelation::from_qlog(&data);
                info!(
                    "Computed correlation: {} sent packets, {} lost, {} reorderings, {} time gaps, {} congestion states",
                    correlation.sent_packets.len(),
                    correlation.lost_packets.len(),
                    correlation.reorderings.len(),
                    correlation.time_gaps.len(),
                    correlation.congestion_states.len()
                );

                // Update stats view
                self.stats_view.update_stats(&data, &correlation);

                self.packet_correlation = Some(correlation);
                self.qlog_data = Some(data);
                self.loading = false;
                self.filter_stream_id = None;
                self.filter_packet_type = None;
                self.filter_text.clear();
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
        self.render_menu_bar(ctx);
        self.render_event_detail(ctx);
        self.render_main_content(ctx);
    }
}
