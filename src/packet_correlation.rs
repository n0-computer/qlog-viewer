use crate::qlog_data::QlogData;
use qlog::events::quic::{AckedRanges, QuicFrame};
use qlog::events::EventData;
use std::collections::HashMap;

const MAX_REORDERINGS: usize = 1000;
const MAX_TIME_GAPS: usize = 1000;
const DEFAULT_GAP_THRESHOLD_MS: f32 = 50.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CongestionState {
    SlowStart,
    CongestionAvoidance,
    Recovery,
    ApplicationLimited,
    Unknown,
}

impl CongestionState {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "slow_start" | "slowstart" => Self::SlowStart,
            "congestion_avoidance" | "congestionavoidance" => Self::CongestionAvoidance,
            "recovery" => Self::Recovery,
            "application_limited" | "applicationlimited" => Self::ApplicationLimited,
            _ => Self::Unknown,
        }
    }

    pub fn color(&self) -> egui::Color32 {
        match self {
            Self::SlowStart => egui::Color32::from_rgba_unmultiplied(0, 200, 0, 30),
            Self::CongestionAvoidance => egui::Color32::from_rgba_unmultiplied(0, 100, 255, 30),
            Self::Recovery => egui::Color32::from_rgba_unmultiplied(255, 50, 50, 30),
            Self::ApplicationLimited => egui::Color32::from_rgba_unmultiplied(255, 200, 0, 30),
            Self::Unknown => egui::Color32::from_rgba_unmultiplied(128, 128, 128, 20),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::SlowStart => "Slow Start",
            Self::CongestionAvoidance => "Congestion Avoidance",
            Self::Recovery => "Recovery",
            Self::ApplicationLimited => "App Limited",
            Self::Unknown => "Unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SentPacketInfo {
    pub time: f32,
    pub ack_time: Option<f32>,
    pub rtt: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct ReorderEvent {
    pub earlier_time: f32,
}

#[derive(Debug, Clone)]
pub struct TimeGap {
    pub start_time: f32,
    pub end_time: f32,
    pub duration: f32,
}

#[derive(Debug, Clone)]
pub struct CongestionPeriod {
    pub state: CongestionState,
    pub start_time: f32,
    pub end_time: f32,
}

#[derive(Debug, Clone, Default)]
pub struct PacketCorrelation {
    // Key: (path_id, packet_type, packet_number)
    // packet_type is the packet space (Initial, Handshake, 1RTT, etc.)
    pub sent_packets: HashMap<(u64, String, u64), SentPacketInfo>,
    pub lost_packets: HashMap<(u64, String, u64), f32>,
    pub reorderings: Vec<ReorderEvent>,
    pub time_gaps: Vec<TimeGap>,
    pub congestion_states: Vec<CongestionPeriod>,
    pub min_time: f32,
    pub max_time: f32,
}

impl PacketCorrelation {
    pub fn from_qlog(qlog: &QlogData) -> Self {
        let mut correlation = Self::default();

        if qlog.events.is_empty() {
            return correlation;
        }

        correlation.min_time = qlog.events.first().map(|e| e.time).unwrap_or(0.0);
        correlation.max_time = qlog.events.last().map(|e| e.time).unwrap_or(0.0);

        correlation.extract_sent_packets(qlog);
        correlation.extract_losses(qlog);
        correlation.correlate_acks(qlog);
        correlation.detect_reorderings(qlog);
        correlation.detect_time_gaps(qlog, DEFAULT_GAP_THRESHOLD_MS);
        correlation.extract_congestion_states(qlog);

        correlation
    }

    fn extract_sent_packets(&mut self, qlog: &QlogData) {
        for event in qlog.events.iter() {
            if let EventData::PacketSent(data) = &event.data {
                if let Some(pn) = data.header.packet_number {
                    let path_id = data.header.path_id.unwrap_or(0);
                    let packet_type = format!("{:?}", data.header.packet_type);
                    self.sent_packets.insert(
                        (path_id, packet_type, pn),
                        SentPacketInfo {
                            time: event.time,
                            ack_time: None,
                            rtt: None,
                        },
                    );
                }
            }
        }
    }

    fn extract_losses(&mut self, qlog: &QlogData) {
        for event in qlog.events.iter() {
            if let EventData::PacketLost(data) = &event.data {
                if let Some(header) = &data.header {
                    if let Some(pn) = header.packet_number {
                        let path_id = header.path_id.unwrap_or(0);
                        let packet_type = format!("{:?}", header.packet_type);
                        self.lost_packets
                            .insert((path_id, packet_type, pn), event.time);
                    }
                }
            }
        }
    }

    fn correlate_acks(&mut self, qlog: &QlogData) {
        // First try PacketsAcked events (most accurate)
        // Note: PacketsAcked doesn't include path_id or packet_type, so we try to match against all combinations
        for event in qlog.events.iter() {
            if let EventData::PacketsAcked(data) = &event.data {
                if let Some(ref packet_numbers) = data.packet_numbers {
                    for &pn in packet_numbers {
                        // Try to find this packet in any path and packet space
                        // Try common packet types first for efficiency
                        let packet_types = ["OneRtt", "Initial", "Handshake", "ZeroRtt"];
                        'outer: for packet_type in &packet_types {
                            for path_id in 0..=255 {
                                if let Some(sent) = self.sent_packets.get_mut(&(
                                    path_id,
                                    packet_type.to_string(),
                                    pn,
                                )) {
                                    if sent.ack_time.is_none() {
                                        sent.ack_time = Some(event.time);
                                        sent.rtt = Some(event.time - sent.time);
                                    }
                                    break 'outer;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Also extract ACKs from received packets' ACK frames
        // In QUIC, ACKs in a packet space acknowledge packets in the same packet space
        for event in &qlog.events {
            let EventData::PacketReceived(data) = &event.data else {
                continue;
            };
            let Some(frames) = &data.frames else { continue };

            // Get path_id and packet_type from the received packet header
            let path_id = data.header.path_id.unwrap_or(0);
            let packet_type = format!("{:?}", data.header.packet_type);

            for frame in frames {
                let QuicFrame::Ack {
                    acked_ranges: Some(ranges),
                    ..
                } = frame
                else {
                    continue;
                };

                let pns: Vec<u64> = match ranges {
                    AckedRanges::Single(nested) => nested.iter().flatten().copied().collect(),
                    AckedRanges::Double(pairs) => pairs.iter().flat_map(|(s, e)| *s..=*e).collect(),
                };

                for pn in pns {
                    // ACK in this packet space acknowledges packets in the same packet space
                    if let Some(sent) =
                        self.sent_packets
                            .get_mut(&(path_id, packet_type.clone(), pn))
                    {
                        if sent.ack_time.is_none() {
                            sent.ack_time = Some(event.time);
                            sent.rtt = Some(event.time - sent.time);
                        }
                    }
                }
            }
        }
    }

    fn detect_reorderings(&mut self, qlog: &QlogData) {
        // Track max received packet number per (path_id, packet_type)
        // Packet numbers are independent per packet space (Initial, Handshake, 1-RTT, etc.)
        let mut max_received_pn_per_path_space: HashMap<(u64, String), u64> = HashMap::new();

        for event in qlog.events.iter() {
            if let EventData::PacketReceived(data) = &event.data {
                if let Some(pn) = data.header.packet_number {
                    let path_id = data.header.path_id.unwrap_or(0);
                    let packet_type = format!("{:?}", data.header.packet_type);
                    let key = (path_id, packet_type.clone());

                    let max_pn = max_received_pn_per_path_space.entry(key).or_insert(0);

                    if pn < *max_pn && self.reorderings.len() < MAX_REORDERINGS {
                        self.reorderings.push(ReorderEvent {
                            earlier_time: event.time,
                        });
                    }
                    *max_pn = (*max_pn).max(pn);
                }
            }
        }

        self.reorderings.sort_by(|a, b| {
            a.earlier_time
                .partial_cmp(&b.earlier_time)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    pub fn detect_time_gaps(&mut self, qlog: &QlogData, threshold_ms: f32) {
        self.time_gaps.clear();

        if qlog.events.len() < 2 {
            return;
        }

        let mut prev_time = qlog.events[0].time;

        for event in qlog.events.iter().skip(1) {
            let gap = event.time - prev_time;
            if gap > threshold_ms && self.time_gaps.len() < MAX_TIME_GAPS {
                self.time_gaps.push(TimeGap {
                    start_time: prev_time,
                    end_time: event.time,
                    duration: gap,
                });
            }
            prev_time = event.time;
        }
    }

    fn extract_congestion_states(&mut self, qlog: &QlogData) {
        let mut current_state = CongestionState::Unknown;
        let mut state_start_time = self.min_time;

        for event in qlog.events.iter() {
            if let EventData::CongestionStateUpdated(data) = &event.data {
                if current_state != CongestionState::Unknown {
                    self.congestion_states.push(CongestionPeriod {
                        state: current_state,
                        start_time: state_start_time,
                        end_time: event.time,
                    });
                }

                current_state = CongestionState::from_str(&format!("{:?}", data.new));
                state_start_time = event.time;
            }
        }

        if current_state != CongestionState::Unknown {
            self.congestion_states.push(CongestionPeriod {
                state: current_state,
                start_time: state_start_time,
                end_time: self.max_time,
            });
        }
    }

    pub fn loss_count(&self) -> usize {
        self.lost_packets.len()
    }

    pub fn visible_time_gaps(&self, start_time: f32, end_time: f32) -> Vec<&TimeGap> {
        self.time_gaps
            .iter()
            .filter(|g| g.end_time >= start_time && g.start_time <= end_time)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_congestion_state_from_str() {
        assert_eq!(
            CongestionState::from_str("slow_start"),
            CongestionState::SlowStart
        );
        assert_eq!(
            CongestionState::from_str("SlowStart"),
            CongestionState::SlowStart
        );
        assert_eq!(
            CongestionState::from_str("congestion_avoidance"),
            CongestionState::CongestionAvoidance
        );
        assert_eq!(
            CongestionState::from_str("recovery"),
            CongestionState::Recovery
        );
        assert_eq!(
            CongestionState::from_str("unknown_state"),
            CongestionState::Unknown
        );
    }

    #[test]
    fn test_default_correlation() {
        let correlation = PacketCorrelation::default();
        assert!(correlation.sent_packets.is_empty());
        assert!(correlation.lost_packets.is_empty());
        assert!(correlation.reorderings.is_empty());
        assert!(correlation.time_gaps.is_empty());
        assert!(correlation.congestion_states.is_empty());
    }
}
