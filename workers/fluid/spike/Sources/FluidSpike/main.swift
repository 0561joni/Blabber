// Phase 0 spike: measures load, memory, streaming and final-pass latency of the
// live pair (Nemotron streaming + Parakeet v3) from local model directories.
// Throwaway code; the numbers end up in benchmark-results/fluid-spike-m3-pro.json.
import CoreML
import Darwin
import FluidAudio
import Foundation

func now() -> Double { Double(DispatchTime.now().uptimeNanoseconds) / 1e6 }

func memory() -> [String: UInt64] {
    var info = task_vm_info_data_t()
    var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<integer_t>.size)
    let result = withUnsafeMutablePointer(to: &info) {
        $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
            task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
        }
    }
    var usage = rusage()
    getrusage(RUSAGE_SELF, &usage)
    guard result == KERN_SUCCESS else { return ["peakRssBytes": UInt64(usage.ru_maxrss)] }
    // ANE programs and weights are wired kernel memory outside the task's
    // footprint; the system wired delta is the closest observable proxy.
    var host = vm_statistics64_data_t()
    var hostCount = mach_msg_type_number_t(MemoryLayout<vm_statistics64_data_t>.size / MemoryLayout<integer_t>.size)
    _ = withUnsafeMutablePointer(to: &host) {
        $0.withMemoryRebound(to: integer_t.self, capacity: Int(hostCount)) {
            host_statistics64(mach_host_self(), HOST_VM_INFO64, $0, &hostCount)
        }
    }
    return [
        "systemWiredBytes": UInt64(host.wire_count) * UInt64(getpagesize()),
        "physFootprintBytes": info.phys_footprint,
        "residentBytes": info.resident_size,
        "peakRssBytes": UInt64(usage.ru_maxrss),
    ]
}

func readWav(_ path: String) throws -> [Float] {
    let data = try Data(contentsOf: URL(fileURLWithPath: path))
    // Fixtures are canonical 16 kHz mono PCM16; locate the "data" chunk.
    var offset = 12
    while offset + 8 <= data.count {
        let id = String(decoding: data[offset..<offset + 4], as: UTF8.self)
        let size = Int(data[offset + 4..<offset + 8].withUnsafeBytes { $0.loadUnaligned(as: UInt32.self) })
        if id == "data" {
            let body = data[offset + 8..<min(data.count, offset + 8 + size)]
            return body.withUnsafeBytes { raw in
                raw.bindMemory(to: Int16.self).map { Float($0) / 32768 }
            }
        }
        offset += 8 + size + (size & 1)
    }
    throw NSError(domain: "spike", code: 1, userInfo: [NSLocalizedDescriptionKey: "no data chunk in \(path)"])
}

func words(_ text: String) -> [String] {
    text.lowercased()
        .components(separatedBy: CharacterSet.alphanumerics.inverted)
        .filter { !$0.isEmpty }
}

func wer(_ reference: String, _ hypothesis: String) -> (errors: Int, words: Int) {
    let r = words(reference), h = words(hypothesis)
    var row = Array(0...h.count)
    for i in 1...max(r.count, 1) where !r.isEmpty {
        var previous = row[0]
        row[0] = i
        for j in stride(from: 1, through: h.count, by: 1) {
            let current = row[j]
            row[j] = min(row[j] + 1, row[j - 1] + 1, previous + (r[i - 1] == h[j - 1] ? 0 : 1))
            previous = current
        }
    }
    return (r.isEmpty ? h.count : row[h.count], r.count)
}

func percentile(_ values: [Double], _ p: Double) -> Double {
    guard !values.isEmpty else { return 0 }
    let sorted = values.sorted()
    let rank = Int((p / 100 * Double(sorted.count - 1)).rounded(.up))
    return sorted[min(rank, sorted.count - 1)]
}

func round1(_ value: Double) -> Double { (value * 10).rounded() / 10 }

struct Options: Sendable {
    var nemotronRoot = ""
    var parakeet = ""
    var fixtures = ""
    var output = ""
    var tiers = [560, 1120, 2240]
    var encoder = "int8"
    var runs = 10
    var section = "all"
}

