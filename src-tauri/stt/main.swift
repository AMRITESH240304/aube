// Transcribes one audio stream for Aube with Apple's on-device SpeechAnalyzer.
// The app runs one of these per speaker, so the mic and the call are heard separately.
//
// aube-stt [language]           transcribe, in the Mac's language unless one is given
//   stdin:  mono 32-bit float PCM at 48 kHz, until EOF.
//   stdout: one JSON object per line: {"start", "end", "text", "final"} or {"error"}.
// aube-stt --languages          print [{"id", "name", "installed"}] as JSON
// aube-stt --install language   download a language's model ahead of time

import AVFoundation
import Foundation
import Speech

let inputFormat = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: 48_000, channels: 1, interleaved: false)!

func emit(_ object: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: object) else { return }
    FileHandle.standardOutput.write(data + Data("\n".utf8))
}

func fail(_ message: String) -> Never {
    emit(["error": message])
    exit(1)
}

func seconds(_ time: CMTime) -> Double {
    time.seconds.isFinite ? time.seconds : 0
}

let arguments = Array(CommandLine.arguments.dropFirst())

if arguments.first == "--languages" {
    // One entry per language, in the Mac's region if supported, else the
    // language's home region. "mul" is the Indian model Hindi already uses.
    let mine = Locale.current.region
    var best: [String: Locale] = [:]
    for locale in await SpeechTranscriber.supportedLocales {
        guard let code = locale.language.languageCode?.identifier, code != "mul" else { continue }
        let home = Locale(identifier: Locale.Language(identifier: code).maximalIdentifier).region
        func rank(_ l: Locale) -> Int { l.region == mine ? 2 : l.region == home ? 1 : 0 }
        if best[code].map({ rank(locale) > rank($0) }) ?? true {
            best[code] = locale
        }
    }
    let installed = Set(await SpeechTranscriber.installedLocales.map { $0.identifier(.bcp47) })
    let languages = best
        .map { code, locale in
            let id = locale.identifier(.bcp47)
            return ["id": id, "name": Locale.current.localizedString(forLanguageCode: code) ?? id, "installed": installed.contains(id)]
        }
        .sorted { ($0["name"] as! String) < ($1["name"] as! String) }
    FileHandle.standardOutput.write(try! JSONSerialization.data(withJSONObject: languages))
    exit(0)
}

let requested = arguments.first == "--install" ? arguments.dropFirst().first : arguments.first
guard let locale = await SpeechTranscriber.supportedLocale(equivalentTo: requested.map(Locale.init) ?? .current) else {
    fail("Transcription doesn't support \(requested.map { Locale.current.localizedString(forIdentifier: $0) ?? $0 } ?? "your Mac's language") yet.")
}
let transcriber = SpeechTranscriber(
    locale: locale,
    transcriptionOptions: [],
    reportingOptions: [.volatileResults],
    attributeOptions: [.audioTimeRange]
)
do {
    // Downloads the language model the first time; a no-op once installed.
    if let request = try await AssetInventory.assetInstallationRequest(supporting: [transcriber]) {
        try await request.downloadAndInstall()
    }
} catch {
    fail("Couldn't download the speech model: \(error.localizedDescription)")
}
if arguments.first == "--install" {
    exit(0)
}
guard let analyzerFormat = await SpeechAnalyzer.bestAvailableAudioFormat(compatibleWith: [transcriber]),
      let converter = AVAudioConverter(from: inputFormat, to: analyzerFormat)
else {
    fail("Couldn't set up audio for transcription.")
}
converter.primeMethod = .none

// The analyzer doesn't resample, so each chunk is converted to its format.
func convert(_ buffer: AVAudioPCMBuffer) -> AVAudioPCMBuffer? {
    let ratio = analyzerFormat.sampleRate / inputFormat.sampleRate
    let capacity = AVAudioFrameCount(Double(buffer.frameLength) * ratio) + 1
    guard let output = AVAudioPCMBuffer(pcmFormat: analyzerFormat, frameCapacity: capacity) else { return nil }
    var given = false
    var error: NSError?
    converter.convert(to: output, error: &error) { _, status in
        if given {
            status.pointee = .noDataNow
            return nil
        }
        given = true
        status.pointee = .haveData
        return buffer
    }
    return error == nil ? output : nil
}

let (inputs, feed) = AsyncStream<AnalyzerInput>.makeStream()

// Reads stdin 100 ms at a time on its own thread; EOF ends the stream.
Thread.detachNewThread {
    let chunk: AVAudioFrameCount = 4_800
    while let buffer = AVAudioPCMBuffer(pcmFormat: inputFormat, frameCapacity: chunk) {
        let read = fread(buffer.floatChannelData![0], MemoryLayout<Float>.size, Int(chunk), stdin)
        if read == 0 { break }
        buffer.frameLength = AVAudioFrameCount(read)
        if let converted = convert(buffer) {
            feed.yield(AnalyzerInput(buffer: converted))
        }
    }
    feed.finish()
}

let results = Task {
    for try await result in transcriber.results {
        emit([
            "start": seconds(result.range.start),
            "end": seconds(result.range.end),
            "text": String(result.text.characters),
            "final": result.isFinal,
        ])
    }
}

do {
    let analyzer = SpeechAnalyzer(modules: [transcriber])
    if let end = try await analyzer.analyzeSequence(inputs) {
        try await analyzer.finalizeAndFinish(through: end)
    } else {
        // No audio at all: the results never end on their own, so stop here.
        await analyzer.cancelAndFinishNow()
        exit(0)
    }
    try await results.value
} catch {
    fail("Transcription stopped: \(error.localizedDescription)")
}
