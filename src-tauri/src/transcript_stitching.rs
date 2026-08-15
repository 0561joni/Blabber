use strsim::normalized_levenshtein;

use crate::asr::TranscriptSegment;
use crate::transcription_quality::normalize_text;

pub fn stitch_segments(mut segments: Vec<TranscriptSegment>) -> Vec<TranscriptSegment> {
    segments.sort_by_key(|segment| (segment.start_ms, segment.end_ms));
    let mut stitched: Vec<TranscriptSegment> = Vec::with_capacity(segments.len());

    for candidate in segments {
        if let Some(previous) = stitched.last_mut() {
            if is_duplicate_overlap(previous, &candidate) {
                if confidence(&candidate) > confidence(previous) {
                    *previous = candidate;
                }
                continue;
            }
        }
        stitched.push(candidate);
    }

    for (order, segment) in stitched.iter_mut().enumerate() {
        segment.segment_order = order as i32;
    }
    stitched
}

fn is_duplicate_overlap(left: &TranscriptSegment, right: &TranscriptSegment) -> bool {
    let overlap_ms = left.end_ms.min(right.end_ms) - left.start_ms.max(right.start_ms);
    if overlap_ms < -250 {
        return false;
    }

    let left_text = normalize_text(&left.text);
    let right_text = normalize_text(&right.text);
    if left_text.is_empty() || right_text.is_empty() {
        return false;
    }
    if left_text == right_text {
        return true;
    }
    if left_text.chars().count() >= 12
        && (left_text.contains(&right_text) || right_text.contains(&left_text))
    {
        return true;
    }
    normalized_levenshtein(&left_text, &right_text) >= 0.86
}

fn confidence(segment: &TranscriptSegment) -> f32 {
    segment.confidence.unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(start_ms: i64, end_ms: i64, text: &str) -> TranscriptSegment {
        TranscriptSegment {
            id: format!("{start_ms}:{end_ms}"),
            start_ms,
            end_ms,
            text: text.to_string(),
            language_code: "en".to_string(),
            segment_order: 0,
            confidence: Some(0.8),
            speaker_id: None,
            speaker_ids: None,
            speaker_attribution: crate::speaker_reconciliation::SpeakerAttribution::None,
            speaker_confidence: None,
        }
    }

    #[test]
    fn removes_text_duplicated_by_audio_overlap() {
        let stitched = stitch_segments(vec![
            segment(0, 5_000, "We should review the contract."),
            segment(4_500, 5_500, "We should review the contract."),
        ]);
        assert_eq!(stitched.len(), 1);
    }

    #[test]
    fn preserves_same_sentence_at_different_times() {
        let stitched = stitch_segments(vec![
            segment(0, 2_000, "Please confirm the proposal."),
            segment(10_000, 12_000, "Please confirm the proposal."),
        ]);
        assert_eq!(stitched.len(), 2);
    }
}