func parseOptions() -> Options {
var options = Options()
var arguments = CommandLine.arguments.dropFirst().makeIterator()
while let flag = arguments.next() {
    let value = arguments.next() ?? ""
    switch flag {
    case "--nemotron-root": options.nemotronRoot = value
    case "--parakeet": options.parakeet = value
    case "--fixtures": options.fixtures = value
    case "--output": options.output = value
    case "--tiers": options.tiers = value.split(separator: ",").compactMap { Int($0) }
    case "--encoder": options.encoder = value
    case "--runs": options.runs = Int(value) ?? 10
    case "--section": options.section = value
    default:
        FileHandle.standardError.write("unknown flag \(flag)\n".data(using: .utf8)!)
        exit(2)
    }
}
return options
}
let options = parseOptions()

func fixture(_ name: String) throws -> [Float] { try readWav("\(options.fixtures)/\(name).wav") }
func reference(_ name: String) -> String {
    (try? String(contentsOfFile: "\(options.fixtures)/\(name).txt", encoding: .utf8))?
        .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
}

func log(_ message: String) {
    FileHandle.standardError.write("[spike] \(message)\n".data(using: .utf8)!)
}

func loadParakeet() async throws -> (AsrManager, Double) {
    let began = now()
    let precision = ParakeetEncoderPrecision(rawValue: options.encoder) ?? .int8
    let models = try AsrModels.loadLocal(
        from: URL(fileURLWithPath: options.parakeet), version: .v3, encoderPrecision: precision)
    let manager = AsrManager(config: .default)
    try await manager.loadModels(models)
    return (manager, now() - began)
}

func loadNemotron(_ tier: Int) async throws -> (StreamingNemotronMultilingualAsrManager, Double) {
    let began = now()
    let manager = StreamingNemotronMultilingualAsrManager()
    try await manager.loadModels(from: URL(fileURLWithPath: "\(options.nemotronRoot)/\(tier)ms"))
    return (manager, now() - began)
}

func parakeetPass(_ manager: AsrManager, _ samples: [Float], language: Language?) async throws -> (String, Double) {
    var state = TdtDecoderState.make()
    let began = now()
    let result = try await manager.transcribe(samples, decoderState: &state, language: language)
    return (result.text, now() - began)
}

/// Feeds one chunk per call, as the worker will, and replays a real-time
/// schedule: chunk i becomes available at (i+1)*chunkMs of wall clock.
func stream(
    _ manager: StreamingNemotronMultilingualAsrManager, _ samples: [Float], chunk: Int, language: String
) async throws -> [String: Any] {
    await manager.setLanguage(language)
    await manager.reset()
    var timings: [Double] = []
    var done = 0.0
    var firstText: Double?
    let chunkMs = Double(chunk) / 16
    var index = 0
    while index < samples.count {
        let end = min(samples.count, index + chunk)
        let part = Array(samples[index..<end])
        if part.count < chunk { break }
        let began = now()
        _ = try await manager.process(samples: part)
        let elapsed = now() - began
        timings.append(elapsed)
        let availableAt = Double(end) / 16
        done = max(availableAt, done) + elapsed
        if firstText == nil, !(await manager.getPartialTranscript()).isEmpty { firstText = done }
        index = end
    }
    let tailBegan = now()
    var tail = Array(samples[index...])
    if tail.isEmpty { tail = [] }
    if !tail.isEmpty { _ = try await manager.process(samples: tail) }
    let text = try await manager.finish()
    let finishMs = now() - tailBegan
    let audioMs = Double(samples.count) / 16
    return [
        "text": text,
        "language": await manager.detectedLanguage() ?? "",
        "chunks": timings.count,
        "chunkMsMedian": round1(percentile(timings, 50)),
        "chunkMsP95": round1(percentile(timings, 95)),
        "rtf": round1(timings.reduce(0, +) / max(1, Double(timings.count) * chunkMs) * 1000) / 1000,
        "firstTextMs": firstText.map(round1) as Any,
        "flushMs": round1(finishMs),
        "audioMs": round1(audioMs),
        "_chunkMs": chunkMs,
    ]
}

