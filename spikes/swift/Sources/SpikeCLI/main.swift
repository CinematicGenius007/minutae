import Foundation

/// Headless entry for SPIKE-01 probes that do not need a full NSApplication run loop.
/// Usage: spike-cli <subcommand> […]
@main
enum SpikeCLIMain {
    static func main() async {
        let args = Array(CommandLine.arguments.dropFirst())
        guard let command = args.first else {
            printUsage()
            exit(2)
        }

        do {
            switch command {
            case "llm":
                try await LLMBridgeProbe.run(args: Array(args.dropFirst()))
            case "speech":
                try await SpeechProbe.run(args: Array(args.dropFirst()))
            case "insertion":
                try InsertionProbe.run(args: Array(args.dropFirst()))
            case "secure-check":
                InsertionProbe.printSecureInputState()
            case "process-tap":
                try ProcessTapProbe.run(args: Array(args.dropFirst()))
            case "package-info":
                PackagingProbe.printNotes()
            case "help", "-h", "--help":
                printUsage()
            default:
                fputs("unknown command: \(command)\n", stderr)
                printUsage()
                exit(2)
            }
        } catch {
            fputs("error: \(error)\n", stderr)
            exit(1)
        }
    }

    static func printUsage() {
        print(
            """
            spike-cli — Minutae SPIKE-01 throwaway probes

            Commands:
              llm codex|grok [--timeout <sec>]
              speech record|whisper-cpp|whisperkit [--seconds 10]
              insertion clipboard|ax [--text <string>]
              secure-check
              process-tap [--pid <pid>] [--list]
              package-info
            """
        )
    }
}
