// Both models stay loaded across sessions. A session only resets streaming
// state, so a shortcut press never pays a model load.
import CoreML
import FluidAudio
import Foundation

func milliseconds(since start: UInt64) -> Double {
    (Double(DispatchTime.now().uptimeNanoseconds - start) / 1e6 * 10).rounded() / 10
}

/// Splits a growing Nemotron transcript into a prefix that only ever grows
/// (committed) and the last, possibly unfinished word (tentative).
struct PreviewLedger {
    private(set) var committed = ""

    mutating func update(with partial: String) -> (committed: String, tentative: String) {
        let stable: Substring
        if let space = partial.lastIndex(where: { $0 == " " }) {
            stable = partial[..<space]
        } else {
            stable = ""
        }
        if stable.hasPrefix(committed) {
            committed = String(stable)
        }
        let tentative = partial.hasPrefix(committed) ? String(partial.dropFirst(committed.count)) : ""
        return (committed, tentative)
    }
}

struct Session {
    let id: String
    /// Next accepted request sequence (`start` is zero).
    var nextSequence = 1
    let language: String
    /// Every acknowledged sample, retained for the final pass.
    var samples: [Float] = []
    /// Samples received but not yet fed to Nemotron (less than one chunk).
    var pendingStart = 0
    var ledger = PreviewLedger()
    var streamError = ""
}

@MainActor
final class Engine {
    private var nemotron: StreamingNemotronMultilingualAsrManager?
    private var parakeet: AsrManager?
    private var chunkSamples = 0
    private var promptLanguages: Set<String> = []
    private var session: Session?

    var loaded: Bool { nemotron != nil && parakeet != nil }

    // MARK: Control requests

    func load(nemotronPath: String, parakeetPath: String) async throws -> [String: Any] {
        session = nil
        await unload()
        let began = DispatchTime.now().uptimeNanoseconds
        let nemotronURL = URL(fileURLWithPath: nemotronPath, isDirectory: true)
        let parakeetURL = URL(fileURLWithPath: parakeetPath, isDirectory: true)
        let required: [(URL, String)] =
            [
                "metadata.json", "tokenizer.json", "encoder.mlmodelc", "decoder_joint.mlmodelc",
            ].map { (nemotronURL, $0) }
            + [
                "Preprocessor.mlmodelc", "Encoder_v2.mlmodelc", "Decoder.mlmodelc",
                "JointDecisionv3.mlmodelc", "parakeet_vocab.json",
            ].map { (parakeetURL, $0) }
        for (directory, name) in required
        where !FileManager.default.fileExists(atPath: directory.appendingPathComponent(name).path) {
            throw WorkerError.session("FLUID_MODEL_MISSING", "missing \(name)")
        }
        do {
            let models = try AsrModels.loadLocal(
                from: parakeetURL, version: .v3, encoderPrecision: .int8V2)
            let parakeet = AsrManager(config: .default)
            try await parakeet.loadModels(models)
            let nemotron = StreamingNemotronMultilingualAsrManager()
            try await nemotron.loadModels(from: nemotronURL)
            let config = await nemotron.config
            self.chunkSamples = config.chunkSamples
            self.promptLanguages = Set(config.promptDictionary.keys)
            self.parakeet = parakeet
            self.nemotron = nemotron
        } catch let error as WorkerError {
            throw error
        } catch {
            await unload()
            throw WorkerError.session("FLUID_LOAD_FAILED", "\(error)")
        }
        return ["loadMs": milliseconds(since: began), "chunkMs": chunkSamples * 1000 / sampleRate]
    }

    /// One second of silence through both models, so the first dictation pays
    /// no first-prediction cost.
    func warmup() async throws -> [String: Any] {
        session = nil
        guard let nemotron, let parakeet else {
            throw WorkerError.session("FLUID_NOT_LOADED", "warmup before load")
        }
        let began = DispatchTime.now().uptimeNanoseconds
        do {
            let silence = [Float](repeating: 0, count: max(sampleRate, chunkSamples))
            await nemotron.setLanguage("auto")
            await nemotron.reset()
            _ = try await nemotron.process(samples: silence)
            _ = try await nemotron.finish()
            await nemotron.reset()
            var state = TdtDecoderState.make()
            _ = try await parakeet.transcribe(silence, decoderState: &state, language: nil)
        } catch {
            throw WorkerError.session("FLUID_LOAD_FAILED", "warmup: \(error)")
        }
        return ["warmupMs": milliseconds(since: began)]
    }

    func ping() -> [String: Any] {
        session = nil
        return ["loaded": loaded]
    }

    func unload() async {
        session = nil
        if let nemotron { await nemotron.cleanup() }
        if let parakeet { await parakeet.cleanup() }
        nemotron = nil
        parakeet = nil
        chunkSamples = 0
    }

    // MARK: Session requests

