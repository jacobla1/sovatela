-- Opens an .xlsx in Excel and reads cells back as Excel understands them.
--
-- This is the only way to see what a number actually became. A reference
-- written as 9007199254740993 comes back from a numeric cell as ...992, and
-- the file, the XML and every check in this repo are all perfectly happy about
-- it — Excel is the thing that shows the digits changed. Pass the cells to
-- read after the path.
--
--   osascript qa/office-oracle/excel.applescript /path/to/file.xlsx A1 B2

on run argv
    set inputPath to item 1 of argv
    set report to ""
    with timeout of 600 seconds
        tell application "Microsoft Excel"
            set display alerts to false
            try
                close every workbook saving no
            end try
            -- No `update links never update`: Excel answers -50 to it here,
            -- and a generated workbook has no links to update anyway.
            set wb to open workbook workbook file name inputPath read only true
            set ws to worksheet 1 of wb
            repeat with i from 2 to (count of argv)
                set cellName to item i of argv
                -- `string value`, not `value`: `value` hands back a number,
                -- and asking AppleScript to render that as a string is
                -- another lossy conversion on top of the one being tested.
                set report to report & "; " & cellName & "=" & (string value of range cellName of ws)
            end repeat
            close wb saving no
        end tell
    end timeout
    return "opened" & report
end run
