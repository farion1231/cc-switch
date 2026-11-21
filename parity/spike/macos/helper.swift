// Parity suite macOS helper: window bounds, real OS mouse clicks, and the Accessibility trust state.
//
//   helper trusted          prints "true" or "false" (AXIsProcessTrusted)
//   helper bounds <pid>     prints "x y w h" of the largest on-screen window owned by pid, in global points
//   helper click <x> <y>    posts a real left click at a global point
//   helper move <x> <y>     moves the cursor to a global point
//   helper activate <pid>   asks the app with this pid to become frontmost
//   helper frontmost        prints the pid of the frontmost app
import AppKit
import ApplicationServices
import CoreGraphics
import Foundation

func fail(_ message: String) -> Never {
    FileHandle.standardError.write((message + "\n").data(using: .utf8)!)
    exit(2)
}

func bounds(pid: pid_t) -> CGRect? {
    let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
    guard let list = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] else {
        return nil
    }
    var best: CGRect?
    for info in list {
        guard let owner = info[kCGWindowOwnerPID as String] as? pid_t, owner == pid,
              let layer = info[kCGWindowLayer as String] as? Int, layer == 0,
              let dict = info[kCGWindowBounds as String] as? NSDictionary,
              let rect = CGRect(dictionaryRepresentation: dict) else { continue }
        if best == nil || rect.width * rect.height > best!.width * best!.height {
            best = rect
        }
    }
    return best
}

func post(_ type: CGEventType, at point: CGPoint) {
    guard let event = CGEvent(mouseEventSource: CGEventSource(stateID: .hidSystemState),
                              mouseType: type, mouseCursorPosition: point, mouseButton: .left) else {
        fail("cannot create \(type) event")
    }
    event.post(tap: .cghidEventTap)
}

let args = CommandLine.arguments
guard args.count >= 2 else { fail("usage: helper trusted | bounds <pid> | click <x> <y> | move <x> <y> | activate <pid> | frontmost") }

switch args[1] {
case "trusted":
    print(AXIsProcessTrusted() ? "true" : "false")
case "bounds":
    guard args.count == 3, let pid = pid_t(args[2]) else { fail("usage: helper bounds <pid>") }
    guard let rect = bounds(pid: pid) else { fail("no on-screen window for pid \(pid)") }
    print("\(rect.origin.x) \(rect.origin.y) \(rect.width) \(rect.height)")
case "click":
    guard args.count == 4, let x = Double(args[2]), let y = Double(args[3]) else {
        fail("usage: helper click <x> <y>")
    }
    let point = CGPoint(x: x, y: y)
    post(.mouseMoved, at: point)
    usleep(80_000)
    post(.leftMouseDown, at: point)
    usleep(60_000)
    post(.leftMouseUp, at: point)
case "move":
    guard args.count == 4, let x = Double(args[2]), let y = Double(args[3]) else {
        fail("usage: helper move <x> <y>")
    }
    post(.mouseMoved, at: CGPoint(x: x, y: y))
case "activate":
    guard args.count == 3, let pid = pid_t(args[2]),
          let app = NSRunningApplication(processIdentifier: pid) else { fail("usage: helper activate <pid>") }
    print(app.activate(options: [.activateAllWindows]) ? "requested" : "refused")
case "frontmost":
    print(NSWorkspace.shared.frontmostApplication?.processIdentifier ?? -1)
default:
    fail("unknown command \(args[1])")
}
