import CoreGraphics
import Foundation

let sentByAube: Int64 = 0x41_75_62_65
let vKey: CGKeyCode = 9
let deleteKey: CGKeyCode = 51

enum Edit: Equatable {
    case undo, lastWord, lastLine, lastSentence, everything, phrase([String])

    init?(_ said: some StringProtocol) {
        let words = said.lowercased()
            .components(separatedBy: CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "'’")).inverted)
            .filter { !$0.isEmpty }
        guard let verb = words.first, ["delete", "remove", "erase", "undo", "scratch"].contains(verb) else { return nil }
        let target = Array(words.dropFirst())
        switch target.filter({ $0 != "the" }) {
        case ["that"], ["it"], ["this"]: self = .undo
        case ["last", "word"]: self = .lastWord
        case ["last", "line"]: self = .lastLine
        case ["last", "sentence"]: self = .lastSentence
        case ["everything"], ["all"], ["all", "of", "it"]: self = .everything
        case let rest where ["delete", "remove", "erase"].contains(verb) && !rest.allSatisfy(["a", "an"].contains):
            self = .phrase(target)
        default: return nil
        }
    }

    var done: String {
        switch self {
        case .undo: "Removed what you just said"
        case .lastWord: "Removed the last word"
        case .lastLine: "Removed the last line"
        case .lastSentence: "Removed the last sentence"
        case .everything: "Removed everything you dictated"
        case .phrase(let words): "Removed “\(words.joined(separator: " "))”"
        }
    }
}

func edited(_ text: String, by edit: Edit) -> String? {
    switch edit {
    case .undo, .lastSentence: withoutLastSentence(text)
    case .lastWord: withoutLastWord(text)
    case .lastLine: withoutLastLine(text)
    case .everything: ""
    case .phrase(let words): removing(words, from: text)
    }
}

let spokenEdit = try! Regex(#"(?i)\b(scratch\s+that|start\s+over|new\s+paragraph|new\s+line)\b[.,!?]?"#)

func applySpokenEdits(_ chunks: [String]) -> String {
    var text = ""
    for unit in chunks.flatMap(sentences) {
        if let edit = Edit(unit), let changed = edited(text, by: edit) {
            text = changed
        } else {
            applyInlineEdits(unit, to: &text)
        }
    }
    return text.trimmingCharacters(in: .whitespaces)
}

func sentences(_ chunk: String) -> [Substring] {
    var units: [Substring] = []
    var start = chunk.startIndex
    for index in chunk.indices where ".!?".contains(chunk[index]) {
        let next = chunk.index(after: index)
        if next == chunk.endIndex || chunk[next].isWhitespace {
            units.append(chunk[start..<next])
            start = next
        }
    }
    units.append(chunk[start...])
    return units.filter { $0.contains(where: \.isLetter) }
}

func applyInlineEdits(_ unit: Substring, to text: inout String) {
    var rest = unit
    while let match = rest.firstMatch(of: spokenEdit) {
        append(rest[..<match.range.lowerBound], to: &text)
        let command = rest[match.range].lowercased()
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
}

func append(_ part: Substring, to text: inout String) {
    var part = String(part.drop { $0.isWhitespace && !$0.isNewline })
    guard part.contains(where: { !$0.isWhitespace }) else { return }
    if text.isEmpty || text.last!.isNewline || ".!?".contains(text.last!) {
        part = part.prefix(1).uppercased() + part.dropFirst()
    }
    if !text.isEmpty, !text.last!.isWhitespace, !part.first!.isWhitespace, !".,;:!?".contains(part.first!) {
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

func withoutLastLine(_ text: String) -> String {
    var kept = Substring(text)
    while kept.last?.isWhitespace == true { kept.removeLast() }
    guard let end = kept.lastIndex(where: \.isNewline) else { return withoutLastSentence(text) }
    return String(kept[..<end])
}

func removing(_ words: [String], from text: String) -> String? {
    let pattern = "(?i)\\b" + words.map(NSRegularExpression.escapedPattern).joined(separator: "\\W+") + "\\b"
    guard let regex = try? Regex(pattern), let range = text.matches(of: regex).last?.range else { return nil }
    var before = String(text[..<range.lowerBound])
    var after = text[range.upperBound...]
    while let last = before.last, last == " " || (",;:".contains(last) && after.first.map { ".!?".contains($0) } == true) {
        before.removeLast()
    }
    if before.isEmpty || before.last!.isNewline || ".!?".contains(before.last!) {
        after = after.drop { ".,;:!?".contains($0) }
    }
    append(after, to: &before)
    return before
}

func keepingBreaks(of original: String, around tidied: String) -> String {
    String(original.prefix(while: \.isNewline)) + tidied + String(original.reversed().prefix(while: \.isNewline))
}
