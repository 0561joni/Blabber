#pragma once

#include <cstdint>
#include <deque>
#include <stdexcept>
#include <string>

namespace blabber {
// Sample positions identify decode steps, not acoustic word timestamps. This
// fixes cadence-dependent bookkeeping; real-audio continuity is a release gate.
class RollingTranscript {
public:
    struct Entry { int64_t end_sample; std::string text; };
    std::string complete;
    std::string prefix;
    std::deque<Entry> entries;
    int64_t window_start = 0;
    int64_t last_sample = 0;

    void advance(int64_t start) {
        if (start < window_start || start > last_sample)
            throw std::runtime_error("R2T2_SAMPLE_ORDER: invalid rolling boundary");
        window_start = start;
        while (!entries.empty() && entries.front().end_sample <= start) {
            prefix.erase(0, entries.front().text.size());
            entries.pop_front();
        }
        // The native ASR parser strips leading whitespace. Normalize only the
        // retained prompt at a rollover, never the already published transcript.
        while (!prefix.empty() && (prefix[0] == ' ' || prefix[0] == '\n' || prefix[0] == '\r' || prefix[0] == '\t')) {
            prefix.erase(0, 1);
            entries.front().text.erase(0, 1);
            if (entries.front().text.empty()) entries.pop_front();
        }
    }

    std::string commit(int64_t end_sample, const std::string & text, bool final = false) {
        if (end_sample < last_sample)
            throw std::runtime_error("R2T2_SAMPLE_ORDER: audio moved backwards");
        // Byte-prefix comparison is safe because both strings are complete UTF-8
        // strings; never remove a suffix using a guessed code-point count.
        // A rollback can withhold part of the already committed prompt when
        // no new word was generated. That is no update, not a revision. A
        // final result must still include every previously committed byte.
        if (!final && text.size() < prefix.size() && prefix.compare(0, text.size(), text) == 0) {
            last_sample = end_sample;
            return {};
        }
        if (text.compare(0, prefix.size(), prefix) != 0)
            throw std::runtime_error("R2T2_PREFIX_MISMATCH: committed text changed");
        std::string delta = text.substr(prefix.size());
        last_sample = end_sample;
        if (!delta.empty()) {
            entries.push_back({end_sample, delta});
            prefix += delta;
            complete += delta;
        }
        return delta;
    }
};
}
