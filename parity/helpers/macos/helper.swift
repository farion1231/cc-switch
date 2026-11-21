// Parity suite macOS helper: window bounds, real OS mouse and keyboard input, and the
// Accessibility trust state. Every event is posted at the HID tap, as a user's would be.
//
//   helper trusted          prints "true" or "false" (AXIsProcessTrusted)
//   helper bounds <pid>     prints "x y w h" of the largest on-screen window owned by pid, in global points
//   helper click <x> <y>    posts a real left click at a global point
//   helper move <x> <y>     moves the cursor to a global point
//   helper activate <pid>   asks the app with this pid to become frontmost
//   helper frontmost        prints the pid of the frontmost app
//   helper type <text>      types the text as key events carrying Unicode strings
//   helper key <combo>      presses a chord such as cmd+a, backspace, shift+tab
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

let keyCodes: [String: CGKeyCode] = [
    "a": 0, "s": 1, "d": 2, "f": 3, "h": 4, "g": 5, "z": 6, "x": 7, "c": 8, "v": 9, "b": 11, "q": 12,
    "w": 13, "e": 14, "r": 15, "y": 16, "t": 17, "1": 18, "2": 19, "3": 20, "4": 21, "6": 22, "5": 23,
    "=": 24, "9": 25, "7": 26, "-": 27, "8": 28, "0": 29, "]": 30, "o": 31, "u": 32, "[": 33, "i": 34,
    "p": 35, "l": 37, "j": 38, "'": 39, "k": 40, ";": 41, "\\": 42, ",": 43, "/": 44, "n": 45, "m": 46,
    ".": 47, "return": 36, "enter": 36, "tab": 48, "space": 49, "backspace": 51, "escape": 53,
    "delete": 117, "left": 123, "right": 124, "down": 125, "up": 126,
]
let modifierFlags: [String: CGEventFlags] = [
    "cmd": .maskCommand, "shift": .maskShift, "alt": .maskAlternate, "ctrl": .maskControl,
]

func postKey(_ code: CGKeyCode, down: Bool, flags: CGEventFlags, text: [UniChar]? = nil) {
    guard let event = CGEvent(keyboardEventSource: CGEventSource(stateID: .hidSystemState), virtualKey: code, keyDown: down) else {
        fail("cannot create key event")
    }
    event.flags = flags
    if var chars = text { event.keyboardSetUnicodeString(stringLength: chars.count, unicodeString: &chars) }
    event.post(tap: .cghidEventTap)
}

let args = CommandLine.arguments
guard args.count >= 2 else { fail("usage: helper trusted | bounds <pid> | click <x> <y> | move <x> <y> | activate <pid> | frontmost | type <text> | key <combo>") }

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
case "type":
    guard args.count == 3 else { fail("usage: helper type <text>") }
    for ch in args[2] {
        let units = Array(String(ch).utf16)
        postKey(0, down: true, flags: [], text: units)
        postKey(0, down: false, flags: [], text: units)
        usleep(12_000)
    }
case "key":
    guard args.count == 3 else { fail("usage: helper key <combo>") }
    var flags: CGEventFlags = []
    var code: CGKeyCode?
    for part in args[2].lowercased().split(separator: "+").map(String.init) {
        if let f = modifierFlags[part] { flags.insert(f) } else if let c = keyCodes[part] { code = c } else { fail("unknown key \(part)") }
    }
    guard let key = code else { fail("no key in \(args[2])") }
    postKey(key, down: true, flags: flags)
    usleep(20_000)
    postKey(key, down: false, flags: flags)
default:
    fail("unknown command \(args[1])")
}
