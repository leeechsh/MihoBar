// Renders assets/icon/icon.svg to assets/icon/icon.png (1024×1024), the
// app icon nsh.json names: `swift scripts/render-icon.swift`.
import AppKit

let root = URL(fileURLWithPath: CommandLine.arguments[0]).deletingLastPathComponent().deletingLastPathComponent()
let svg = root.appendingPathComponent("assets/icon/icon.svg")
let png = root.appendingPathComponent("assets/icon/icon.png")
guard let image = NSImage(contentsOf: svg) else { fatalError("cannot read \(svg.path)") }
let side = 1024
guard let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: side, pixelsHigh: side, bitsPerSample: 8,
                                 samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
                                 bytesPerRow: 0, bitsPerPixel: 0) else { fatalError("no bitmap") }
rep.size = NSSize(width: side, height: side)
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
image.draw(in: NSRect(x: 0, y: 0, width: side, height: side))
NSGraphicsContext.restoreGraphicsState()
try! rep.representation(using: .png, properties: [:])!.write(to: png)
print("wrote \(png.path)")
