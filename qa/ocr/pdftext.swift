import Foundation
import PDFKit

// Print the text layer of a PDF, or nothing if it has none.
//
// The fixture check this backs used to be `grep` over the raw PDF bytes, which
// only ever matched an *uncompressed* text layer. A compressed one — the
// ordinary case — went unnoticed, so a fixture that was not a scan at all would
// have been read rather than recognised and the oracle would have said nothing.
//
// Exits non-zero if the document cannot be opened, so the caller can fail
// closed rather than read silence as "no text".
let url = URL(fileURLWithPath: CommandLine.arguments[1])
guard let doc = PDFDocument(url: url) else {
    FileHandle.standardError.write("cannot open \(url.path)\n".data(using: .utf8)!)
    exit(2)
}
var out = ""
for i in 0..<doc.pageCount {
    if let page = doc.page(at: i), let s = page.string { out += s }
}
print(out)
