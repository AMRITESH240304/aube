import CoreGraphics
import Foundation

let sentByAube: Int64 = 0x41_75_62_65
let vKey: CGKeyCode = 9
let deleteKey: CGKeyCode = 51

enum Fix: Equatable {
    case undo, lastWord, lastLine, everything

    init?(_ heard: String) {
        let words = heard.lowercased()
            .components(separatedBy: CharacterSet.letters.inverted)
            .filter { !$0.isEmpty }
        guard let verb = words.first, ["delete", "remove", "erase", "undo", "scratch"].contains(verb) else { return nil }
        switch words.dropFirst().filter({ $0 != "the" }) {
        case ["that"], ["it"]: self = .undo
        case ["last", "word"]: self = .lastWord
        case ["last", "line"]: self = .lastLine
        case ["everything"], ["all"], ["all", "of", "it"]: self = .everything
        default: return nil
        }
    }

    var done: String {
        switch self {
        case .undo: "Removed what you just said"
        case .lastWord: "Removed the last word"
        case .lastLine: "Removed the last line"
        case .everything: "Removed everything you dictated"
        }
    }
}

let spokenEdit = try! Regex(#"(?i)\b(scratch\s+that|start\s+over|new\s+paragraph|new\s+line)\b[.,!?]?"#)

func applySpokenEdits(_ heard: String) -> String {
    var text = ""
    var rest = Substring(heard)
    while let match = rest.firstMatch(of: spokenEdit) {
        append(rest[..<match.range.lowerBound], to: &text)
        let command = String(rest[match.range]).lowercased()
        rest = rest[match.range.upperBound...]
        if command.hasPrefix("scratch") {
            text = withoutLastSentence(text)
        } else if command.hasPrefix("start") {
            text = ""
        } else {
            text = text.trimmingCharacters(in: .whitespaces) + (command.contains("paragraph") ? "\n\n" : "\n")
        }
    }
    append(rest, to: &text)
    return text.trimmingCharacters(in: .whitespaces)
}

func append(_ part: Substring, to text: inout String) {
    var part = String(part.drop(while: \.isWhitespace))
    guard !part.isEmpty else { return }
    let startsSentence = text.isEmpty || text.last!.isNewline || ".!?".contains(text.last!)
    if startsSentence {
        part = part.prefix(1).uppercased() + part.dropFirst()
    }
    if !text.isEmpty, !text.last!.isWhitespace {
        text += " "
    }
    text += part
}

func withoutLastSentence(_ text: String) -> String {
    var kept = Substring(text)
    while let last = kept.last, last.isWhitespace || ",;:".contains(last) { kept.removeLast() }
    if let last = kept.last, ".!?".contains(last) { kept.removeLast() }
    guard let end = kept.lastIndex(where: { $0.isNewline || ".!?".contains($0) }) else { return "" }
    return String(kept[...end])
}

func withoutLastWord(_ text: String) -> String {
    var kept = Substring(text)
    while kept.last?.isWhitespace == true { kept.removeLast() }
    while let last = kept.last, !last.isWhitespace { kept.removeLast() }
    return String(kept)
}

func keepingBreaks(of original: String, around tidied: String) -> String {
    String(original.prefix(while: \.isNewline)) + tidied + String(original.reversed().prefix(while: \.isNewline))
}
