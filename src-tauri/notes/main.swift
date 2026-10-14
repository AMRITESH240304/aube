import Foundation
import FoundationModels

struct Topic: Codable {
    let heading: String
    let points: [String]
}

struct BatchRequest: Decodable {
    let transcript: [String]
    let notes: [String]
}

struct CombineRequest: Decodable {
    let title: String
    let topics: [Topic]
    let notes: [String]
    let headings: [String]
}

struct Topics: Codable {
    let topics: [Topic]
}

struct Headings: Codable {
    let headings: [String]
}

struct Overview: Codable {
    let overview: String
}

struct Section: Codable {
    let heading: String
    let topics: [Int]
    let notes: [Int]
}

struct Summary: Codable {
    let overview: String
    let sections: [Section]
}

enum Failure {
    case tooLong, declined, language, notReady, other
}

func emit(_ value: some Encodable) {
    guard let data = try? JSONEncoder().encode(value) else { return }
    FileHandle.standardOutput.write(data + Data("\n".utf8))
}

func fail(_ message: String) -> Never {
    emit(["error": message])
    exit(1)
}

let model = SystemLanguageModel.default
let inputBudget = model.contextSize / 2
let options = GenerationOptions(temperature: 0.3, maximumResponseTokens: model.contextSize / 4)

func estimatedTokens(_ text: String) -> Int {
    text.count / 3
}

func schema(_ name: String, _ properties: [DynamicGenerationSchema.Property]) -> GenerationSchema {
    try! GenerationSchema(root: DynamicGenerationSchema(name: name, properties: properties), dependencies: [])
}

func strings(_ name: String, _ description: String, count: ClosedRange<Int>) -> DynamicGenerationSchema.Property {
    .init(
        name: name,
        description: description,
        schema: DynamicGenerationSchema(
            arrayOf: DynamicGenerationSchema(type: String.self),
            minimumElements: count.lowerBound,
            maximumElements: count.upperBound
        )
    )
}

let topicsSchema = schema("Topics", [
    .init(
        name: "topics",
        schema: DynamicGenerationSchema(
            arrayOf: DynamicGenerationSchema(name: "Topic", properties: [
                .init(name: "heading", description: "A short heading of two to five words", schema: DynamicGenerationSchema(type: String.self)),
                strings("points", "Key points on this topic in the order they came up, one short sentence each", count: 1...8),
            ]),
            minimumElements: 1,
            maximumElements: 3
        )
    ),
])

func headingsSchema(topics: Int) -> GenerationSchema {
    schema("Headings", [
        strings("headings", "For each numbered topic, in order, its heading in the final summary", count: topics...topics),
    ])
}

let overviewSchema = schema("Overview", [
    .init(
        name: "overview",
        description: "Two or three sentences on what the meeting covered and decided",
        schema: DynamicGenerationSchema(type: String.self)
    ),
])

func placementSchema(headings: [String], notes: Int) -> GenerationSchema {
    schema("Placement", [
        .init(
            name: "headings",
            description: "For each numbered note, in order, the heading it belongs under",
            schema: DynamicGenerationSchema(
                arrayOf: DynamicGenerationSchema(name: "Heading", anyOf: headings),
                minimumElements: notes,
                maximumElements: notes
            )
        ),
    ])
}

let batchInstructions = """
    You write concise meeting notes. You get part of a meeting transcript, and any notes the user typed \
    during it. Split it into one to three topics. For each topic list the key points: decisions, facts, \
    numbers, names, open questions and action items. Each point is one short sentence. Use only what the \
    transcript and notes say. Write in English.
    """

let headingsInstructions = """
    You organise a meeting summary. You get the meeting's topics in order, numbered, each written from a \
    few minutes of the meeting. Give each topic the heading it should appear under in the final summary. \
    Topics about the same subject get exactly the same heading, so they are merged. Headings are two to \
    five words. Write in English.
    """

let overviewInstructions = """
    You write the opening of a meeting summary: two or three sentences on what the meeting covered. \
    Only call something a decision or an action if the topics say it was decided or assigned. \
    Write in English.
    """

let placementInstructions = """
    You file the user's own meeting notes. You get the headings of a meeting summary and the user's notes, \
    numbered. For each note, in order, choose the heading it belongs under.
    """

func respond<Value: Decodable>(_ instructions: String, _ prompt: String, _ schema: GenerationSchema, as type: Value.Type) async throws -> Value {
    let session = LanguageModelSession(instructions: instructions)
    let reply = try await session.respond(to: prompt, schema: schema, options: options)
    return try JSONDecoder().decode(Value.self, from: Data(reply.content.jsonString.utf8))
}

func failure(_ error: Error) -> Failure {
    if #available(macOS 27.0, *), let error = error as? LanguageModelError {
        switch error {
        case .contextSizeExceeded: return .tooLong
        case .guardrailViolation, .refusal: return .declined
        case .unsupportedLanguageOrLocale: return .language
        default: return .other
        }
    }
    switch error {
    case LanguageModelSession.GenerationError.exceededContextWindowSize: return .tooLong
    case LanguageModelSession.GenerationError.guardrailViolation, LanguageModelSession.GenerationError.refusal: return .declined
    case LanguageModelSession.GenerationError.unsupportedLanguageOrLocale: return .language
    case LanguageModelSession.GenerationError.assetsUnavailable: return .notReady
    default: return .other
    }
}

