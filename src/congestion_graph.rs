use crate::qlog_data::QlogData;
use egui::Color32;
use egui_plot::{Line, Plot, PlotPoints};
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
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, qlog_data: &QlogData) {
        ui.horizontal(|ui| {
            ui.heading("Congestion Graph");
            ui.separator();
            if ui.button("Reset Toggles").clicked() {
                *self = Self::new();
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
        });
        ui.separator();

        let metrics = self.extract_metrics(qlog_data);

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
            .link_cursor("congestion_time", true);

        congestion_plot.show(ui, |plot_ui| {
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
            .link_cursor("congestion_time", true);

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

    fn extract_metrics(&self, qlog_data: &QlogData) -> MetricsData {
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

            match &event.data {
                EventData::MetricsUpdated(data) => {
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
                EventData::PacketSent(data) => {
                    packets_sent += 1;
                    let packet_size = data.raw.as_ref().and_then(|r| r.length).unwrap_or(1200);
                    cumulative_sent += packet_size;
                    data_sent_points.push([time, cumulative_sent as f64]);
                }
                EventData::PacketReceived(data) => {
                    packets_acked += 1;
                    let packet_size = data.raw.as_ref().and_then(|r| r.length).unwrap_or(1200);
                    cumulative_acked += packet_size;
                    data_acked_points.push([time, cumulative_acked as f64]);
                }
                EventData::PacketLost(_data) => {
                    cumulative_lost += 1200;
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
