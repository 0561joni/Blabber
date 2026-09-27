#include "audiocpp.h"
#include "cJSON.h"
#include <chrono>
#include <cmath>
#include <cstdlib>
#include <iostream>
#include <memory>
#include <stdexcept>
#include <string>
#include <vector>
#include <sys/resource.h>

namespace {
using Json = std::unique_ptr<cJSON, decltype(&cJSON_Delete)>;
using Clock = std::chrono::steady_clock;
void check(audiocpp_status status) {
    if (status != AUDIOCPP_OK) throw std::runtime_error(audiocpp_last_error());
}
std::string string(const cJSON * j, const char * key, const std::string & fallback = "") {
    const auto * v = cJSON_GetObjectItemCaseSensitive(j, key);
    if (!v) return fallback;
    if (!cJSON_IsString(v)) throw std::runtime_error("R2T2_PROTOCOL: expected string");
    return v->valuestring;
}
int64_t integer(const cJSON * j, const char * key, int64_t fallback = -1) {
    const auto * v = cJSON_GetObjectItemCaseSensitive(j, key);
    if (!v) return fallback;
    if (!cJSON_IsNumber(v) || !std::isfinite(v->valuedouble) || v->valuedouble < 0 ||
        v->valuedouble > 9007199254740991.0 || std::floor(v->valuedouble) != v->valuedouble)
        throw std::runtime_error("R2T2_PROTOCOL: expected nonnegative integer");
    return static_cast<int64_t>(v->valuedouble);
}

struct Worker {
    audiocpp_registry * registry = nullptr;
    audiocpp_model * model = nullptr;
    audiocpp_session * session = nullptr;
    std::string model_path, session_id, committed, tentative;
    int chunk_ms = 320, rollback = 1;
    int64_t input_sequence = 0, output_sequence = 0, consumed = 0;
    bool active = false, received_tail = false;
    Clock::time_point began = Clock::now();
    ~Worker() { unload(); audiocpp_registry_free(registry); }
    void unload() {
        audiocpp_session_free(session); session = nullptr;
        audiocpp_model_free(model); model = nullptr;
        model_path.clear(); tentative.clear(); active = false;
    }
    void emit(const char * type, const std::string & text = "", const std::string & code = "") {
        Json j(cJSON_CreateObject(), cJSON_Delete);
        cJSON_AddNumberToObject(j.get(), "version", 1);
        cJSON_AddStringToObject(j.get(), "sessionId", session_id.c_str());
        cJSON_AddNumberToObject(j.get(), "sequence", ++output_sequence);
        cJSON_AddStringToObject(j.get(), "type", type);
        cJSON_AddStringToObject(j.get(), "text", text.c_str());
        cJSON_AddStringToObject(j.get(), "tentativeText", tentative.c_str());
        cJSON_AddStringToObject(j.get(), "code", code.c_str());
        cJSON_AddNumberToObject(j.get(), "processedSamples", consumed);
        cJSON_AddNumberToObject(j.get(), "elapsedMs", std::chrono::duration<double, std::milli>(Clock::now() - began).count());
        struct rusage usage{};
        if (getrusage(RUSAGE_SELF, &usage) == 0) cJSON_AddNumberToObject(j.get(), "peakRssBytes", usage.ru_maxrss);
        char * encoded = cJSON_PrintUnformatted(j.get());
        if (!encoded) throw std::bad_alloc();
        std::cout << encoded << '\n' << std::flush;
        cJSON_free(encoded);
    }
    void start(const cJSON * j) {
        if (active) throw std::runtime_error("R2T2_PROTOCOL: session already active");
        session_id = string(j, "sessionId");
        if (session_id.empty() || session_id.size() > 128) throw std::runtime_error("R2T2_PROTOCOL: invalid session id");
        input_sequence = integer(j, "sequence"); output_sequence = 0;
        if (input_sequence != 0) throw std::runtime_error("R2T2_PROTOCOL: start sequence must be zero");
        consumed = 0; received_tail = false; committed.clear(); tentative.clear(); began = Clock::now();
        const auto path = string(j, "modelPath");
        const auto ms = integer(j, "chunkMs", 320);
        const auto tokens = integer(j, "rollbackTokens", 1);
        if ((ms != 320 && ms != 640 && ms != 1280 && ms != 2000) || (tokens != 1 && tokens != 5))
            throw std::runtime_error("R2T2_PROTOCOL: unsupported decode configuration");
        if (!session || model_path != path || chunk_ms != ms || rollback != tokens) {
            unload();
            if (!registry) check(audiocpp_registry_create(nullptr, &registry));
            audiocpp_model_config config{};
            config.family_hint = "confucius4_r2t2";
            check(audiocpp_model_load(registry, path.c_str(), &config, nullptr, &model));
            auto * opts = audiocpp_options_create();
            std::unique_ptr<audiocpp_options, decltype(&audiocpp_options_free)> options(opts, audiocpp_options_free);
            check(audiocpp_options_set(opts, "confucius4_r2t2.chunk_size_ms", std::to_string(ms).c_str()));
            check(audiocpp_options_set(opts, "confucius4_r2t2.unfixed_token_num", std::to_string(tokens).c_str()));
            check(audiocpp_options_set(opts, "confucius4_r2t2.max_tokens", "32"));
            audiocpp_backend_config backend{"metal", 0, 6};
            check(audiocpp_session_create(model, "asr", "streaming", &backend, opts, &session));
            model_path = path; chunk_ms = static_cast<int>(ms); rollback = static_cast<int>(tokens);
        }
        std::unique_ptr<audiocpp_request, decltype(&audiocpp_request_free)> request(audiocpp_request_create(), audiocpp_request_free);
        check(audiocpp_request_set_text(request.get(), string(j, "context").c_str(), string(j, "language").c_str()));
        check(audiocpp_request_set_audio(request.get(), nullptr, 0, 16000, 1));
        check(audiocpp_stream_start(session, request.get()));
        active = true;
        emit("ready");
    }
    void handle(const cJSON * j) {
        if (integer(j, "version") != 1) throw std::runtime_error("R2T2_PROTOCOL: unsupported version");
        const auto type = string(j, "type");
        if (type == "start") { start(j); return; }
        if (!active || string(j, "sessionId") != session_id || integer(j, "sequence") != input_sequence + 1)
            throw std::runtime_error("R2T2_PROTOCOL: stale or unordered request");
        ++input_sequence;
        if (type == "audio") {
            const auto * values = cJSON_GetObjectItemCaseSensitive(j, "samples");
            const int count = cJSON_GetArraySize(values);
            if (!cJSON_IsArray(values) || count <= 0 || count > chunk_ms * 16 || received_tail ||
                integer(j, "startSample") != consumed || consumed + count > 300 * 16000)
                throw std::runtime_error("R2T2_PROTOCOL: invalid audio frame");
            std::vector<float> samples;
            samples.reserve(count);
            const cJSON * value = nullptr;
            cJSON_ArrayForEach(value, values) {
                if (!cJSON_IsNumber(value) || !std::isfinite(value->valuedouble) || std::abs(value->valuedouble) > 1.0)
                    throw std::runtime_error("R2T2_PROTOCOL: invalid sample");
                samples.push_back(static_cast<float>(value->valuedouble));
            }
            audiocpp_event * event = nullptr;
            check(audiocpp_stream_push(session, samples.data(), samples.size(), 16000, 1, consumed, &event));
            std::unique_ptr<audiocpp_event, decltype(&audiocpp_event_free)> owner(event, audiocpp_event_free);
            consumed += count;
            received_tail = count < chunk_ms * 16;
            if (event) {
                const char * delta = nullptr;
                const auto status = audiocpp_result_text(audiocpp_event_as_result(event), &delta, nullptr);
                if (status != AUDIOCPP_ERR_NOT_AVAILABLE) check(status);
                if (delta) committed += delta;
                const char * preview = nullptr;
                const auto preview_status = audiocpp_event_tentative_text(event, &preview);
                if (preview_status != AUDIOCPP_ERR_NOT_AVAILABLE) {
                    check(preview_status);
                    tentative = preview ? preview : "";
                }
            }
            emit("progress", committed);
        } else if (type == "finish") {
            if (integer(j, "totalSamples") != consumed)
                throw std::runtime_error("R2T2_PROTOCOL: missing audio at finish");
            audiocpp_result * result = nullptr;
            check(audiocpp_stream_finish(session, &result));
            std::unique_ptr<audiocpp_result, decltype(&audiocpp_result_free)> owner(result, audiocpp_result_free);
            const char * text = nullptr;
            check(audiocpp_result_text(result, &text, nullptr));
            std::string final_text = text ? text : "";
            if (final_text.compare(0, committed.size(), committed) != 0)
                throw std::runtime_error("R2T2_PREFIX_MISMATCH: final text changed committed text");
            check(audiocpp_stream_reset(session));
            active = false; tentative.clear();
            emit("result", final_text);
        } else if (type == "cancel" || type == "reset") {
            check(audiocpp_stream_reset(session)); active = false; tentative.clear(); emit("canceled");
        } else throw std::runtime_error("R2T2_PROTOCOL: unknown request");
    }
};
}

