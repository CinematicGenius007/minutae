import Foundation

/// Legacy summary prompt (`legacy/src/lib/prompts/summarize.ts`), with action
/// items expanded so owner and due can be returned when the transcript states them.
public enum SummaryPrompt {
    public static let system = """
        You are a meeting assistant. Given a meeting transcript, extract structured \
        information and return it as JSON.

        Be concise and factual. Only include information that is clearly stated in \
        the transcript.
        """

    public static let mapPreamble = """
        Summarise this part of a meeting transcript. Be concise and factual. \
        Only include information that is clearly stated. Keep names, decisions and tasks.
        """

    public static func user(transcript: String) -> String {
        """
        Here is the meeting transcript:

        <transcript>
        \(transcript)
        </transcript>

        Return a JSON object with exactly this shape:
        {
          "title": "A short, descriptive meeting title (max 60 chars)",
          "summary": ["bullet point 1", "bullet point 2", "bullet point 3"],
          "keyDecisions": ["decision 1", "decision 2"],
          "actionItems": [
            {"text": "Alice to do X by Friday", "owner": "Alice", "due": "Friday"}
          ],
          "participants": ["Alice", "Bob"]
        }

        Rules:
        - summary: 3-5 concise bullet points covering the main topics discussed
        - keyDecisions: things that were explicitly decided (can be empty array)
        - actionItems: concrete tasks assigned to people (can be empty array). \
        text is the checklist line. owner and due are optional and null when unknown
        - participants: first names or handles mentioned in the transcript \
        (can be empty array)
        - Return ONLY the JSON object, no markdown fences, no explanation
        """
    }

    public static let jsonSchema: Data = Data(jsonSchemaText.utf8)

    private static let jsonSchemaText = """
        {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "title": {"type": "string"},
            "summary": {"type": "array", "items": {"type": "string"}},
            "keyDecisions": {"type": "array", "items": {"type": "string"}},
            "actionItems": {
              "type": "array",
              "items": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                  "text": {"type": "string"},
                  "owner": {"type": "string"},
                  "due": {"type": "string"}
                },
                "required": ["text"]
              }
            },
            "participants": {"type": "array", "items": {"type": "string"}}
          },
          "required": ["title", "summary", "keyDecisions", "actionItems", "participants"]
        }
        """
}
