use crate::constants::DEFAULT_PACKET_SIZE;
use crate::packet_correlation::PacketCorrelation;
use crate::qlog_data::QlogData;
use crate::utils::format_bytes;
use egui::{RichText, Ui};
use qlog::events::EventData;
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct StreamStats {
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub frames_sent: u64,
    pub frames_received: u64,
}

#[derive(Debug, Clone)]
pub struct ConnectionStats {
    pub packets_sent: u64,
    pub packets_received: u64,
    pub packets_lost: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub loss_rate: f32,
    pub min_rtt: Option<f32>,
    pub avg_rtt: Option<f32>,
    pub max_rtt: Option<f32>,
    pub handshake_duration: Option<f32>,
    pub total_duration: f32,
    pub stream_stats: HashMap<u64, StreamStats>,
    pub first_event_time: f32,
    pub last_event_time: f32,
}

impl Default for ConnectionStats {
    fn default() -> Self {
        Self {
            packets_sent: 0,
            packets_received: 0,
            packets_lost: 0,
            bytes_sent: 0,
            bytes_received: 0,
            loss_rate: 0.0,
            min_rtt: None,
            avg_rtt: None,
            max_rtt: None,
            handshake_duration: None,
            total_duration: 0.0,
            stream_stats: HashMap::new(),
            first_event_time: 0.0,
            last_event_time: 0.0,
        }
    }
}

impl ConnectionStats {
    pub fn from_qlog(qlog: &QlogData, correlation: &PacketCorrelation) -> Self {
        let mut stats = Self::default();

        if qlog.events.is_empty() {
            return stats;
        }

        stats.first_event_time = qlog.events.first().map(|e| e.time).unwrap_or(0.0);
        stats.last_event_time = qlog.events.last().map(|e| e.time).unwrap_or(0.0);
        stats.total_duration = stats.last_event_time - stats.first_event_time;

        let mut handshake_start: Option<f32> = None;
        let mut handshake_end: Option<f32> = None;
        let mut rtt_samples: Vec<f32> = Vec::new();

        for event in &qlog.events {
            match &event.data {
                EventData::PacketSent(data) => {
                    stats.packets_sent += 1;
                    // Try to get packet size from raw, or estimate from frames
                    let packet_bytes =
                        data.raw.as_ref().and_then(|r| r.length).unwrap_or_else(|| {
                            // Estimate from frames if raw not available
                            data.frames
                                .as_ref()
                                .map(|frames| {
                                    frames.iter().fold(0u64, |acc, frame| {
                                        acc + Self::estimate_frame_size(frame)
                                    }) + 20
                                })
                                .unwrap_or(DEFAULT_PACKET_SIZE)
                        });
                    stats.bytes_sent += packet_bytes;

                    if let Some(ref frames) = data.frames {
                        for frame in frames {
                            if let qlog::events::quic::QuicFrame::Stream {
                                stream_id, raw, ..
                            } = frame
                            {
                                let length = raw.as_ref().and_then(|r| r.length).unwrap_or(0);
                                let stream_stats =
                                    stats.stream_stats.entry(*stream_id).or_default();
                                stream_stats.frames_sent += 1;
                                stream_stats.bytes_sent += length;
                            }
                        }
                    }
                }
                EventData::PacketReceived(data) => {
                    stats.packets_received += 1;
                    // Try to get packet size from raw, or estimate from frames
                    let packet_bytes =
                        data.raw.as_ref().and_then(|r| r.length).unwrap_or_else(|| {
                            data.frames
                                .as_ref()
                                .map(|frames| {
                                    frames.iter().fold(0u64, |acc, frame| {
                                        acc + Self::estimate_frame_size(frame)
                                    }) + 20
                                })
                                .unwrap_or(DEFAULT_PACKET_SIZE)
                        });
                    stats.bytes_received += packet_bytes;

                    if let Some(ref frames) = data.frames {
                        for frame in frames {
                            if let qlog::events::quic::QuicFrame::Stream {
                                stream_id, raw, ..
                            } = frame
                            {
                                let length = raw.as_ref().and_then(|r| r.length).unwrap_or(0);
                                let stream_stats =
                                    stats.stream_stats.entry(*stream_id).or_default();
                                stream_stats.frames_received += 1;
                                stream_stats.bytes_received += length;
                            }
                        }
                    }
                }
                EventData::PacketLost(_) => {
                    stats.packets_lost += 1;
                }
                EventData::MetricsUpdated(data) => {
                    if let Some(rtt) = data.latest_rtt {
                        rtt_samples.push(rtt);
                        stats.min_rtt = Some(stats.min_rtt.map_or(rtt, |m| m.min(rtt)));
                        stats.max_rtt = Some(stats.max_rtt.map_or(rtt, |m| m.max(rtt)));
                    }
                }
                EventData::ConnectionStarted(_) => {
                    if handshake_start.is_none() {
                        handshake_start = Some(event.time);
                    }
                }
                EventData::ConnectionStateUpdated(data) => {
                    let state_str = format!("{:?}", data.new);
                    if (state_str.to_lowercase().contains("handshake_done")
                        || state_str.to_lowercase().contains("connected"))
                        && handshake_end.is_none()
                    {
                        handshake_end = Some(event.time);
                    }
                }
                _ => {}
            }
        }

        stats.packets_lost = correlation.loss_count() as u64;

        if stats.packets_sent > 0 {
            stats.loss_rate = stats.packets_lost as f32 / stats.packets_sent as f32 * 100.0;
        }

        if !rtt_samples.is_empty() {
            let sum: f32 = rtt_samples.iter().sum();
            stats.avg_rtt = Some(sum / rtt_samples.len() as f32);
        }

        if let (Some(start), Some(end)) = (handshake_start, handshake_end) {
            stats.handshake_duration = Some(end - start);
        }

        stats
    }

