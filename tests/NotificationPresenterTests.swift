import Foundation
import UserNotifications

private final class RecordingNotificationCenter: UserNotificationCentering {
    weak var delegate: UNUserNotificationCenterDelegate?
    private(set) var requestedOptions: UNAuthorizationOptions = []
    private(set) var requests: [UNNotificationRequest] = []

    func requestAuthorization(options: UNAuthorizationOptions, completionHandler: @escaping (Bool, Error?) -> Void) {
        requestedOptions = options
        completionHandler(true, nil)
    }

    func add(_ request: UNNotificationRequest, withCompletionHandler completionHandler: ((Error?) -> Void)?) {
        requests.append(request)
        completionHandler?(nil)
    }
}

private func expect(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() {
        fputs("FAIL: \(message)\n", stderr)
        exit(1)
    }
}

@main
private struct NotificationPresenterTestRunner {
    static func main() {
        let center = RecordingNotificationCenter()
        let presenter = NotificationPresenter(center: center)
        presenter.prepare()
        expect(center.delegate === presenter, "presenter becomes the notification-center delegate")
        expect(center.requestedOptions.contains(.alert), "notification authorization requests alerts")

        presenter.postCaptureSucceeded()
        expect(center.requests.count == 1, "capture success posts one native notification")
        if let request = center.requests.first {
            expect(request.identifier.hasPrefix("capture-success-"), "capture notification has a scoped identifier")
            expect(request.content.title == "SSH Image Paste", "capture notification uses the product title")
            expect(request.content.subtitle == "Screenshot path copied", "capture notification confirms the screenshot path")
            expect(request.content.body.contains("Paste"), "capture notification tells the user what to do next")
        }

        print("PASS: native capture notification presenter")
    }
}
