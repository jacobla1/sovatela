-- Opens a .docx in Word and reports what Word makes of it.
--
-- The point is the things only Word can answer. A generated document can be
-- valid OPC, pass every check this repo runs, and still be wrong in ways that
-- only appear once Word has laid it out: two Markdown tables that merged into
-- one grid are still one `count tables` here, and no amount of XML assertion
-- notices. `read only true` and `add to recent files false` keep the run from
-- touching the user's Word state; the PDF export forces a full layout pass, so
-- a document that renders wrongly fails there rather than looking fine.
--
--   osascript qa/office-oracle/word.applescript /path/to/file.docx

on run argv
    set inputPath to item 1 of argv
    set outputPath to inputPath & ".pdf"
    set pdfNote to "pdf=written"
    -- Office can take a long time on a cold start, and the first run of each
    -- application waits for macOS to ask for Automation permission. The
    -- default AppleEvent timeout is 60 seconds, which is not enough for either.
    with timeout of 600 seconds
        tell application "Microsoft Word"
            set display alerts to none
            -- Nothing left open from an earlier run, for the reason spelled
            -- out in powerpoint.applescript: a stale copy answers for the
            -- current file.
            close every document saving no
            set docRef to open file name inputPath read only true add to recent files false
            set pageCount to compute statistics docRef statistic statistic pages
            set tableCount to count tables of docRef
            set paraCount to count paragraphs of docRef
            -- The export is worth having and not worth failing over: it is the
            -- layout pass, but the counts above are the oracle.
            try
                save as docRef file name outputPath file format format PDF add to recent files false
            on error errText
                set pdfNote to "pdf=FAILED (" & errText & ")"
            end try
            close every document saving no
        end tell
    end timeout
    return "opened; tables=" & tableCount & "; paragraphs=" & paraCount & "; pages=" & pageCount & "; " & pdfNote
end run