@MainActor
func run() async throws {
    var report: [String: Any] = [
        "machine": ProcessInfo.processInfo.hostName.isEmpty ? "unknown" : "Apple M3 Pro",
        "osVersion": ProcessInfo.processInfo.operatingSystemVersionString,
        "physicalMemoryBytes": ProcessInfo.processInfo.physicalMemory,
        "fluidAudioRevision": "0b1f46289fe27d95b5e66ad8be46e64f5ee02ae7",
        "parakeetEncoder": options.encoder,
        "baselineMemory": memory(),
    ]
    let clipNames = (1...20).flatMap { id in ["de", "en"].map { "\($0)-\(String(format: "%02d", id))" } }

    if options.section == "all" || options.section == "load" {
        // Each tier is loaded once cold (fresh process sees whatever cache exists).
        var loads: [String: Any] = [:]
        let (parakeet, parakeetMs) = try await loadParakeet()
        loads["parakeetLoadMs"] = round1(parakeetMs)
        loads["memoryAfterParakeet"] = memory()
        let warm = try await parakeetPass(parakeet, Array(repeating: 0, count: 16_000), language: nil)
        loads["parakeetWarmupMs"] = round1(warm.1)
        let tier = options.tiers.contains(1120) ? 1120 : options.tiers[0]
        let (nemotron, nemotronMs) = try await loadNemotron(tier)
        loads["nemotronLoadMs"] = round1(nemotronMs)
        loads["nemotronTier"] = tier
        loads["memoryBothLoaded"] = memory()
        _ = try await stream(nemotron, Array(repeating: 0, count: 16_000 * 2), chunk: tier * 16, language: "auto")
        _ = try await parakeetPass(parakeet, try fixture("de-01"), language: nil)
        loads["memoryBothAfterUse"] = memory()
        // Unload and reload, as the critical-memory policy will.
        await nemotron.cleanup()
        await parakeet.cleanup()
        loads["memoryAfterUnload"] = memory()
        let (reParakeet, reParakeetMs) = try await loadParakeet()
        let (reNemotron, reNemotronMs) = try await loadNemotron(tier)
        loads["reloadMs"] = ["parakeet": round1(reParakeetMs), "nemotron": round1(reNemotronMs)]
        let check = try await parakeetPass(reParakeet, try fixture("en-01"), language: nil)
        loads["reloadUsable"] = !check.0.isEmpty
        loads["memoryAfterReload"] = memory()
        _ = reNemotron
        report["load"] = loads
    }

    if options.section == "all" || options.section == "stream" {
        var tiers: [String: Any] = [:]
        for tier in options.tiers {
            let (manager, loadMs) = try await loadNemotron(tier)
            var perLanguage: [String: Any] = [:]
            for (mode, language) in [("auto", "auto"), ("fixed", "")] {
                var errors = [String: (Int, Int)]()
                var first: [Double] = [], rtf: [Double] = [], chunkP95: [Double] = [], flush: [Double] = []
                var detected: [String: Int] = [:]
                for name in clipNames {
                    let lang = String(name.prefix(2))
                    let hint = mode == "auto" ? language : (lang == "de" ? "de-DE" : "en-US")
                    let result = try await stream(manager, try fixture(name), chunk: tier * 16, language: hint)
                    let score = wer(reference(name), result["text"] as? String ?? "")
                    let total = errors[lang] ?? (0, 0)
                    errors[lang] = (total.0 + score.errors, total.1 + score.words)
                    if let value = result["firstTextMs"] as? Double { first.append(value) }
                    rtf.append(result["rtf"] as? Double ?? 0)
                    chunkP95.append(result["chunkMsP95"] as? Double ?? 0)
                    flush.append(result["flushMs"] as? Double ?? 0)
                    detected["\(lang)->\(result["language"] as? String ?? "")", default: 0] += 1
                }
                perLanguage[mode] = [
                    "werDe": round1(Double(errors["de"]!.0) / Double(errors["de"]!.1) * 1000) / 10,
                    "werEn": round1(Double(errors["en"]!.0) / Double(errors["en"]!.1) * 1000) / 10,
                    "firstTextMsMedian": round1(percentile(first, 50)),
                    "firstTextMsP95": round1(percentile(first, 95)),
                    "rtfMedian": percentile(rtf, 50),
                    "chunkMsP95": round1(percentile(chunkP95, 95)),
                    "flushMsP95": round1(percentile(flush, 95)),
                    "detectedLanguages": detected,
                ]
                log("tier \(tier) \(mode): \(perLanguage[mode]!)")
            }
            // Long-run throughput: five minutes, real time must be sustained.
            let long = try await stream(manager, try fixture("de-five-minutes"), chunk: tier * 16, language: "auto")
            perLanguage["fiveMinutesGerman"] = long.filter { $0.key != "text" && $0.key != "_chunkMs" }
            perLanguage["loadMs"] = round1(loadMs)
            perLanguage["memory"] = memory()
            tiers["\(tier)ms"] = perLanguage
            await manager.cleanup()
        }
        report["nemotron"] = tiers
    }

    if options.section == "all" || options.section == "final" {
        let (parakeet, _) = try await loadParakeet()
        _ = try await parakeetPass(parakeet, Array(repeating: 0, count: 16_000), language: nil)
        var final: [String: Any] = [:]
        let source = try fixture("de-five-minutes") + (try fixture("en-five-minutes"))
        for seconds in [10, 30, 120, 300] {
            var latencies: [Double] = []
            let runs = seconds >= 120 ? max(3, options.runs / 3) : options.runs
            for run in 0..<runs {
                // Different windows per run so the cache never sees identical input.
                let start = (run * 7 * 16_000) % max(1, source.count - seconds * 16_000)
                let clip = Array(source[start..<start + seconds * 16_000])
                latencies.append(try await parakeetPass(parakeet, clip, language: nil).1)
            }
            final["\(seconds)s"] = [
                "runs": runs, "medianMs": round1(percentile(latencies, 50)),
                "p95Ms": round1(percentile(latencies, 95)),
            ]
            log("parakeet \(seconds)s: \(final["\(seconds)s"]!)")
        }
        var errors = [String: (Int, Int)]()
        for name in clipNames {
            let lang = String(name.prefix(2))
            let (text, _) = try await parakeetPass(parakeet, try fixture(name), language: nil)
            let score = wer(reference(name), text)
            let total = errors[lang] ?? (0, 0)
            errors[lang] = (total.0 + score.errors, total.1 + score.words)
        }
        final["werDe"] = round1(Double(errors["de"]!.0) / Double(errors["de"]!.1) * 1000) / 10
        final["werEn"] = round1(Double(errors["en"]!.0) / Double(errors["en"]!.1) * 1000) / 10
        final["memory"] = memory()
        report["parakeet"] = final
    }

    if options.section == "all" || options.section == "vocab" {
        // Decode-time biasing exists for Nemotron only; Parakeet v3 biasing in
        // FluidAudio needs an extra CTC keyword-spotter model.
        let tier = options.tiers.contains(1120) ? 1120 : options.tiers[0]
        let (manager, _) = try await loadNemotron(tier)
        let samples = try fixture("de-vocab")
        let plain = try await stream(manager, samples, chunk: tier * 16, language: "de-DE")["text"] as? String ?? ""
        await manager.setCustomVocabulary(["Zwiebelmeier", "Quaschnik", "Wuppertal"].map { CustomVocabularyTerm(text: $0) })
        let biased = try await stream(manager, samples, chunk: tier * 16, language: "de-DE")["text"] as? String ?? ""
        report["vocabulary"] = ["germanPlain": plain, "germanBiased": biased, "reference": reference("de-vocab"),
            "parakeet": try await parakeetPass(try await loadParakeet().0, samples, language: nil).0]
    }

    let data = try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
    if options.output.isEmpty {
        FileHandle.standardOutput.write(data)
    } else {
        try data.write(to: URL(fileURLWithPath: options.output))
    }
}

let done = DispatchSemaphore(value: 0)
Task { @MainActor in
    do { try await run() } catch {
        log("failed: \(error)")
        exit(1)
    }
    done.signal()
}
while done.wait(timeout: .now()) == .timedOut {
    RunLoop.main.run(until: Date().addingTimeInterval(0.05))
}
