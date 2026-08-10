import Foundation
import UserNotifications

protocol UserNotificationCentering: AnyObject {
    var delegate: UNUserNotificationCenterDelegate? { get set }
    func requestAuthorization(options: UNAuthorizationOptions,
                              completionHandler: @escaping (Bool, Error?) -> Void)
    func add(_ request: UNNotificationRequest,
             withCompletionHandler completionHandler: ((Error?) -> Void)?)
}

extension UNUserNotificationCenter: UserNotificationCentering {}

final class NotificationPresenter: NSObject, UNUserNotificationCenterDelegate {
    private let center: UserNotificationCentering

    init(center: UserNotificationCentering = UNUserNotificationCenter.current()) {
        self.center = center
        super.init()
    }

    func prepare() {
        center.delegate = self
        center.requestAuthorization(options: [.alert]) { _, _ in }
    }

    func postCaptureSucceeded() {
        let content = UNMutableNotificationContent()
        content.title = "SSH Image Paste"
        content.subtitle = "Screenshot path copied"
        content.body = "Paste it into your SSH session."

        let request = UNNotificationRequest(
            identifier: "capture-success-\(UUID().uuidString)",
            content: content,
            trigger: nil
        )
        center.add(request, withCompletionHandler: nil)
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter,
                                willPresent notification: UNNotification,
                                withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
        completionHandler([.banner])
    }
}
