use crate::constants::DEFAULT_PACKET_SIZE;
use crate::packet_correlation::PacketCorrelation;
use crate::qlog_data::QlogData;
use crate::utils::{format_bytes, get_event_path_id};
use egui::Color32;
use egui_plot::{Line, Plot, PlotPoints, Polygon, VLine};
use qlog::events::EventData;

pub struct CongestionGraph {
    show_data_sent: bool,
    show_data_acked: bool,
    show_data_lost: bool,
    show_cwnd: bool,
    show_bytes_in_flight: bool,
    show_smoothed_rtt: bool,
    show_latest_rtt: bool,
    show_min_rtt: bool,
    show_congestion_states: bool,
    show_ecn_states: bool,
    show_time_gaps: bool,
    time_gap_threshold_ms: f64,
    selected_path_id: u64,
}

impl CongestionGraph {
    pub fn new() -> Self {
        Self {
            show_data_sent: true,
            show_data_acked: true,
            show_data_lost: true,
            show_cwnd: true,
            show_bytes_in_flight: true,
            show_smoothed_rtt: true,
            show_latest_rtt: true,
            show_min_rtt: true,
            show_congestion_states: true,
            show_ecn_states: false,
            show_time_gaps: false,
            time_gap_threshold_ms: 50.0,
            selected_path_id: 0,
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        qlog_data: &QlogData,
        correlation: &PacketCorrelation,
    ) {
        // Collect all unique path IDs
        let available_path_ids = Self::collect_path_ids(qlog_data);

        // Ensure selected_path_id is valid, default to first available
        if !available_path_ids.contains(&self.selected_path_id) {
            self.selected_path_id = *available_path_ids.first().unwrap_or(&0);
        }

        ui.horizontal(|ui| {
            ui.heading("Congestion Graph");
            ui.separator();

            // Path ID selector
            if available_path_ids.len() > 1 {
                ui.label("Path:");
                egui::ComboBox::from_id_salt("path_id_selector")
                    .selected_text(format!("Path {}", self.selected_path_id))
                    .show_ui(ui, |ui| {
                        for &path_id in &available_path_ids {
                            ui.selectable_value(
                                &mut self.selected_path_id,
                                path_id,
                                format!("Path {}", path_id),
                            );
                        }
                    });
                ui.separator();
            }

            if ui.button("Reset Toggles").clicked() {
                let current_path = self.selected_path_id;
                *self = Self::new();
                self.selected_path_id = current_path;
            }
        });

        ui.horizontal(|ui| {
            ui.label("Data:");
            ui.checkbox(&mut self.show_data_sent, "Sent");
            ui.checkbox(&mut self.show_data_acked, "Acked");
            ui.checkbox(&mut self.show_data_lost, "Lost");
            ui.separator();
            ui.checkbox(&mut self.show_cwnd, "Cwnd");
            ui.checkbox(&mut self.show_bytes_in_flight, "In Flight");
            ui.separator();
            ui.checkbox(&mut self.show_congestion_states, "CC States");
            ui.checkbox(&mut self.show_ecn_states, "ECN");
            ui.checkbox(&mut self.show_time_gaps, "Time Gaps");
            if self.show_time_gaps {
                ui.add(
                    egui::Slider::new(&mut self.time_gap_threshold_ms, 10.0..=500.0)
                        .text("Gap (ms)"),
                );
            }
        });
        ui.separator();

        let metrics = self.extract_metrics(qlog_data, self.selected_path_id);

        let congestion_plot = Plot::new("congestion_data_plot")
            .legend(egui_plot::Legend::default().position(egui_plot::Corner::LeftTop))
            .allow_zoom(true)
            .allow_drag(true)
            .allow_scroll(true)
            .allow_boxed_zoom(true)
            .x_axis_label("Time (ms)")
            .y_axis_label("Bytes")
            .height(ui.available_height() * 0.45)
            .link_axis("congestion_time", true)
            .link_cursor("congestion_time", true)
            .show_x(true)
            .show_y(true)
            .label_formatter(Self::create_label_formatter(&metrics, true));

        let max_y = metrics.max_y_value();

        congestion_plot.show(ui, |plot_ui| {
            if self.show_congestion_states {
                for period in &correlation.congestion_states {
                    let points = vec![
                        [period.start_time as f64, 0.0],
                        [period.start_time as f64, max_y],
                        [period.end_time as f64, max_y],
                        [period.end_time as f64, 0.0],
                    ];
                    let polygon = Polygon::new(period.state.name(), PlotPoints::from(points))
                        .fill_color(period.state.color())
                        .stroke(egui::Stroke::NONE);
                    plot_ui.polygon(polygon);
                }
            }

            if self.show_ecn_states {
                for period in &correlation.ecn_states {
                    let points = vec![
                        [period.start_time as f64, 0.0],
                        [period.start_time as f64, max_y],
                        [period.end_time as f64, max_y],
                        [period.end_time as f64, 0.0],
                    ];
                    let polygon = Polygon::new(period.name(), PlotPoints::from(points))
                        .fill_color(period.color())
                        .stroke(egui::Stroke::NONE);
                    plot_ui.polygon(polygon);
                }
            }

            if self.show_time_gaps {
                for gap in correlation.visible_time_gaps(correlation.min_time, correlation.max_time)
                {
                    if gap.duration >= self.time_gap_threshold_ms {
                        let intensity = ((gap.duration / 500.0).min(1.0) * 200.0) as u8;
                        let vline = VLine::new("", gap.start_time as f64)
                            .color(Color32::from_rgba_unmultiplied(255, 100, 0, 50 + intensity))
                            .width(2.0);
                        plot_ui.vline(vline);
                    }
                }
            }

            if self.show_data_sent && !metrics.data_sent_points.is_empty() {
                let line = Line::new(
                    "Data sent (includes retransmits)",
                    PlotPoints::from(metrics.data_sent_points.clone()),
                )
                .color(Color32::from_rgb(0, 100, 255))
                .width(2.0);
                plot_ui.line(line);
            }

            if self.show_data_acked && !metrics.data_acked_points.is_empty() {
                let line = Line::new(
                    "Data acknowledged",
                    PlotPoints::from(metrics.data_acked_points.clone()),
                )
                .color(Color32::from_rgb(128, 128, 0))
                .width(2.0);
                plot_ui.line(line);
            }

            if self.show_data_lost && !metrics.data_lost_points.is_empty() {
                let line = Line::new(
                    "Data lost",
                    PlotPoints::from(metrics.data_lost_points.clone()),
                )
                .color(Color32::from_rgb(255, 0, 0))
                .width(2.0);
                plot_ui.line(line);
            }

            if self.show_cwnd && !metrics.cwnd_points.is_empty() {
                let line = Line::new(
                    "Congestion window",
                    PlotPoints::from(metrics.cwnd_points.clone()),
                )
                .color(Color32::from_rgb(180, 0, 180))
                .width(2.0);
                plot_ui.line(line);
            }

            if self.show_bytes_in_flight && !metrics.bytes_in_flight_points.is_empty() {
                let line = Line::new(
                    "Bytes in flight",
                    PlotPoints::from(metrics.bytes_in_flight_points.clone()),
                )
                .color(Color32::from_rgb(100, 100, 0))
                .width(2.0);
                plot_ui.line(line);
            }
        });

        ui.separator();

        ui.horizontal(|ui| {
            ui.label("RTT:");
            ui.checkbox(&mut self.show_min_rtt, "Min");
            ui.checkbox(&mut self.show_latest_rtt, "Latest");
            ui.checkbox(&mut self.show_smoothed_rtt, "Smoothed");
        });
        ui.separator();

        let rtt_plot = Plot::new("rtt_plot")
            .legend(egui_plot::Legend::default().position(egui_plot::Corner::RightBottom))
            .allow_zoom(true)
            .allow_drag(true)
            .allow_scroll(true)
            .allow_boxed_zoom(true)
            .x_axis_label("Time (ms)")
            .y_axis_label("RTT (ms)")
            .height(ui.available_height() * 0.9)
            .link_axis("congestion_time", true)
            .link_cursor("congestion_time", true)
            .show_x(true)
            .show_y(true)
            .label_formatter(Self::create_label_formatter(&metrics, false));

        rtt_plot.show(ui, |plot_ui| {
            if self.show_min_rtt && !metrics.min_rtt_points.is_empty() {
                let line = Line::new("Min RTT", PlotPoints::from(metrics.min_rtt_points.clone()))
                    .color(Color32::from_rgb(255, 150, 200))
                    .width(2.0);
                plot_ui.line(line);
            }

            if self.show_latest_rtt && !metrics.latest_rtt_points.is_empty() {
                let line = Line::new(
                    "Latest RTT",
                    PlotPoints::from(metrics.latest_rtt_points.clone()),
                )
                .color(Color32::from_rgb(255, 165, 0))
                .width(2.0);
                plot_ui.line(line);
            }

            if self.show_smoothed_rtt && !metrics.smoothed_rtt_points.is_empty() {
                let line = Line::new(
                    "Smoothed RTT",
                    PlotPoints::from(metrics.smoothed_rtt_points.clone()),
                )
                .color(Color32::from_rgb(128, 0, 0))
                .width(2.0);
                plot_ui.line(line);
            }
        });

        ui.label(format!(
            "Metrics events: {} | Packets sent: {} | Packets acked: {}",
            metrics.metrics_event_count, metrics.packets_sent, metrics.packets_acked
        ));
    }

    fn collect_path_ids(qlog_data: &QlogData) -> Vec<u64> {
        use std::collections::BTreeSet;

        let mut path_ids = BTreeSet::new();

        for event in qlog_data.events.iter() {
            let path_id = match &event.data {
                EventData::QuicPacketSent(data) => data.header.path_id,
                EventData::QuicPacketReceived(data) => data.header.path_id,
                EventData::QuicPacketLost(data) => data.header.as_ref().and_then(|h| h.path_id),
                EventData::QuicMetricsUpdated(data) => data.path_id,
                _ => None,
            };

            if let Some(pid) = path_id {
                path_ids.insert(pid);
            } else {
                // If no path_id is present, treat as path 0
                path_ids.insert(0);
            }
        }

        path_ids.into_iter().collect()
    }

    fn extract_metrics(&self, qlog_data: &QlogData, path_id: u64) -> MetricsData {
        let mut cwnd_points = Vec::new();
        let mut bytes_in_flight_points = Vec::new();
        let mut smoothed_rtt_points = Vec::new();
        let mut latest_rtt_points = Vec::new();
        let mut min_rtt_points = Vec::new();
        let mut data_sent_points = Vec::new();
        let mut data_acked_points = Vec::new();
        let mut data_lost_points = Vec::new();

        let mut metrics_event_count = 0;
        let mut packets_sent = 0u64;
        let mut packets_acked = 0u64;
        let mut cumulative_sent: u64 = 0;
        let mut cumulative_acked: u64 = 0;
        let mut cumulative_lost: u64 = 0;

        for event in qlog_data.events.iter() {
            let time = event.time as f64;

            let event_path_id = get_event_path_id(&event.data);
            if event_path_id != path_id {
                continue;
            }

            match &event.data {
                EventData::QuicMetricsUpdated(data) => {
                    metrics_event_count += 1;

                    if let Some(cwnd) = data.congestion_window {
                        cwnd_points.push([time, cwnd as f64]);
                    }

                    if let Some(bif) = data.bytes_in_flight {
                        bytes_in_flight_points.push([time, bif as f64]);
                    }

                    if let Some(srtt) = data.smoothed_rtt {
                        smoothed_rtt_points.push([time, srtt as f64]);
                    }

                    if let Some(lrtt) = data.latest_rtt {
                        latest_rtt_points.push([time, lrtt as f64]);
                    }

                    if let Some(mrtt) = data.min_rtt {
                        min_rtt_points.push([time, mrtt as f64]);
                    }
                }
                EventData::QuicPacketSent(data) => {
                    packets_sent += 1;
                    let packet_size = data
                        .raw
                        .as_ref()
                        .and_then(|r| r.length)
                        .unwrap_or(DEFAULT_PACKET_SIZE);
                    cumulative_sent += packet_size;
                    data_sent_points.push([time, cumulative_sent as f64]);
                }
                EventData::QuicPacketReceived(data) => {
                    packets_acked += 1;
                    let packet_size = data
                        .raw
                        .as_ref()
                        .and_then(|r| r.length)
                        .unwrap_or(DEFAULT_PACKET_SIZE);
                    cumulative_acked += packet_size;
                    data_acked_points.push([time, cumulative_acked as f64]);
                }
                EventData::QuicPacketLost(_data) => {
                    cumulative_lost += DEFAULT_PACKET_SIZE;
                    data_lost_points.push([time, cumulative_lost as f64]);
                }
                _ => {}
            }
        }

        let max_points = 2000;

        MetricsData {
            cwnd_points: Self::downsample(cwnd_points, max_points),
            bytes_in_flight_points: Self::downsample(bytes_in_flight_points, max_points),
            smoothed_rtt_points: Self::downsample(smoothed_rtt_points, max_points),
            latest_rtt_points: Self::downsample(latest_rtt_points, max_points),
            min_rtt_points: Self::downsample(min_rtt_points, max_points),
            data_sent_points: Self::downsample(data_sent_points, max_points),
            data_acked_points: Self::downsample(data_acked_points, max_points),
            data_lost_points: Self::downsample(data_lost_points, max_points),
            metrics_event_count,
            packets_sent,
            packets_acked,
        }
    }

    fn downsample(points: Vec<[f64; 2]>, max_points: usize) -> Vec<[f64; 2]> {
        if points.len() <= max_points {
            return points;
        }

        let step = points.len() / max_points;
        points
            .into_iter()
            .step_by(step.max(1))
            .take(max_points)
            .collect()
    }

    fn create_label_formatter(
        metrics: &MetricsData,
        is_congestion_plot: bool,
    ) -> impl Fn(&str, &egui_plot::PlotPoint) -> String {
        let cwnd = metrics.cwnd_points.clone();
        let bif = metrics.bytes_in_flight_points.clone();
        let data_sent = metrics.data_sent_points.clone();
        let data_acked = metrics.data_acked_points.clone();
        let data_lost = metrics.data_lost_points.clone();
        let smoothed_rtt = metrics.smoothed_rtt_points.clone();
        let latest_rtt = metrics.latest_rtt_points.clone();
        let min_rtt = metrics.min_rtt_points.clone();

        move |_name: &str, point: &egui_plot::PlotPoint| {
            let x = point.x;
            let mut lines = vec![format!("Time: {:.2} ms", x)];

            if is_congestion_plot {
                if let Some(val) = Self::find_nearest_value(&data_sent, x) {
                    lines.push(format!("Data Sent: {}", format_bytes(val as u64)));
                }
                if let Some(val) = Self::find_nearest_value(&data_acked, x) {
                    lines.push(format!("Data Acked: {}", format_bytes(val as u64)));
                }
                if let Some(val) = Self::find_nearest_value(&data_lost, x) {
                    lines.push(format!("Data Lost: {}", format_bytes(val as u64)));
                }
                if let Some(val) = Self::find_nearest_value(&cwnd, x) {
                    lines.push(format!("Cwnd: {}", format_bytes(val as u64)));
                }
                if let Some(val) = Self::find_nearest_value(&bif, x) {
                    lines.push(format!("In Flight: {}", format_bytes(val as u64)));
                }
            } else {
                if let Some(val) = Self::find_nearest_value(&smoothed_rtt, x) {
                    lines.push(format!("Smoothed RTT: {:.2} ms", val));
                }
                if let Some(val) = Self::find_nearest_value(&latest_rtt, x) {
                    lines.push(format!("Latest RTT: {:.2} ms", val));
                }
                if let Some(val) = Self::find_nearest_value(&min_rtt, x) {
                    lines.push(format!("Min RTT: {:.2} ms", val));
                }
            }

            lines.join("\n")
        }
    }

    fn find_nearest_value(points: &[[f64; 2]], x: f64) -> Option<f64> {
        if points.is_empty() {
            return None;
        }

        // Binary search for closest point
        let idx = points
            .binary_search_by(|p| p[0].partial_cmp(&x).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or_else(|i| i);

        // Check points around the found index
        let tolerance = 50.0; // ms tolerance for "nearby" points
        let candidates: Vec<_> = [idx.checked_sub(1), Some(idx), Some(idx + 1)]
            .into_iter()
            .flatten()
            .filter_map(|i| points.get(i))
            .filter(|p| (p[0] - x).abs() < tolerance)
            .collect();

        candidates
            .into_iter()
            .min_by(|a, b| {
                (a[0] - x)
                    .abs()
                    .partial_cmp(&(b[0] - x).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|p| p[1])
    }
}

struct MetricsData {
    cwnd_points: Vec<[f64; 2]>,
    bytes_in_flight_points: Vec<[f64; 2]>,
    smoothed_rtt_points: Vec<[f64; 2]>,
    latest_rtt_points: Vec<[f64; 2]>,
    min_rtt_points: Vec<[f64; 2]>,
    data_sent_points: Vec<[f64; 2]>,
    data_acked_points: Vec<[f64; 2]>,
    data_lost_points: Vec<[f64; 2]>,
    metrics_event_count: usize,
    packets_sent: u64,
    packets_acked: u64,
}

impl MetricsData {
    fn max_y_value(&self) -> f64 {
        let max_from_points =
            |points: &[[f64; 2]]| -> f64 { points.iter().map(|p| p[1]).fold(0.0, f64::max) };

        [
            max_from_points(&self.data_sent_points),
            max_from_points(&self.data_acked_points),
            max_from_points(&self.data_lost_points),
            max_from_points(&self.cwnd_points),
            max_from_points(&self.bytes_in_flight_points),
        ]
        .into_iter()
        .fold(0.0, f64::max)
        .max(1.0) // Ensure at least 1.0
    }
}