    func start(_ request: Request) async throws -> [String: Any] {
        session = nil
        guard request.sequence == 0 else {
            throw WorkerError.session("FLUID_STALE_SESSION", "start sequence must be zero")
        }
        guard let nemotron else { throw WorkerError.session("FLUID_NOT_LOADED", "start before load") }
        let language = try request.string("language", default: "auto")
        guard language == "auto" || promptLanguages.contains(language) else {
            throw WorkerError.session("FLUID_LANGUAGE_UNSUPPORTED", "unsupported language")
        }
        let chunkMs = try request.integer("chunkMs")
        guard chunkMs * sampleRate / 1000 == chunkSamples else {
            throw WorkerError.session("FLUID_CHUNK_MISMATCH", "loaded models use another chunk size")
        }
        await nemotron.setLanguage(language)
        await nemotron.reset()
        session = Session(id: request.sessionId, language: language)
        return ["language": language]
    }

    /// Validates that a request belongs to the active session, in order.
    private func current(_ request: Request) throws -> Session {
        guard var active = session, active.id == request.sessionId else {
            throw WorkerError.session("FLUID_STALE_SESSION", "no such active session")
        }
        guard request.sequence == active.nextSequence else {
            session = nil
            throw WorkerError.session("FLUID_STALE_SESSION", "unordered request")
        }
        active.nextSequence += 1
        return active
    }

    func audio(_ request: Request) async throws -> [String: Any] {
        var active = try current(request)
        // A rejected frame ends the session: the parent's ledger is wrong.
        session = nil
        let start = try request.integer("startSample")
        guard start == active.samples.count else {
            throw WorkerError.session("FLUID_SAMPLES_MISMATCH", "gap or duplicate audio")
        }
        let samples = try request.samples()
        guard active.samples.count + samples.count <= maxSessionSamples else {
            throw WorkerError.session("FLUID_LIMIT", "recording exceeds five minutes")
        }
        active.samples.append(contentsOf: samples)
        var preview = (committed: active.ledger.committed, tentative: "")
        if active.streamError.isEmpty, let nemotron {
            let whole = (active.samples.count - active.pendingStart) / chunkSamples * chunkSamples
            if whole > 0 {
                do {
                    let chunk = Array(active.samples[active.pendingStart..<active.pendingStart + whole])
                    _ = try await nemotron.process(samples: chunk)
                    active.pendingStart += whole
                    preview = active.ledger.update(with: await nemotron.getPartialTranscript())
                } catch {
                    // The preview stops; the final pass still runs on the full buffer.
                    active.streamError = "FLUID_STREAM_FAILED"
                    FileHandle.standardError.write(Data("FLUID_STREAM_FAILED\n".utf8))
                }
            }
        }
        session = active
        return [
            "committedText": preview.committed, "tentativeText": preview.tentative,
            "ackSample": active.samples.count, "streamError": active.streamError,
        ]
    }

    func finish(_ request: Request) async throws -> [String: Any] {
        let active = try current(request)
        session = nil
        guard try request.integer("expectedSamples") == active.samples.count else {
            throw WorkerError.session("FLUID_SAMPLES_MISMATCH", "finish sample count differs")
        }
        guard let nemotron, let parakeet else {
            throw WorkerError.session("FLUID_NOT_LOADED", "models were unloaded")
        }
        let audioMs = active.samples.count * 1000 / sampleRate
        var streamText = active.ledger.committed
        var streamError = active.streamError
        var language = active.language == "auto" ? "" : active.language
        let flushBegan = DispatchTime.now().uptimeNanoseconds
        if streamError.isEmpty {
            do {
                if active.pendingStart < active.samples.count {
                    _ = try await nemotron.process(samples: Array(active.samples[active.pendingStart...]))
                }
                streamText = try await nemotron.finish()
                if language.isEmpty { language = await nemotron.detectedLanguage() ?? "" }
            } catch {
                streamError = "FLUID_STREAM_FAILED"
            }
        }
        await nemotron.reset()
        let flushMs = milliseconds(since: flushBegan)
        let finalBegan = DispatchTime.now().uptimeNanoseconds
        var finalText = ""
        var finalError = ""
        if !active.samples.isEmpty {
            // Parakeet needs at least 0.3 s; pad short presses to one second.
            var audio = active.samples
            if audio.count < sampleRate {
                audio.append(contentsOf: [Float](repeating: 0, count: sampleRate - audio.count))
            }
            let hint = Language(rawValue: String(language.prefix(2)))
            do {
                var state = TdtDecoderState.make()
                finalText = try await parakeet.transcribe(audio, decoderState: &state, language: hint).text
                    .trimmingCharacters(in: .whitespacesAndNewlines)
            } catch {
                finalError = "FLUID_FINAL_FAILED"
                FileHandle.standardError.write(Data("FLUID_FINAL_FAILED\n".utf8))
            }
        }
        return [
            "streamText": streamText, "finalText": finalText, "language": language,
            "streamError": streamError, "finalError": finalError, "ackSample": active.samples.count,
            "timings": [
                "audioMs": audioMs, "flushMs": flushMs, "finalMs": milliseconds(since: finalBegan),
            ],
        ]
    }

    /// Cancels the named session if it is active. Idempotent.
    func cancel(_ request: Request) async {
        if session?.id == request.sessionId {
            session = nil
            await nemotron?.reset()
        }
    }

    var hasActiveSession: Bool { session != nil }
}
