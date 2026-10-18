import Foundation

enum OllamaEngine {
    case apple
    case ollama(URL, String)
}

enum OllamaError: LocalizedError {
    case noModel
    case badStatus(Int, String)
    case badReply

    var errorDescription: String? {
        switch self {
        case .noModel:
            "Choose an Ollama model in Settings."
        case .badStatus(let code, let body):
            "Ollama returned \(code): \(Ollama.message(from: body))"
        case .badReply:
            "Ollama returned an unexpected reply."
        }
    }
}

enum Ollama {
    private struct Reply: Decodable {
        struct Message: Decodable {
            let content: String
        }

        let message: Message
    }

    private struct Tags: Decodable {
        struct Model: Decodable {
            let name: String
            let capabilities: [String]?
        }

        let models: [Model]
    }

    static func models(at base: URL) async throws -> [String] {
        var request = URLRequest(url: base.appendingPathComponent("api/tags"))
        request.timeoutInterval = 10
        let (data, response) = try await URLSession.shared.data(for: request)
        try check(response, data)
        return try JSONDecoder()
            .decode(Tags.self, from: data)
            .models
            .filter { $0.capabilities?.contains("completion") ?? true }
            .map(\.name)
    }

    static func chat(
        base: URL,
        model: String,
        instructions: String?,
        prompt: String,
        shape: String,
        temperature: Double = 0.3
    ) async throws -> String {
        guard !model.isEmpty else { throw OllamaError.noModel }
        var messages: [[String: String]] = []
        if let instructions {
            messages.append(["role": "system", "content": instructions])
        }
        messages.append([
            "role": "user",
            "content": "\(prompt)\n\nReply with only JSON in this shape:\n\(shape)",
        ])
        let body: [String: Any] = [
            "model": model,
            "stream": false,
            "format": "json",
            "messages": messages,
            "options": ["temperature": temperature],
        ]
        var request = URLRequest(url: base.appendingPathComponent("api/chat"))
        request.httpMethod = "POST"
        request.timeoutInterval = 300
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        let (data, response) = try await URLSession.shared.data(for: request)
        try check(response, data)
        let reply = try JSONDecoder().decode(Reply.self, from: data)
        return json(reply.message.content)
    }

    // Local models often wrap JSON in a Markdown fence despite format: json.
    static func json(_ text: String) -> String {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let start = trimmed.firstIndex(of: "{"),
              let end = trimmed.lastIndex(of: "}"),
              start <= end
        else { return trimmed }
        return String(trimmed[start...end])
    }

    private static func check(_ response: URLResponse, _ data: Data) throws {
        guard let http = response as? HTTPURLResponse else { throw OllamaError.badReply }
        guard (200..<300).contains(http.statusCode) else {
            let body = String(data: data, encoding: .utf8) ?? ""
            throw OllamaError.badStatus(http.statusCode, String(body.prefix(500)))
        }
    }

    static func message(from body: String) -> String {
        guard let data = body.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let message = object["error"] as? String
        else { return body }
        return message
    }
}
