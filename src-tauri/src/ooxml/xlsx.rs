//! Tabular data in, `.xlsx` out.

use super::markdown::{flatten, parse, Block};
use super::{escape, Package, REL_BASE};

const BOOK: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
const SHEET: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
const STYLES: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml";
const S: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

/// A number wider than this displays in scientific notation under Excel's
/// General format, however wide its column is.
///
/// Which meant a thirteen-digit order reference — stored perfectly, every
/// digit intact — reached the reader as `4.91512E+12`. The value was right and
/// unreadable, and the only way to see it was to open the file in Excel. The
/// fix is to stop asking for General: style index 1 below is the integer
/// format `0`, which shows every digit.
const GENERAL_DIGIT_LIMIT: usize = 11;

/// The style part, holding the one format this writer needs beyond the default.
///
/// Excel refuses a styles part that omits any of fonts, fills, borders,
/// cellStyleXfs or cellXfs, and the two fills have to be the two it reserves —
/// so this is the smallest thing Excel will accept rather than the smallest
/// thing the schema allows.
const STYLES_XML: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
    r#"<fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts>"#,
    r#"<fills count="2"><fill><patternFill patternType="none"/></fill>"#,
    r#"<fill><patternFill patternType="gray125"/></fill></fills>"#,
    r#"<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>"#,
    r#"<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>"#,
    // Index 0 is General; index 1 is `0`, the built-in integer format.
    r#"<cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>"#,
    r#"<xf numFmtId="1" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/></cellXfs>"#,
    r#"</styleSheet>"#
);

/// A cell is a number or it is text. Nothing else, at this stage.
enum Cell {
    Number(String),
    Text(String),
}

/// Decide whether a cell holds a number.
///
/// Deliberately narrow. A spreadsheet that turns an order reference into
/// scientific notation, or drops the leading zero from a postcode, has damaged
/// the user's data in a way that looks like their mistake — so anything that
/// is not unambiguously a quantity stays text.
fn classify(raw: &str) -> Cell {
    let s = raw.trim();
    if s.is_empty() {
        return Cell::Text(String::new());
    }
    let unsigned = s.strip_prefix('-').unwrap_or(s);

    // A leading zero carries meaning — 007, 01234 — unless it is the units
    // digit of a decimal.
    if unsigned.len() > 1 && unsigned.starts_with('0') && !unsigned.starts_with("0.") {
        return Cell::Text(s.to_string());
    }
    // A spreadsheet cell holds an IEEE-754 double, which carries about fifteen
    // significant digits. Past that the value is silently rounded and cannot be
    // recovered: 9007199254740993 comes back as 9007199254740992, and Excel
    // agrees with the original literal because that is unrepresentable too.
    // Order references, IBAN fragments, phone numbers and timestamps-as-integers
    // all land here — and this function's own rule, that a spreadsheet must not
    // damage data in a way that looks like the user's mistake, is the one being
    // broken.
    const MAX_SIGNIFICANT_DIGITS: usize = 15;
    if unsigned.chars().filter(|c| c.is_ascii_digit()).count() > MAX_SIGNIFICANT_DIGITS {
        return Cell::Text(s.to_string());
    }

    // Thousands separators, currency symbols and percent signs all mean a
    // human wrote this for reading. Converting loses the formatting and
    // changes the value in the percent case.
    if s.parse::<f64>().is_ok()
        && s.chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == '-')
    {
        return Cell::Number(s.to_string());
    }
    Cell::Text(s.to_string())
}

