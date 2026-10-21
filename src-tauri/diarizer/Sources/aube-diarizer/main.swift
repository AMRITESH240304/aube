import FluidAudio
import Foundation

func emit(_ object: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: object) else { return }
    FileHandle.standardOutput.write(data + Data("\n".utf8))
}

func fail(_ message: String) -> Never {
    emit(["error": message])
    exit(1)
}

let usage = "usage: aube-diarizer --fetch <models> | aube-diarizer <models> <16 kHz mono s16le audio>"
let arguments = Array(CommandLine.arguments.dropFirst())
guard arguments.count == 2 else {
    fail(usage)
}

if arguments[0] == "--fetch" {
    do {
        _ = try await OfflineDiarizerModels.load(from: URL(fileURLWithPath: arguments[1]))
        exit(0)
    } catch {
        fail("Couldn't fetch the speaker models: \(error.localizedDescription)")
    }
}

let models = URL(fileURLWithPath: arguments[0])
guard let data = FileManager.default.contents(atPath: arguments[1]) else {
    fail("Couldn't read \(arguments[1]).")
}
let audio: [Float] = data.withUnsafeBytes { raw in
    raw.bindMemory(to: Int16.self).map { Float(Int16(littleEndian: $0)) / Float(Int16.max) }
}

do {
    ModelHub.offlineMode = true
    let diarizer = OfflineDiarizerManager()
    diarizer.initialize(models: try await OfflineDiarizerModels.load(from: models))
    let result = try await diarizer.process(audio: audio)
    let segments = result.segments.map {
        ["speaker": $0.speakerId, "start": Double($0.startTimeSeconds), "end": Double($0.endTimeSeconds)] as [String: Any]
    }
    let voices = (result.speakerDatabase ?? [:]).mapValues { $0.map(Double.init) }
    emit(["segments": segments, "voices": voices])
} catch {
    fail("Couldn't tell the speakers apart: \(error.localizedDescription)")
}
