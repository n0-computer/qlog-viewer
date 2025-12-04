use crate::qlog_data::QlogData;
use crate::utils;
use egui_plot::{Plot, PlotPoints, Polygon};
use qlog::events::{quic::QuicFrame, EventData};
use std::collections::HashMap;

type StreamSegments = HashMap<u64, Vec<(f64, f64, u64)>>;

pub struct MultiplexingDiagram {
    max_segments: usize,
}

impl MultiplexingDiagram {
    pub fn new() -> Self {
        Self { max_segments: 2000 }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, qlog_data: &QlogData) {
        ui.horizontal(|ui| {
            ui.heading("Multiplexing Diagram");
            ui.separator();
            ui.label("Max segments:");
            ui.add(
                egui::DragValue::new(&mut self.max_segments)
                    .range(500..=10000)
                    .speed(100),
            );
        });
        ui.label("Shows stream scheduling and prioritization across the connection");
        ui.separator();

        let (stream_data, total_segments) = self.extract_stream_data(qlog_data);

        let plot = Plot::new("multiplexing_diagram")
            .legend(egui_plot::Legend::default().position(egui_plot::Corner::RightTop))
            .allow_zoom(true)
            .allow_drag(true)
            .allow_scroll(true)
            .x_axis_label("Time (ms)")
            .y_axis_label("Stream ID");

        plot.show(ui, |plot_ui| {
            for (stream_id, segments) in stream_data.iter() {
                let color = utils::stream_color(*stream_id);

                for (start_time, end_time, _bytes) in segments {
                    let polygon = Polygon::new(
                        format!("Stream {}", stream_id),
                        PlotPoints::from(vec![
                            [*start_time, *stream_id as f64 - 0.4],
                            [*end_time, *stream_id as f64 - 0.4],
                            [*end_time, *stream_id as f64 + 0.4],
                            [*start_time, *stream_id as f64 + 0.4],
                        ]),
                    )
                    .fill_color(color.linear_multiply(0.7))
                    .stroke(egui::Stroke::new(1.0, color));

                    plot_ui.polygon(polygon);
                }
            }
        });

        ui.separator();
        let shown_segments: usize = stream_data.values().map(|v| v.len()).sum();
        ui.label(format!(
            "Active streams: {} | Showing {} of {} segments",
            stream_data.len(),
            shown_segments,
            total_segments
        ));
    }

    fn extract_stream_data(&self, qlog_data: &QlogData) -> (StreamSegments, usize) {
        let mut stream_data: StreamSegments = HashMap::new();
        let mut total_segments = 0usize;

        for event in qlog_data.events.iter() {
            let time = event.time as f64;

            match &event.data {
                EventData::PacketSent(data) => {
                    if let Some(ref frames) = data.frames {
                        for frame in frames.iter() {
                            if let QuicFrame::Stream { stream_id, raw, .. } = frame {
                                total_segments += 1;
                                let current_total: usize =
                                    stream_data.values().map(|v| v.len()).sum();
                                if current_total < self.max_segments {
                                    let length =
                                        raw.as_ref().and_then(|r| r.length).unwrap_or(1000);
                                    let duration = (length as f64 / 1000.0).max(0.1);
                                    stream_data.entry(*stream_id).or_default().push((
                                        time,
                                        time + duration,
                                        length,
                                    ));
                                }
                            }
                        }
                    }
                }
                EventData::PacketReceived(data) => {
                    if let Some(ref frames) = data.frames {
                        for frame in frames.iter() {
                            if let QuicFrame::Stream { stream_id, raw, .. } = frame {
                                total_segments += 1;
                                let current_total: usize =
                                    stream_data.values().map(|v| v.len()).sum();
                                if current_total < self.max_segments {
                                    let length =
                                        raw.as_ref().and_then(|r| r.length).unwrap_or(1000);
                                    let duration = (length as f64 / 1000.0).max(0.1);
                                    stream_data.entry(*stream_id).or_default().push((
                                        time,
                                        time + duration,
                                        length,
                                    ));
                                }
                            }
                        }
                    }
                }
                EventData::FramesProcessed(data) => {
                    for frame in data.frames.iter() {
                        if let QuicFrame::Stream { stream_id, raw, .. } = frame {
                            total_segments += 1;
                            let current_total: usize = stream_data.values().map(|v| v.len()).sum();
                            if current_total < self.max_segments {
                                let length = raw.as_ref().and_then(|r| r.length).unwrap_or(1000);
                                let duration = (length as f64 / 1000.0).max(0.1);
                                stream_data.entry(*stream_id).or_default().push((
                                    time,
                                    time + duration,
                                    length,
                                ));
                            }
                        }
                    }
                }
                EventData::StreamStateUpdated(data) => {
                    stream_data.entry(data.stream_id).or_default();
                }
                _ => {}
            }
        }

        (stream_data, total_segments)
    }
}
