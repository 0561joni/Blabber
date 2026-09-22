#include "rolling.hpp"
#include <cassert>
#include <iostream>

int main() {
    {
        blabber::RollingTranscript held;
        held.commit(100, "hello world");
        assert(held.commit(200, "hello").empty());
        assert(held.complete == "hello world");
        bool rejected = false;
        try { held.commit(200, "hello", true); }
        catch (const std::runtime_error &) { rejected = true; }
        assert(rejected);
    }
    for (int cadence : {5120, 10240, 20480, 32000}) {
        blabber::RollingTranscript transcript;
        std::string expected;
        int64_t start = 0;
        for (int64_t end = cadence; end <= 300 * 16000; end += cadence) {
            if (end - start > 16 * 16000) {
                start += 8 * 16000;
                transcript.advance(start);
            }
            const std::string delta = " Größe " + std::to_string(end);
            expected += delta;
            assert(transcript.commit(end, transcript.prefix + delta) == delta);
            assert(transcript.complete == expected);
            assert(transcript.entries.size() <= 16 * 16000 / cadence + 1);
        }
        // Exact chunk-boundary finish must still append the withheld suffix.
        assert(transcript.commit(transcript.last_sample, transcript.prefix + " Ende.") == " Ende.");
        assert(transcript.complete == expected + " Ende.");
        bool rejected = false;
        try { transcript.commit(transcript.last_sample, "rewritten"); }
        catch (const std::runtime_error &) { rejected = true; }
        assert(rejected);
        rejected = false;
        try { transcript.advance(start - 1); }
        catch (const std::runtime_error &) { rejected = true; }
        assert(rejected);
    }
    std::cout << "rolling sample ledger: passed\n";
}
