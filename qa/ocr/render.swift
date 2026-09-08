import Foundation
import Quartz
import AppKit

// Rasterise page 1 of a PDF at a real pixel scale, the way a scanner would.
let src = URL(fileURLWithPath: CommandLine.arguments[1])
let dst = URL(fileURLWithPath: CommandLine.arguments[2])
let scale = CGFloat(Double(CommandLine.arguments[3])!) / 72.0

guard let doc = CGPDFDocument(src as CFURL), let page = doc.page(at: 1) else { exit(1) }
let box = page.getBoxRect(.mediaBox)
let w = Int(box.width * scale), h = Int(box.height * scale)
guard let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8,
                          bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
                          bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else { exit(1) }
ctx.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))
ctx.fill(CGRect(x: 0, y: 0, width: w, height: h))
ctx.scaleBy(x: scale, y: scale)
ctx.drawPDFPage(page)
guard let img = ctx.makeImage() else { exit(1) }
let rep = NSBitmapImageRep(cgImage: img)
try! rep.representation(using: .jpeg, properties: [.compressionFactor: 0.92])!.write(to: dst)
print("\(w)x\(h)")
