// The Safari extension's bridge to B-Downloader, Safari's counterpart of
// bdl-host (crates/host). Safari passes the extension's
// `runtime.sendNativeMessage` messages to this helper inside the app; it
// passes them on to the app and returns its answer.
//
// The helper runs in a sandbox, which can't reach the app's socket, so it
// connects to the app's loopback port instead (`SAFARI_PORT` in
// crates/ipc): one line of greeting, then the message as one line of JSON,
// as on the socket. Built by scripts/macos-app.sh.

import AppKit
import Foundation
import SafariServices

final class SafariWebExtensionHandler: NSObject, NSExtensionRequestHandling {
    func beginRequest(with context: NSExtensionContext) {
        let message = (context.inputItems.first as? NSExtensionItem)?.userInfo?[SFExtensionMessageKey]
        // Waiting for the app (or starting it) takes a while: not on Safari's thread.
        DispatchQueue.global(qos: .userInitiated).async {
            let response = NSExtensionItem()
            response.userInfo = [SFExtensionMessageKey: Bridge.handle(message)]
            context.completeRequest(returningItems: [response], completionHandler: nil)
        }
    }
}

enum Bridge {
    static let port: UInt16 = 47_381
    static let greeting = "BDOWNLOADER/1\n"
    static let appID = "com.ahmadanjum.bdownloader"
    /// How long to wait for the app to start listening after starting it.
    static let startupWait: TimeInterval = 20

    struct Failure: Error { let reason: String }

    static func handle(_ message: Any?) -> [String: Any] {
        guard let message = message as? [String: Any],
              JSONSerialization.isValidJSONObject(message),
              let json = try? JSONSerialization.data(withJSONObject: message)
        else { return ["ok": false, "error": "not a message"] }
        let type = message["type"] as? String
        // Don't start the app just because Safari started.
        if type != "add" {
            _ = try? exchange(json, expectsReply: false)
            return ["ok": true]
        }
        // A download the browser is holding waits for the app's answer.
        let expectsReply = !(message["expect"] == nil || message["expect"] is NSNull)
        do {
            return try exchange(json, expectsReply: expectsReply)
        } catch {
            startApp()
            let deadline = Date().addingTimeInterval(startupWait)
            while Date() < deadline {
                Thread.sleep(forTimeInterval: 0.2)
                if let reply = try? exchange(json, expectsReply: expectsReply) {
                    return reply
                }
            }
            return ["ok": false, "error": "B-Downloader didn't start"]
        }
    }

    /// Sends the message to the app, and reads its answer when there is one.
    static func exchange(_ json: Data, expectsReply: Bool) throws -> [String: Any] {
        let fd = socket(AF_INET, SOCK_STREAM, 0)
        guard fd >= 0 else { throw Failure(reason: "no socket") }
        defer { close(fd) }
        // An answer waits for the app to check the link: allow a minute.
        var timeout = timeval(tv_sec: 60, tv_usec: 0)
        setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))
        var address = sockaddr_in()
        address.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        address.sin_family = sa_family_t(AF_INET)
        address.sin_port = port.bigEndian
        address.sin_addr.s_addr = inet_addr("127.0.0.1")
        let connected = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                connect(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
        guard connected == 0 else { throw Failure(reason: "B-Downloader isn't running") }

        var out = Data(greeting.utf8)
        out.append(json)
        out.append(UInt8(ascii: "\n"))
        let written = out.withUnsafeBytes { write(fd, $0.baseAddress, $0.count) }
        guard written == out.count else { throw Failure(reason: "couldn't send") }
        guard expectsReply else { return ["ok": true] }

        var line = Data()
        var byte: UInt8 = 0
        while read(fd, &byte, 1) == 1 {
            if byte == UInt8(ascii: "\n") { break }
            line.append(byte)
        }
        guard let reply = try? JSONSerialization.jsonObject(with: line) as? [String: Any] else {
            throw Failure(reason: "no answer")
        }
        return ["ok": reply["ok"] as? Bool ?? false, "error": reply["error"] ?? NSNull()]
    }

    /// Starts B-Downloader in the background, as bdl-host does.
    static func startApp() {
        guard let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: appID) else { return }
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.activates = false
        let started = DispatchSemaphore(value: 0)
        NSWorkspace.shared.openApplication(at: url, configuration: configuration) { _, _ in started.signal() }
        _ = started.wait(timeout: .now() + 10)
    }
}
