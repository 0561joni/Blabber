use serde::{Deserialize, Serialize};

pub const SHERPA_ONNX_VERSION: &str = "1.13.5";
pub const DIARIZATION_MODEL_ID: &str = "sherpa-diarization-pyannote3-eres2net-v1";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiarizationModelSpec {
    pub manifest_version: u32,
    pub window_shift_ratio: f32,
    pub min_duration_on_seconds: f32,
    pub min_duration_off_seconds: f32,
    pub clustering_threshold: f32,
    pub num_clusters: i32,
}

pub const DIARIZATION_MODEL_SPEC_V1: DiarizationModelSpec = DiarizationModelSpec {
    manifest_version: 1,
    window_shift_ratio: 0.1,
    min_duration_on_seconds: 0.3,
    min_duration_off_seconds: 0.5,
    clustering_threshold: 0.5,
    num_clusters: -1,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawDiarizationTurn {
    pub start_ms: i64,
    pub end_ms: i64,
    pub cluster_ids: Vec<i32>,
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiarizationTurn {
    pub id: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker_ids: Vec<String>,
    pub confidence: Option<f32>,
    pub is_overlap: bool,
    pub is_uncertain: bool,
    pub turn_order: i32,
}

/// Replaces runtime cluster numbers with transcript-local IDs ordered by first appearance.
pub fn normalize_turns(mut turns: Vec<RawDiarizationTurn>) -> Vec<DiarizationTurn> {
    use std::collections::HashMap;
    turns.sort_by_key(|turn| (turn.start_ms, turn.end_ms));
    let mut speakers = HashMap::<i32, String>::new();
    let mut next = 0usize;
    turns
        .into_iter()
        .enumerate()
        .filter_map(|(index, turn)| {
            if turn.end_ms <= turn.start_ms || turn.cluster_ids.is_empty() {
                return None;
            }
            let mut ids = Vec::new();
            for cluster in turn.cluster_ids {
                let id = speakers.entry(cluster).or_insert_with(|| {
                    let id = format!("speaker_{next}");
                    next += 1;
                    id
                });
                if !ids.contains(id) {
                    ids.push(id.clone());
                }
            }
            Some(DiarizationTurn {
                id: format!("turn_{index}"),
                start_ms: turn.start_ms,
                end_ms: turn.end_ms,
                is_overlap: ids.len() > 1,
                speaker_ids: ids,
                confidence: turn.confidence,
                is_uncertain: false,
                turn_order: index as i32,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maps_clusters_by_first_appearance() {
        let turns = normalize_turns(vec![
            RawDiarizationTurn {
                start_ms: 500,
                end_ms: 900,
                cluster_ids: vec![9],
                confidence: None,
            },
            RawDiarizationTurn {
                start_ms: 0,
                end_ms: 400,
                cluster_ids: vec![42],
                confidence: None,
            },
            RawDiarizationTurn {
                start_ms: 900,
                end_ms: 1200,
                cluster_ids: vec![42, 9],
                confidence: None,
            },
        ]);
        assert_eq!(turns[0].speaker_ids, ["speaker_0"]);
        assert_eq!(turns[1].speaker_ids, ["speaker_1"]);
        assert_eq!(turns[2].speaker_ids, ["speaker_0", "speaker_1"]);
        assert!(turns[2].is_overlap);
    }
}
