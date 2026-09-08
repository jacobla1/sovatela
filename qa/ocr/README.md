# The OCR oracle

OCR is the part of this app that unit tests cannot reach. The recogniser
belongs to the operating system, it runs inside a child process that
re-executes the app's own binary, and neither exists until something is built.

`scan-oracle.sh` builds a scanned PDF from `contract.txt` and reads it back
through a built binary, the same way the app does:

    npm run tauri build -- --bundles app
    qa/ocr/scan-oracle.sh src-tauri/target/release/bundle/macos/Sovatela.app/Contents/MacOS/scale

It asserts nothing. A scan is a picture, and whether the words came back is a
judgement — so it prints what the recogniser returned, with positions, next to
what the app produced, and leaves the reading to a person.

`probe.swift` is the second half of that: it asks Vision directly, so a wrong
answer can be attributed to the engine or to this code rather than guessed at.
That distinction settled the 1.8.3 ordering defect — Vision was returning
observations out of reading order, and the app was concatenating them as they
arrived.

Run it at 300 dpi and at 72. The high-resolution page is what a scanner
produces and is read almost perfectly; the low-resolution one is where
ordering and line-splitting behaviour actually shows.
