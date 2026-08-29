-- Opens a .pptx in PowerPoint and reports what PowerPoint makes of it.
--
-- A deck that PowerPoint offers to repair is a failed generation even though
-- the file is a valid zip full of valid XML — the repair prompt is the only
-- signal, and it appears nowhere else. The slide count catches pagination
-- going wrong, and the PDF export forces the layout that a placeholder
-- pointing at nothing would otherwise survive.
--
--   osascript qa/office-oracle/powerpoint.applescript /path/to/file.pptx

on run argv
    set inputPath to item 1 of argv
    set outputPath to inputPath & ".pdf"
    set pdfNote to "pdf=written"
    with timeout of 600 seconds
        tell application "Microsoft PowerPoint"
            -- Anything already open is closed first. `open` on a file
            -- PowerPoint already has open returns the copy it is holding, so a
            -- deck left over from an earlier run answers for the current one —
            -- which is how this reported nine slides for a ten-slide file, and
            -- would have hidden a regression rather than finding one.
            try
                close every presentation saving no
            end try
            -- `open` hands back something that is not a usable presentation
            -- reference — `count slides of` it fails with -9074. The opened
            -- deck is the active one, so ask for that instead.
            open inputPath
            set p to active presentation
            set slideCount to count slides of p
            try
                -- `POSIX file`, not the bare path: given a plain string
                -- PowerPoint raises no error, reports nothing, and writes no
                -- file — so the export silently did not happen and the `try`
                -- called it a success. The shell script checks the file is
                -- there for the same reason.
                save p in (POSIX file outputPath) as save as PDF
            on error errText
                set pdfNote to "pdf=FAILED (" & errText & ")"
            end try
            close p
        end tell
    end timeout
    return "opened; slides=" & slideCount & "; " & pdfNote
end run
