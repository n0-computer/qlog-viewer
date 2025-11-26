use anyhow::{Context, Result};
use qlog::{
    events::{Event, EventData},
    Trace, TraceSeq,
};
use serde::Deserialize;
use std::{fs, path::Path};
use tracing::{error, info, warn};

#[derive(Deserialize)]
struct QlogFileSeq {
    qlog_version: Option<String>,
    qlog_format: Option<String>,
    #[allow(dead_code)]
    title: Option<String>,
    #[allow(dead_code)]
    description: Option<String>,
    #[allow(dead_code)]
    trace: TraceSeq,
}

pub struct QlogData {
    pub events: Vec<Event>,
}

impl QlogData {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        info!("Reading file: {:?}", path);

        let content =
            fs::read_to_string(path).with_context(|| format!("Failed to read file: {:?}", path))?;

        info!("File size: {} bytes", content.len());

        if content.starts_with('\x1E') {
            info!("Detected RFC 7464 JSON-SEQ format (starts with \\x1E)");
            Self::from_json_seq(&content)
        } else if content
            .lines()
            .next()
            .is_some_and(|l| l.contains("\"qlog_format\""))
        {
            info!("Detected NDJSON format");
            Self::from_ndjson(&content)
        } else {
            info!("Detected standard JSON format");
            Self::from_json(&content)
        }
    }

    pub fn from_json(json: &str) -> Result<Self> {
        info!("Parsing qlog JSON...");

        let trace: Trace = match sonic_rs::from_str(json) {
            Ok(t) => {
                info!("Successfully parsed with sonic-rs");
                t
            }
            Err(e) => {
                warn!("sonic-rs failed ({}), falling back to serde_json", e);
                serde_json::from_str(json)
                    .with_context(|| "Failed to parse qlog file with serde_json")?
            }
        };

        info!(
            "Successfully parsed trace with {} events",
            trace.events.len()
        );

        Ok(Self {
            events: trace.events,
        })
    }

    pub fn from_ndjson(ndjson: &str) -> Result<Self> {
        info!("Parsing NDJSON qlog...");

        let mut lines = ndjson.lines();

        let first_line = lines
            .next()
            .ok_or_else(|| anyhow::anyhow!("Empty NDJSON file"))?;

        let header: QlogFileSeq = serde_json::from_str(first_line)
            .map_err(|e| {
                error!("Serde error: {}", e);
                e
            })
            .with_context(|| {
                format!(
                    "Failed to parse qlog header from: {}",
                    &first_line[..first_line.len().min(200)]
                )
            })?;

        info!(
            "Parsed trace header (version: {:?}, format: {:?})",
            header.qlog_version, header.qlog_format
        );

        let events = Self::parse_event_lines(lines);
        info!("Parsed {} events", events.len());

        Ok(Self { events })
    }

    pub fn from_json_seq(json_seq: &str) -> Result<Self> {
        info!("Parsing RFC 7464 JSON-SEQ qlog...");

        let records: Vec<&str> = json_seq
            .split('\x1E')
            .filter(|s| !s.trim().is_empty())
            .collect();

        info!("Found {} JSON-SEQ records", records.len());

        let header: QlogFileSeq = if let Some(first_record) = records.first() {
            serde_json::from_str(first_record.trim()).with_context(|| {
                format!(
                    "Failed to parse qlog trace header from: {}",
                    &first_record[..first_record.len().min(200)]
                )
            })?
        } else {
            anyhow::bail!("Empty JSON-SEQ file");
        };

        info!(
            "Parsed trace header (version: {:?}, format: {:?})",
            header.qlog_version, header.qlog_format
        );

        let mut events = Vec::new();
        for (idx, record) in records.iter().skip(1).enumerate() {
            match serde_json::from_str::<Event>(record.trim_start_matches('\n')) {
                Ok(event) => events.push(event),
                Err(e) => {
                    warn!("Failed to parse event {}: {}", idx, e);
                }
            }
        }

        info!("Parsed {} events", events.len());
        Ok(Self { events })
    }

    fn parse_event_lines<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<Event> {
        let mut events = Vec::new();
        let mut parse_errors = 0;

        for (idx, line) in lines.enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            match serde_json::from_str::<Event>(line) {
                Ok(event) => events.push(event),
                Err(e) => {
                    parse_errors += 1;
                    if parse_errors <= 5 {
                        warn!("Failed to parse event {}: {}", idx, e);
                    }
                }
            }
        }

        if parse_errors > 5 {
            warn!("Total parse errors: {} (showing first 5)", parse_errors);
        }

        events
    }

    pub fn get_event_name(&self, event: &Event) -> String {
        match &event.data {
            EventData::PacketSent(_) => "transport:packet_sent",
            EventData::PacketReceived(_) => "transport:packet_received",
            EventData::PacketDropped(_) => "transport:packet_dropped",
            EventData::PacketBuffered(_) => "transport:packet_buffered",
            EventData::PacketLost(_) => "recovery:packet_lost",
            EventData::PacketsAcked(_) => "recovery:packets_acked",
            EventData::FramesProcessed(_) => "transport:frames_processed",
            EventData::MetricsUpdated(_) => "recovery:metrics_updated",
            EventData::CongestionStateUpdated(_) => "recovery:congestion_state_updated",
            EventData::LossTimerUpdated(_) => "recovery:loss_timer_updated",
            EventData::DatagramsSent(_) => "transport:datagrams_sent",
            EventData::DatagramsReceived(_) => "transport:datagrams_received",
            EventData::DatagramDropped(_) => "transport:datagram_dropped",
            EventData::StreamStateUpdated(_) => "transport:stream_state_updated",
            EventData::H3ParametersSet(_) => "http:parameters_set",
            EventData::ConnectionStarted(_) => "connectivity:connection_started",
            EventData::ConnectionClosed(_) => "connectivity:connection_closed",
            EventData::ConnectionStateUpdated(_) => "connectivity:connection_state_updated",
            EventData::VersionInformation(_) => "transport:version_information",
            EventData::AlpnInformation(_) => "transport:alpn_information",
            _ => "unknown",
        }
        .to_string()
    }

    pub fn format_time(&self, event: &Event) -> String {
        format!("{:.3}ms", event.time)
    }

    pub fn get_event_summary(&self, event: &Event) -> String {
        match &event.data {
            EventData::PacketSent(d) => {
                format!(
                    "PN: {:?}, Type: {:?}",
                    d.header.packet_number.unwrap_or(0),
                    d.header.packet_type.clone()
                )
            }
            EventData::PacketReceived(d) => {
                format!(
                    "PN: {:?}, Type: {:?}",
                    d.header.packet_number.unwrap_or(0),
                    d.header.packet_type.clone()
                )
            }
            EventData::StreamStateUpdated(d) => {
                format!("Stream: {}, State: {:?}", d.stream_id, d.new)
            }
            EventData::PacketLost(d) => {
                if let Some(header) = &d.header {
                    format!(
                        "PN: {:?}, Type: {:?}",
                        header.packet_number.unwrap_or(0),
                        header.packet_type.clone()
                    )
                } else {
                    "Lost packet".to_string()
                }
            }
            EventData::MetricsUpdated(d) => {
                let mut parts = Vec::new();
                if let Some(cwnd) = d.congestion_window {
                    parts.push(format!("cwnd: {}", cwnd));
                }
                if let Some(rtt) = d.smoothed_rtt {
                    parts.push(format!("srtt: {:.2}ms", rtt));
                }
                if let Some(bif) = d.bytes_in_flight {
                    parts.push(format!("bif: {}", bif));
                }
                parts.join(", ")
            }
            EventData::FramesProcessed(d) => {
                format!("Frames: {}", d.frames.len())
            }
            _ => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STANDARD_JSON: &str = r#"{
        "qlog_version": "0.3",
        "qlog_format": "JSON",
        "title": "test",
        "vantage_point": {"type": "client"},
        "common_fields": {"protocol_type": ["QUIC"]},
        "events": [
            {"time": 0.0, "name": "connectivity:connection_started", "data": {"ip_version": "ipv4", "src_ip": "127.0.0.1", "dst_ip": "127.0.0.1"}},
            {"time": 1.5, "name": "recovery:metrics_updated", "data": {"congestion_window": 14720}}
        ]
    }"#;

    fn make_ndjson() -> String {
        let header = r#"{"qlog_version":"0.3","qlog_format":"JSON-SEQ","title":"test","trace":{"vantage_point":{"type":"client"},"common_fields":{"protocol_type":["QUIC"]}}}"#;
        let event1 = r#"{"time":0.0,"name":"connectivity:connection_started","data":{"ip_version":"ipv4","src_ip":"127.0.0.1","dst_ip":"127.0.0.1"}}"#;
        let event2 =
            r#"{"time":1.5,"name":"recovery:metrics_updated","data":{"congestion_window":14720}}"#;
        format!("{}\n{}\n{}", header, event1, event2)
    }

    fn make_json_seq() -> String {
        let header = r#"{"qlog_version":"0.3","qlog_format":"JSON-SEQ","title":"test","trace":{"vantage_point":{"type":"client"},"common_fields":{"protocol_type":["QUIC"]}}}"#;
        let event1 = r#"{"time":0.0,"name":"connectivity:connection_started","data":{"ip_version":"ipv4","src_ip":"127.0.0.1","dst_ip":"127.0.0.1"}}"#;
        let event2 =
            r#"{"time":1.5,"name":"recovery:metrics_updated","data":{"congestion_window":14720}}"#;
        format!("\x1E{}\n\x1E{}\n\x1E{}\n", header, event1, event2)
    }

    #[test]
    fn test_parse_standard_json() {
        let result = QlogData::from_json(STANDARD_JSON);
        assert!(
            result.is_ok(),
            "Failed to parse standard JSON: {:?}",
            result.err()
        );
        let data = result.unwrap();
        assert_eq!(data.events.len(), 2);
        assert_eq!(data.events[0].time, 0.0);
        assert_eq!(data.events[1].time, 1.5);
    }

    #[test]
    fn test_parse_ndjson() {
        let ndjson = make_ndjson();
        let result = QlogData::from_ndjson(&ndjson);
        assert!(result.is_ok(), "Failed to parse NDJSON: {:?}", result.err());
        let data = result.unwrap();
        assert_eq!(data.events.len(), 2);
        assert_eq!(data.events[0].time, 0.0);
        assert_eq!(data.events[1].time, 1.5);
    }

    #[test]
    fn test_parse_json_seq() {
        let json_seq = make_json_seq();
        let result = QlogData::from_json_seq(&json_seq);
        assert!(
            result.is_ok(),
            "Failed to parse JSON-SEQ: {:?}",
            result.err()
        );
        let data = result.unwrap();
        assert_eq!(data.events.len(), 2);
        assert_eq!(data.events[0].time, 0.0);
        assert_eq!(data.events[1].time, 1.5);
    }

    #[test]
    fn test_format_detection_standard_json() {
        // Standard JSON doesn't start with \x1E and first line doesn't have qlog_format
        let content = STANDARD_JSON;
        assert!(!content.starts_with('\x1E'));
        assert!(!content.lines().next().unwrap().contains("\"qlog_format\""));
    }

    #[test]
    fn test_format_detection_ndjson() {
        let ndjson = make_ndjson();
        assert!(!ndjson.starts_with('\x1E'));
        assert!(ndjson.lines().next().unwrap().contains("\"qlog_format\""));
    }

    #[test]
    fn test_format_detection_json_seq() {
        let json_seq = make_json_seq();
        assert!(json_seq.starts_with('\x1E'));
    }

    #[test]
    fn test_event_name_extraction() {
        let data = QlogData::from_json(STANDARD_JSON).unwrap();
        assert_eq!(
            data.get_event_name(&data.events[0]),
            "connectivity:connection_started"
        );
        assert_eq!(
            data.get_event_name(&data.events[1]),
            "recovery:metrics_updated"
        );
    }

    #[test]
    fn test_time_formatting() {
        let data = QlogData::from_json(STANDARD_JSON).unwrap();
        assert_eq!(data.format_time(&data.events[0]), "0.000ms");
        assert_eq!(data.format_time(&data.events[1]), "1.500ms");
    }

    #[test]
    fn test_empty_ndjson_fails() {
        let result = QlogData::from_ndjson("");
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_json_seq_fails() {
        let result = QlogData::from_json_seq("");
        assert!(result.is_err());
    }
}
