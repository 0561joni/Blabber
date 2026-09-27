// Exercise the actual opaque C API wrapper without loading weights or a GPU.
#include "capi/audiocpp.cpp"
#include <cassert>
#include <cstring>

int main() {
    const char * preview = nullptr;
    assert(audiocpp_event_tentative_text(nullptr, &preview) == AUDIOCPP_ERR_INVALID_ARGUMENT);
    auto none = wrap_event(rt::StreamEvent{});
    assert(audiocpp_event_tentative_text(none.get(), &preview) == AUDIOCPP_ERR_NOT_AVAILABLE);
    assert(preview == nullptr);

    rt::StreamEvent draft;
    draft.tentative_text = "Größe über die Straße";
    auto event = wrap_event(std::move(draft));
    assert(audiocpp_event_tentative_text(event.get(), &preview) == AUDIOCPP_OK);
    assert(std::strcmp(preview, "Größe über die Straße") == 0);
    assert(audiocpp_result_text(audiocpp_event_as_result(event.get()), nullptr, nullptr) == AUDIOCPP_ERR_NOT_AVAILABLE);

    rt::StreamEvent promoted;
    promoted.partial_text = rt::Transcript{"Größe", "German"};
    promoted.tentative_text = " über";
    event = wrap_event(std::move(promoted));
    const char * committed = nullptr;
    assert(audiocpp_result_text(audiocpp_event_as_result(event.get()), &committed, nullptr) == AUDIOCPP_OK);
    assert(std::strcmp(committed, "Größe") == 0);
    assert(audiocpp_event_tentative_text(event.get(), &preview) == AUDIOCPP_OK);
    assert(std::strcmp(preview, " über") == 0);

    rt::StreamEvent cleared;
    cleared.tentative_text = "";
    event = wrap_event(std::move(cleared));
    assert(audiocpp_event_tentative_text(event.get(), &preview) == AUDIOCPP_OK);
    assert(preview != nullptr && *preview == '\0');
}