/// A1, B1 … Z1, AA1.
fn reference(col: usize, row: usize) -> String {
    let mut name = String::new();
    let mut n = col;
    loop {
        name.insert(0, (b'A' + (n % 26) as u8) as char);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    format!("{name}{row}")
}

/// Rows of cells from a Markdown table, or from delimited text if there is no
/// table in the input.
pub fn rows_from(src: &str) -> Vec<Vec<String>> {
    let blocks = parse(src);
    let table: Vec<Vec<String>> = blocks
        .iter()
        .find_map(|b| match b {
            Block::Table(rows) => Some(
                rows.iter()
                    .map(|r| r.iter().map(|c| flatten(c)).collect())
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default();
    if !table.is_empty() {
        return table;
    }

    // No table: treat it as delimited text. Tabs win over commas when both
    // are present, because a comma inside prose is far more likely than a tab.
    let delimiter = if src.contains('\t') { '\t' } else { ',' };
    src.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split(delimiter).map(|c| c.trim().to_string()).collect())
        .collect()
}

/// Column widths, so the values are legible without touching anything.
///
/// Every column arrived at Excel's default 8.43 characters. That is not merely
/// narrow: a General-formatted number too wide for its column is *shown in
/// scientific notation*, so a thirteen-digit order reference — stored
/// perfectly, every digit intact — appeared to the reader as `4.9151E+12`.
/// Text simply disappeared under the next column. The file was correct and
/// unreadable, which no check here could see and Excel showed immediately.
///
/// The unit is roughly one `0` of the default font, so the longest value plus
/// a little padding is the right measure. Bounded at both ends: narrower than
/// the default looks broken, and a column wide enough for a paragraph pushes
/// everything else off the screen — past that, wrapping is the reader's call.
fn columns(rows: &[Vec<String>]) -> String {
    const PADDING: usize = 2;
    const MIN: usize = 9;
    const MAX: usize = 60;

    let count = rows.iter().map(Vec::len).max().unwrap_or(0);
    if count == 0 {
        return String::new();
    }
    let cols: String = (0..count)
        .map(|c| {
            let widest = rows
                .iter()
                .filter_map(|r| r.get(c))
                .map(|v| super::pptx::display_width(v))
                .max()
                .unwrap_or(0);
            let width = (widest + PADDING).clamp(MIN, MAX);
            format!(
                r#"<col min="{}" max="{}" width="{width}" customWidth="1"/>"#,
                c + 1,
                c + 1
            )
        })
        .collect();
    format!("<cols>{cols}</cols>")
}

/// Build an `.xlsx` from tabular text.
pub fn from_table(src: &str) -> Result<Vec<u8>, String> {
    let rows = rows_from(src);

    let body: String = rows
        .iter()
        .enumerate()
        .map(|(r, cells)| {
            let row_number = r + 1;
            let tds: String = cells
                .iter()
                .enumerate()
                .map(|(c, raw)| {
                    let at = reference(c, row_number);
                    match classify(raw) {
                        Cell::Number(n) => {
                            let digits = n.chars().filter(char::is_ascii_digit).count();
                            let long = !n.contains('.') && digits > GENERAL_DIGIT_LIMIT;
                            let style = if long { r#" s="1""# } else { "" };
                            format!(r#"<c r="{at}"{style}><v>{n}</v></c>"#)
                        }
                        Cell::Text(t) if t.is_empty() => format!(r#"<c r="{at}"/>"#),
                        // Inline strings rather than a shared-strings part:
                        // one fewer part to keep in step, and these documents
                        // are generated once and read, not edited at scale.
                        Cell::Text(t) => format!(
                            r#"<c r="{at}" t="inlineStr"><is><t xml:space="preserve">{}</t></is></c>"#,
                            escape::text(&t)
                        ),
                    }
                })
                .collect();
            format!(r#"<row r="{row_number}">{tds}</row>"#)
        })
        .collect();

    let sheet = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="{S}">{}<sheetData>{body}</sheetData></worksheet>"#,
        columns(&rows)
    );
    let workbook = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="{S}" xmlns:r="{REL_BASE}"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#
    );

    let mut pkg = Package::new();
    pkg.add_rels(
        "_rels/.rels",
        Package::rels(&[(
            "rId1",
            &format!("{REL_BASE}/officeDocument"),
            "xl/workbook.xml",
        )]),
    );
    pkg.add("xl/workbook.xml", BOOK, workbook);
    pkg.add_rels(
        "xl/_rels/workbook.xml.rels",
        Package::rels(&[
            (
                "rId1",
                &format!("{REL_BASE}/worksheet"),
                "worksheets/sheet1.xml",
            ),
            ("rId2", &format!("{REL_BASE}/styles"), "styles.xml"),
        ]),
    );
    pkg.add("xl/worksheets/sheet1.xml", SHEET, sheet);
    pkg.add("xl/styles.xml", STYLES, STYLES_XML.to_string());
    pkg.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ooxml::validate;

    #[test]
    fn a_generated_workbook_is_structurally_valid() {
        let bytes = from_table("| A | B |\n| --- | --- |\n| 1 | 2 |").unwrap();
        assert_eq!(validate(&bytes), Ok(()));
    }

    #[test]
    fn cell_references_run_past_z() {
        assert_eq!(reference(0, 1), "A1");
        assert_eq!(reference(1, 2), "B2");
        assert_eq!(reference(25, 1), "Z1");
        assert_eq!(reference(26, 1), "AA1");
        assert_eq!(reference(27, 3), "AB3");
        assert_eq!(reference(51, 1), "AZ1");
        assert_eq!(reference(52, 1), "BA1");
    }

    #[test]
    fn quantities_are_numbers() {
        for s in ["0", "1", "128400", "-42", "0.12", "3.14159", "-0.5"] {
            assert!(
                matches!(classify(s), Cell::Number(_)),
                "{s:?} should be a number"
            );
        }
    }

    #[test]
    fn a_number_too_long_to_survive_the_trip_stays_text() {
        // A cell holds an IEEE-754 double: about fifteen significant digits.
        // 9007199254740993 came back as ...992, irreversibly, and Excel agreed
        // with the original literal because that is unrepresentable too. Order
        // references, IBAN fragments, phone numbers and timestamps all land
        // here.
        for s in [
            "9007199254740993",
            "4915123456789012345",
            "12345678901234567890",
        ] {
            assert!(matches!(classify(s), Cell::Text(_)), "{s} should stay text");
        }
        // A phone number that does fit is still a number — the bound is about
        // precision, not about guessing intent.
        assert!(matches!(classify("4915123456789"), Cell::Number(_)));
    }

    #[test]
    fn ordinary_figures_are_unaffected_by_the_precision_bound() {
        for s in ["0", "42", "128400", "-1234.56", "0.125", "999999999999999"] {
            assert!(
                matches!(classify(s), Cell::Number(_)),
                "{s} should be a number"
            );
        }
    }

    fn part(bytes: &[u8], name: &str) -> String {
        use std::io::Read as _;
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut out = String::new();
        zip.by_name(name)
            .unwrap_or_else(|_| panic!("no {name} in the package"))
            .read_to_string(&mut out)
            .unwrap();
        out
    }

    #[test]
    fn columns_are_wide_enough_for_what_is_in_them() {
        // Every column arrived at Excel's default width, so the longest value
        // in each was unreadable — text vanished under the next column, and a
        // number too wide for its column is *shown in scientific notation*
        // however correct it is. Excel is the only thing that shows this.
        let bytes = from_table(
            "| Ref | Description |\n|---|---|\n             | 1 | A description long enough that the default column width hides most of it |\n",
        )
        .unwrap();
        let sheet = part(&bytes, "xl/worksheets/sheet1.xml");
        assert!(sheet.contains("<cols>"), "no column widths at all: {sheet}");
        // The widths must precede the data, or Excel rejects the file.
        assert!(
            sheet.find("<cols>") < sheet.find("<sheetData>"),
            "columns declared after the data"
        );
        let width = |col: &str| -> f64 {
            let at = sheet
                .find(&format!(r#"<col min="{col}""#))
                .unwrap_or_else(|| panic!("no col {col}: {sheet}"));
            let rest = &sheet[at..];
            let w = rest.find(r#"width=""#).unwrap() + r#"width=""#.len();
            let end = rest[w..].find('"').unwrap();
            rest[w..w + end].parse().unwrap()
        };
        // The narrow column stays near the default rather than shrinking to
        // nothing; the wide one grows, and stops well short of the page.
        assert!(width("1") >= 9.0, "the Ref column collapsed");
        assert!(width("2") > 40.0, "the Description column did not grow");
        assert!(width("2") <= 60.0, "a single column took over the sheet");
    }

    #[test]
    fn a_long_whole_number_is_asked_to_show_all_its_digits() {
        // Under General, Excel switches to scientific notation past eleven
        // digits whatever the column width — so a thirteen-digit order
        // reference, stored perfectly, reached the reader as `4.91512E+12`.
        let bytes =
            from_table("| Ref |\n|---|\n| 4915123456789 |\n| 42 |\n| 1234.5678 |\n").unwrap();
        let sheet = part(&bytes, "xl/worksheets/sheet1.xml");
        assert!(
            sheet.contains(r#"s="1"><v>4915123456789</v>"#),
            "the long reference was left on General: {sheet}"
        );
        // Only where it is needed: a short number and a decimal keep General,
        // so they are not forced to look like integers.
        assert!(sheet.contains(r#"><v>42</v>"#) && !sheet.contains(r#"s="1"><v>42</v>"#));
        assert!(
            !sheet.contains(r#"s="1"><v>1234.5678</v>"#),
            "a decimal was made an integer"
        );
        // And the style it points at has to exist, or Excel repairs the file.
        let styles = part(&bytes, "xl/styles.xml");
        assert!(
            styles.contains(r#"<cellXfs count="2""#),
            "style 1 is not defined: {styles}"
        );
    }

    #[test]
    fn the_integer_format_is_not_a_date_format() {
        // This app reads spreadsheets as well as writing them, and a styled
        // number that the reader takes for a date would come back as one.
        let bytes = from_table("| Ref |\n|---|\n| 4915123456789 |\n").unwrap();
        let text = crate::document_text("out.xlsx", &bytes).unwrap();
        assert!(
            text.contains("4915123456789"),
            "our own reader turned it into something else: {text}"
        );
    }

    #[test]
    fn things_that_merely_look_numeric_stay_text() {
        // Each of these is a value a spreadsheet would damage: an order
        // reference losing its leading zeros, a percentage silently becoming a
        // hundred times itself, a formatted figure losing its separators.
        for s in [
            "007",
            "01234",
            "12%",
            "1,234",
            "£99",
            "2026-08-28",
            "1e10",
            "+1",
        ] {
            assert!(
                matches!(classify(s), Cell::Text(_)),
                "{s:?} should stay text"
            );
        }
    }

    #[test]
    fn a_markdown_table_becomes_rows() {
        let rows = rows_from("| Region | Revenue |\n| --- | --- |\n| EMEA | 128400 |");
        assert_eq!(
            rows,
            vec![
                vec!["Region".to_string(), "Revenue".to_string()],
                vec!["EMEA".to_string(), "128400".to_string()],
            ]
        );
    }

    #[test]
    fn csv_without_a_table_still_works() {
        let rows = rows_from("Region,Revenue\nEMEA,128400");
        assert_eq!(
            rows,
            vec![
                vec!["Region".to_string(), "Revenue".to_string()],
                vec!["EMEA".to_string(), "128400".to_string()],
            ]
        );
    }

    #[test]
    fn tabs_win_over_commas() {
        // A comma inside prose is common; a tab is not. Splitting a sentence
        // on its commas would make one column per clause.
        let rows = rows_from("Name\tNote\nEMEA\tup 12%, and rising");
        assert_eq!(
            rows[1],
            vec!["EMEA".to_string(), "up 12%, and rising".to_string()]
        );
    }

    #[test]
    fn text_cells_are_escaped_and_numbers_are_not_quoted() {
        let bytes =
            from_table("| Client | Revenue |\n| --- | --- |\n| Smith & Sons | 94100 |").unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes[..])).unwrap();
        let mut sheet = String::new();
        {
            use std::io::Read as _;
            zip.by_name("xl/worksheets/sheet1.xml")
                .unwrap()
                .read_to_string(&mut sheet)
                .unwrap();
        }
        assert!(sheet.contains("Smith &amp; Sons"), "not escaped: {sheet}");
        assert!(
            sheet.contains("<v>94100</v>"),
            "not written as a number: {sheet}"
        );
    }

    #[test]
    fn an_empty_cell_is_empty_rather_than_an_empty_string() {
        let bytes = from_table("| A | B |\n| --- | --- |\n|  | 2 |").unwrap();
        assert_eq!(validate(&bytes), Ok(()));
    }

    #[test]
    fn awkward_input_does_not_panic() {
        for src in ["", "\n\n", "|", ",", "a", "| A |\n| --- |"] {
            let _ = from_table(src);
        }
    }
}
