import Foundation
import Vision
import AppKit

let url = URL(fileURLWithPath: CommandLine.arguments[1])
guard let img = NSImage(contentsOf: url),
      let cg = img.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
    print("could not load"); exit(1)
}
let req = VNRecognizeTextRequest()
req.recognitionLevel = .accurate
req.usesLanguageCorrection = true
let handler = VNImageRequestHandler(cgImage: cg, options: [:])
try handler.perform([req])
let obs = req.results ?? []
print("observations: \(obs.count)")
for (i, o) in obs.enumerated() {
    let b = o.boundingBox
    let cands = o.topCandidates(3)
    let best = cands.first.map { "\"\($0.string)\" conf=\(String(format: "%.3f", $0.confidence))" } ?? "<NO CANDIDATE>"
    print(String(format: "%2d  y=%.4f..%.4f  x=%.4f..%.4f  cands=%d  %@",
                 i, b.minY, b.maxY, b.minX, b.maxX, cands.count, best))
}