    fn estimate_frame_size(frame: &qlog::events::quic::QuicFrame) -> u64 {
        use qlog::events::quic::QuicFrame;
        match frame {
            QuicFrame::Stream { raw, .. } => raw.as_ref().and_then(|r| r.length).unwrap_or(0) + 3,
            QuicFrame::Crypto { raw, .. } => raw.as_ref().and_then(|r| r.length).unwrap_or(0) + 2,
            QuicFrame::Ack { .. } => 20, // Estimate for ACK frame
            QuicFrame::Padding { .. } => 1,
            QuicFrame::Ping { .. } => 1,
            QuicFrame::ResetStream { .. } => 15,
            QuicFrame::StopSending { .. } => 10,
            QuicFrame::NewConnectionId { .. } => 30,
            QuicFrame::RetireConnectionId { .. } => 5,
            QuicFrame::PathChallenge { .. } => 9,
            QuicFrame::PathResponse { .. } => 9,
            QuicFrame::ConnectionClose { .. } => 20,
            QuicFrame::HandshakeDone { .. } => 1,
            QuicFrame::MaxData { .. } => 8,
            QuicFrame::MaxStreamData { .. } => 10,
            QuicFrame::MaxStreams { .. } => 8,
            QuicFrame::DataBlocked { .. } => 8,
            QuicFrame::StreamDataBlocked { .. } => 10,
            QuicFrame::StreamsBlocked { .. } => 8,
            QuicFrame::NewToken { .. } => 50,
            _ => 10, // Default estimate for unknown frames
        }
    }
}

pub struct StatsView {
    stats: Option<ConnectionStats>,
}

impl StatsView {
    pub fn new() -> Self {
        Self { stats: None }
    }

    pub fn update_stats(&mut self, qlog: &QlogData, correlation: &PacketCorrelation) {
        self.stats = Some(ConnectionStats::from_qlog(qlog, correlation));
    }

