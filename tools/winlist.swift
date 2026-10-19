import CoreGraphics
import Foundation
// Usage: winlist <pid>: prints "on-screen-windows=N max=WxH" for that process (no Screen Recording needed for bounds).
let pid = Int32(CommandLine.arguments[1])!
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
var n = 0; var best = (0, 0)
for w in list where (w[kCGWindowOwnerPID as String] as? Int32) == pid && (w[kCGWindowLayer as String] as? Int) == 0 {
    n += 1
    if let b = w[kCGWindowBounds as String] as? [String: Any], let wd = b["Width"] as? Int, let ht = b["Height"] as? Int, wd * ht > best.0 * best.1 { best = (wd, ht) }
}
print("on-screen-windows=\(n) max=\(best.0)x\(best.1)")