func explain(_ error: Error) -> String {
    switch failure(error) {
    case .declined: "Apple Intelligence declined to summarise part of this meeting."
    case .language: "Apple Intelligence can't summarise this meeting's language yet."
    case .tooLong: "This meeting is too long to summarise on this Mac."
    case .notReady: "Apple Intelligence isn't ready yet. Try again in a few minutes."
    case .other: "Couldn't write the summary: \(error.localizedDescription)"
    }
}

func numbered(_ lines: [String]) -> String {
    lines.enumerated().map { "\($0.offset + 1). \($0.element)" }.joined(separator: "\n")
}

func topics(_ transcript: [String], _ notes: [String]) async throws -> [Topic] {
    var prompt = "Transcript:\n" + transcript.joined(separator: "\n")
    if !notes.isEmpty {
        prompt += "\n\nNotes the user typed:\n" + notes.joined(separator: "\n")
    }
    let half = transcript.count / 2
    let split = {
        let first = try await topics(Array(transcript[..<half]), notes)
        let second = try await topics(Array(transcript[half...]), [])
        return first + second
    }
    if estimatedTokens(prompt) > inputBudget && transcript.count > 1 {
        return try await split()
    }
    do {
        return try await respond(batchInstructions, prompt, topicsSchema, as: Topics.self).topics
    } catch where failure(error) == .tooLong && transcript.count > 1 {
        return try await split()
    }
}

func outline(_ topics: [Topic], pointsEach: Int) -> String {
    numbered(topics.map { topic in
        ([topic.heading] + topic.points.prefix(pointsEach)).joined(separator: " — ")
    })
}

func briefOutline(_ topics: [Topic]) -> String {
    var pointsEach = 3
    var text = outline(topics, pointsEach: pointsEach)
    while estimatedTokens(text) > inputBudget && pointsEach > 0 {
        pointsEach -= 1
        text = outline(topics, pointsEach: pointsEach)
    }
    return text
}

func combine(_ request: CombineRequest) async throws -> Summary {
    let topics = request.topics
    let outline = briefOutline(topics)
    var prompt = "Meeting: \(request.title)\n\nTopics in order:\n" + outline
    if !request.headings.isEmpty {
        prompt += "\n\nThe user organised their notes under these headings; reuse them where they fit: " + request.headings.joined(separator: ", ")
    }
    let named = try await respond(headingsInstructions, prompt, headingsSchema(topics: topics.count), as: Headings.self).headings
    var headings: [String] = []
    var grouped: [String: [Int]] = [:]
    for (index, topic) in topics.enumerated() {
        let heading = index < named.count ? named[index].trimmingCharacters(in: .whitespaces) : topic.heading
        if grouped[heading] == nil {
            headings.append(heading)
        }
        grouped[heading, default: []].append(index)
    }

    let overview = try await respond(overviewInstructions, "Meeting: \(request.title)\n\n" + outline, overviewSchema, as: Overview.self).overview

    var filed: [String: [Int]] = [:]
    if headings.count == 1 {
        filed[headings[0]] = Array(request.notes.indices)
    } else if !request.notes.isEmpty {
        let prompt = "Headings:\n" + headings.map { "- \($0)" }.joined(separator: "\n") + "\n\nThe user's notes:\n" + numbered(request.notes)
        let placement = try await respond(
            placementInstructions, prompt, placementSchema(headings: headings, notes: request.notes.count), as: Headings.self
        )
        for (note, heading) in placement.headings.enumerated() {
            filed[heading, default: []].append(note)
        }
    }
    return Summary(overview: overview, sections: headings.map { heading in
        Section(heading: heading, topics: grouped[heading, default: []], notes: filed[heading, default: []])
    })
}

if case .unavailable(let reason) = model.availability {
    switch reason {
    case .appleIntelligenceNotEnabled:
        fail("Turn on Apple Intelligence in System Settings to get summaries.")
    case .deviceNotEligible:
        fail("This Mac can't run Apple Intelligence, so summaries aren't available.")
    case .modelNotReady:
        fail("Apple Intelligence is still downloading. Summaries will work once it's ready.")
    @unknown default:
        fail("Apple Intelligence isn't available right now.")
    }
}

let input = FileHandle.standardInput.readDataToEndOfFile()
do {
    switch CommandLine.arguments.dropFirst().first {
    case "batch":
        let request = try JSONDecoder().decode(BatchRequest.self, from: input)
        emit(Topics(topics: try await topics(request.transcript, request.notes)))
    case "combine":
        emit(try await combine(try JSONDecoder().decode(CombineRequest.self, from: input)))
    default:
        fail("usage: aube-notes batch|combine < request.json")
    }
} catch let error as DecodingError {
    fail("Couldn't read the request: \(error)")
} catch {
    fail(explain(error))
}
