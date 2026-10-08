// Transcribes one audio stream for Ease Rec with Apple's on-device SpeechAnalyzer.
// The app runs one of these per speaker, so the mic and the call are heard separately.
//
// stdin:  mono 32-bit float PCM at 48 kHz, until EOF.
// stdout: one JSON object per line: {"start", "end", "text", "final"} or {"error"}.

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

guard let locale = await SpeechTranscriber.supportedLocale(equivalentTo: .current) else {
    fail("Transcription doesn't support your Mac's language yet.")
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
        await analyzer.cancelAndFinishNow()
    }
    try await results.value
} catch {
    fail("Transcription stopped: \(error.localizedDescription)")
}
