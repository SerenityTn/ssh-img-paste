import AppKit

@main
struct ProfileManagerScreenshot {
    static func main() {
        guard CommandLine.arguments.count == 3 else {
            fputs("usage: ProfileManagerScreenshot MOCK_CLI OUTPUT.png\n", stderr)
            exit(64)
        }

        let outputURL = URL(fileURLWithPath: CommandLine.arguments[2])
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
        let controller = ProfileManagerWindowController(
            client: ScriptClient(executable: CommandLine.arguments[1]),
            onMutation: {}
        )
        controller.show()

        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) {
            guard let frameView = controller.window?.contentView?.superview,
                  let bitmap = frameView.bitmapImageRepForCachingDisplay(in: frameView.bounds) else {
                fputs("Could not render the profile-manager window.\n", stderr)
                exit(1)
            }
            frameView.cacheDisplay(in: frameView.bounds, to: bitmap)
            guard let png = bitmap.representation(using: .png, properties: [:]) else {
                fputs("Could not encode the profile-manager screenshot.\n", stderr)
                exit(1)
            }
            do {
                try FileManager.default.createDirectory(
                    at: outputURL.deletingLastPathComponent(),
                    withIntermediateDirectories: true
                )
                try png.write(to: outputURL, options: .atomic)
                app.terminate(nil)
            } catch {
                fputs("\(error)\n", stderr)
                exit(1)
            }
        }
        app.run()
    }
}