    pub fn show(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading("Connection Statistics");
        });
        ui.separator();

        let Some(ref stats) = self.stats else {
            ui.label("No data loaded");
            return;
        };

        egui::ScrollArea::vertical().show(ui, |ui| {
            self.show_summary_section(ui, stats);
            ui.add_space(16.0);
            self.show_rtt_section(ui, stats);
            ui.add_space(16.0);
            self.show_timing_section(ui, stats);
            ui.add_space(16.0);
            self.show_stream_section(ui, stats);
        });
    }

    fn show_summary_section(&self, ui: &mut Ui, stats: &ConnectionStats) {
        ui.heading("Summary");
        ui.separator();

        egui::Grid::new("summary_grid")
            .num_columns(2)
            .spacing([40.0, 4.0])
            .show(ui, |ui| {
                ui.label("Packets Sent:");
                ui.label(RichText::new(format!("{}", stats.packets_sent)).strong());
                ui.end_row();

                ui.label("Packets Received:");
                ui.label(RichText::new(format!("{}", stats.packets_received)).strong());
                ui.end_row();

                ui.label("Packets Lost:");
                let loss_color = if stats.packets_lost > 0 {
                    egui::Color32::RED
                } else {
                    egui::Color32::GREEN
                };
                ui.label(
                    RichText::new(format!("{}", stats.packets_lost))
                        .strong()
                        .color(loss_color),
                );
                ui.end_row();

                ui.label("Loss Rate:");
                let rate_color = if stats.loss_rate > 5.0 {
                    egui::Color32::RED
                } else if stats.loss_rate > 1.0 {
                    egui::Color32::YELLOW
                } else {
                    egui::Color32::GREEN
                };
                ui.label(
                    RichText::new(format!("{:.2}%", stats.loss_rate))
                        .strong()
                        .color(rate_color),
                );
                ui.end_row();

                ui.label("Bytes Sent:");
                ui.label(RichText::new(format_bytes(stats.bytes_sent)).strong());
                ui.end_row();

                ui.label("Bytes Received:");
                ui.label(RichText::new(format_bytes(stats.bytes_received)).strong());
                ui.end_row();
            });
    }

    fn show_rtt_section(&self, ui: &mut Ui, stats: &ConnectionStats) {
        ui.heading("RTT Statistics");
        ui.separator();

        egui::Grid::new("rtt_grid")
            .num_columns(2)
            .spacing([40.0, 4.0])
            .show(ui, |ui| {
                ui.label("Min RTT:");
                ui.label(RichText::new(Self::format_rtt(stats.min_rtt)).strong());
                ui.end_row();

                ui.label("Avg RTT:");
                ui.label(RichText::new(Self::format_rtt(stats.avg_rtt)).strong());
                ui.end_row();

                ui.label("Max RTT:");
                ui.label(RichText::new(Self::format_rtt(stats.max_rtt)).strong());
                ui.end_row();
            });
    }

    fn show_timing_section(&self, ui: &mut Ui, stats: &ConnectionStats) {
        ui.heading("Timing");
        ui.separator();

        egui::Grid::new("timing_grid")
            .num_columns(2)
            .spacing([40.0, 4.0])
            .show(ui, |ui| {
                ui.label("Total Duration:");
                ui.label(RichText::new(format!("{:.2} ms", stats.total_duration)).strong());
                ui.end_row();

                ui.label("Handshake Duration:");
                let hs_text = match stats.handshake_duration {
                    Some(d) => format!("{:.2} ms", d),
                    None => "N/A".to_string(),
                };
                ui.label(RichText::new(hs_text).strong());
                ui.end_row();

                if stats.total_duration > 0.0 && stats.bytes_sent > 0 {
                    let throughput =
                        stats.bytes_sent as f64 / (stats.total_duration as f64 / 1000.0);
                    ui.label("Avg Send Throughput:");
                    ui.label(
                        RichText::new(format!("{}/s", format_bytes(throughput as u64))).strong(),
                    );
                    ui.end_row();
                }

                if stats.total_duration > 0.0 && stats.bytes_received > 0 {
                    let throughput =
                        stats.bytes_received as f64 / (stats.total_duration as f64 / 1000.0);
                    ui.label("Avg Recv Throughput:");
                    ui.label(
                        RichText::new(format!("{}/s", format_bytes(throughput as u64))).strong(),
                    );
                    ui.end_row();
                }
            });
    }

    fn show_stream_section(&self, ui: &mut Ui, stats: &ConnectionStats) {
        if stats.stream_stats.is_empty() {
            return;
        }

        ui.heading("Per-Stream Statistics");
        ui.separator();

        let mut streams: Vec<_> = stats.stream_stats.iter().collect();
        streams.sort_by_key(|(id, _)| *id);

        egui::Grid::new("stream_grid")
            .num_columns(5)
            .spacing([20.0, 4.0])
            .striped(true)
            .show(ui, |ui| {
                ui.label(RichText::new("Stream ID").strong());
                ui.label(RichText::new("Bytes Sent").strong());
                ui.label(RichText::new("Bytes Recv").strong());
                ui.label(RichText::new("Frames Sent").strong());
                ui.label(RichText::new("Frames Recv").strong());
                ui.end_row();

                for (stream_id, stream_stats) in streams.iter().take(50) {
                    ui.label(format!("{}", stream_id));
                    ui.label(format_bytes(stream_stats.bytes_sent));
                    ui.label(format_bytes(stream_stats.bytes_received));
                    ui.label(format!("{}", stream_stats.frames_sent));
                    ui.label(format!("{}", stream_stats.frames_received));
                    ui.end_row();
                }
            });

        if stats.stream_stats.len() > 50 {
            ui.label(format!(
                "... and {} more streams",
                stats.stream_stats.len() - 50
            ));
        }
    }

    fn format_rtt(rtt: Option<f32>) -> String {
        match rtt {
            Some(r) => format!("{:.2} ms", r),
            None => "N/A".to_string(),
        }
    }
}