int main() {
    Worker worker;
    try {
        std::string line;
        // Limit allocation before parsing; the parent uses exactly one decode
        // block per message. EOF during an active session is never success.
        char c;
        while (std::cin.get(c)) {
            if (c != '\n') {
                if (line.size() >= 1024 * 1024) throw std::runtime_error("R2T2_PROTOCOL: frame too large");
                line += c; continue;
            }
            Json request(cJSON_ParseWithOpts(line.c_str(), nullptr, true), cJSON_Delete);
            if (!request || !cJSON_IsObject(request.get())) throw std::runtime_error("R2T2_PROTOCOL: invalid JSON");
            worker.handle(request.get()); line.clear();
        }
        if (worker.active || !line.empty()) throw std::runtime_error("R2T2_PROTOCOL: premature EOF");
        return 0;
    } catch (const std::exception & error) {
        // Errors contain codes only on the wire; native diagnostics may name
        // tensor/configuration failures but never include transcript content.
        const std::string detail(error.what());
        const auto colon = detail.find(':');
        const std::string code = detail.rfind("R2T2_", 0) == 0 ? detail.substr(0, colon) : "R2T2_RUNTIME_ERROR";
        worker.tentative.clear();
        worker.emit("error", "", code);
        std::cerr << code << '\n';
        if (std::getenv("BLABBER_R2T2_PROBE")) std::cerr << detail << '\n';
        return 1;
    }
}
