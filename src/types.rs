use crate::packet_correlation::PacketCorrelation;
use crate::qlog_data::QlogData;
use std::path::PathBuf;

pub struct LoadedFile {
    pub path: PathBuf,
    pub label: String,
    pub qlog_data: QlogData,
    pub packet_correlation: PacketCorrelation,
    pub stream_ids: Vec<u64>,
    pub packet_types: Vec<String>,
}

pub trait CachingVisualization {
    fn invalidate_cache(&mut self);
}
