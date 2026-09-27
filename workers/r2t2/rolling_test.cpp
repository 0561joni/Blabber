#include "rolling.hpp"
#include "engine/community_models/confucius4_r2t2/text_postprocess.h"
#include <cassert>
#include <iostream>

int main() {
    {
        blabber::RollingTranscript transcript;
        assert(transcript.tentative("Grö") == "Grö");
        assert(transcript.tentative("Größe über") == "Größe über");
        assert(transcript.tentative("Größe") == "Größe");
        assert(transcript.complete.empty() && transcript.prefix.empty());
        transcript.commit(10240, "Größe");
        assert(transcript.tentative("Größe über die Straße") == " über die Straße");
        assert(transcript.tentative("changed").empty());
        assert(transcript.tentative("Grö").empty());
        assert(transcript.tentative("Größe").empty());
        assert(transcript.complete == "Größe");
        transcript.commit(20480, "Größe über");
        assert(transcript.tentative("Größe über die Straße") == " die Straße");
        transcript.advance(10240);
        assert(transcript.prefix == "über");
        assert(transcript.tentative("über die Straße") == " die Straße");
        assert(transcript.complete == "Größe über");
        // Styling may move a combining grapheme, but native bytes stay exact.
        blabber::RollingTranscript unicode;
        unicode.commit(1, "Cafe");
        assert(unicode.tentative("Cafe\u0301.") == "\u0301.");
    }
    {
        using namespace engine::community_models::confucius4_r2t2;
        // Native Auto parsing deliberately leaves language empty for None,
        // even when it returns speech. Exercise the actual parser contract.
        for (const std::string text : {"I", "Größe", "Hello world."}) {
            const auto parsed = parse_asr_output("language None<asr_text>" + text, "");
            assert(parsed.language.empty());
            assert(parsed.text == text);
            blabber::RollingTranscript transcript;
            transcript.commit(10240, parsed.text);
            const auto prefix = blabber::stream_prompt_prefix(transcript.prefix, parsed.language, true);
            assert(prefix == "language None<asr_text>" + text);
            // No new tokens at stop still returns every committed byte.
            const auto final = parse_asr_output(prefix, "");
            assert(contains_asr_text_tag(prefix));
            assert(transcript.commit(10240, final.text, true).empty());
            const auto next = parse_asr_output(prefix + " More.", "");
            assert(transcript.commit(20480, next.text) == " More.");
            assert(transcript.complete == text + " More.");
        }
        assert(blabber::stream_prompt_prefix("", "", true).empty());
        assert(blabber::stream_prompt_prefix("", "English", true) == "language English<asr_text>");
        assert(blabber::stream_prompt_prefix("Hallo", "German", true) == "language German<asr_text>Hallo");
        assert(blabber::stream_prompt_prefix("Hallo", "", false) == "Hallo");
    }
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
