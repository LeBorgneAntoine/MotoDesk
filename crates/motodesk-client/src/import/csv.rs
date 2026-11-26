//! Reading a CSV file the way people export one (port of `desktop/src/lib/import/csv.ts`):
//! whatever the separator, whatever the encoding Excel chose, quotes and all.

/// A CSV file, read.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParsedCsv {
    /// The names of the columns, from the first line, trimmed.
    pub headers: Vec<String>,
    /// The other lines: one trimmed value per column, in the order of `headers`.
    pub rows: Vec<Vec<String>>,
    /// The separator the file uses.
    pub delimiter: char,
}

impl ParsedCsv {
    /// The position of a column, by its name.
    pub fn column(&self, header: &str) -> Option<usize> {
        self.headers.iter().position(|h| h == header)
    }
}

/// The text of a CSV file. Excel on Windows saves "CSV" as Windows-1252, not UTF-8, and
/// reading that as UTF-8 loses every accent: UTF-8 is tried strictly first, Windows-1252 is
/// the fallback.
pub fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.strip_prefix('\u{feff}').unwrap_or(text).to_string(),
        Err(_) => encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned(),
    }
}

const SEPARATORS: [char; 4] = [',', ';', '\t', '|'];

/// Cut a text into records and fields. A quoted field keeps separators and line breaks; two
/// quotes inside it are one quote. `limit` stops after that many records (0: all of them).
fn records(text: &str, delimiter: char, limit: usize) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                other => field.push(other),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            c if c == delimiter => record.push(std::mem::take(&mut field)),
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                record.push(std::mem::take(&mut field));
                out.push(std::mem::take(&mut record));
                if limit > 0 && out.len() >= limit {
                    return out;
                }
            }
            other => field.push(other),
        }
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        out.push(record);
    }
    out
}

fn is_blank(record: &[String]) -> bool {
    record.iter().all(|value| value.trim().is_empty())
}

/// The separator a file uses: the one that cuts its first lines into the same number of
/// columns, and into the most of them.
fn sniff(text: &str) -> char {
    let mut best = (',', 1usize, false);
    for candidate in SEPARATORS {
        let sample: Vec<Vec<String>> = records(text, candidate, 12).into_iter().filter(|r| !is_blank(r)).collect();
        let Some(columns) = sample.first().map(Vec::len) else { continue };
        let steady = sample.iter().all(|record| record.len() == columns);
        let better = match (steady, best.2) {
            (true, false) => columns > 1,
            (false, true) => false,
            _ => columns > best.1,
        };
        if better {
            best = (candidate, columns, steady);
        }
    }
    best.0
}

/// Read a CSV text: the first line names the columns, every other line that is not empty is
/// a row. Values are trimmed; typing them comes later.
pub fn parse(text: &str) -> Result<ParsedCsv, String> {
    let delimiter = sniff(text);
    let mut lines = records(text, delimiter, 0).into_iter().filter(|record| !is_blank(record));
    // The first line that holds anything names the columns: at least one name is not empty.
    let first = lines.next().ok_or("Le fichier est vide.")?;

    // A column without a name is not offered; two of the same name are told apart.
    let mut headers: Vec<String> = Vec::new();
    let mut kept: Vec<usize> = Vec::new();
    for (index, raw) in first.iter().enumerate() {
        let name = raw.trim();
        if name.is_empty() {
            continue;
        }
        let mut unique = name.to_string();
        let mut copy = 1;
        while headers.contains(&unique) {
            unique = format!("{name}_{copy}");
            copy += 1;
        }
        headers.push(unique);
        kept.push(index);
    }

    let rows = lines
        .map(|record| kept.iter().map(|&index| record.get(index).map(|v| v.trim().to_string()).unwrap_or_default()).collect::<Vec<_>>())
        // A line with values only under unnamed columns brings nothing.
        .filter(|row: &Vec<String>| row.iter().any(|value| !value.is_empty()))
        .collect();
    Ok(ParsedCsv { headers, rows, delimiter })
}

/// Write rows as CSV text, with the given separator. A value that holds the separator, a
/// quote, a line break or a space at either end is quoted.
pub fn to_csv(headers: &[String], rows: &[Vec<String>], delimiter: char) -> String {
    let quote = |value: &str| {
        let needs = value.contains([delimiter, '"', '\n', '\r']) || value.starts_with(' ') || value.ends_with(' ');
        if needs { format!("\"{}\"", value.replace('"', "\"\"")) } else { value.to_string() }
    };
    let line = |values: &[String]| values.iter().map(|value| quote(value)).collect::<Vec<_>>().join(&delimiter.to_string());
    std::iter::once(line(headers)).chain(rows.iter().map(|row| line(row))).collect::<Vec<_>>().join("\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn utf8_keeps_its_accents_and_loses_the_byte_order_mark() {
        assert_eq!(decode("\u{feff}nom;prénom\nDupré;Zoé".as_bytes()), "nom;prénom\nDupré;Zoé");
    }

    #[test]
    fn a_file_saved_by_excel_is_read_as_windows_1252() {
        // "prénom;Zoé" with é = 0xE9, which is not valid UTF-8.
        let bytes = [0x70, 0x72, 0xe9, 0x6e, 0x6f, 0x6d, 0x3b, 0x5a, 0x6f, 0xe9];
        assert_eq!(decode(&bytes), "prénom;Zoé");
        // The euro sign is where Windows-1252 and Latin-1 differ.
        assert_eq!(decode(&[0x35, 0x20, 0x80]), "5 €");
    }

    #[test]
    fn the_french_separator_is_found_and_values_are_trimmed() {
        let parsed = parse("Nom ; Prénom;Téléphone\r\nMartin; Léa ;06 12 34 56 78\r\n\r\nDubois;;\r\n").unwrap();
        assert_eq!(parsed.delimiter, ';');
        assert_eq!(parsed.headers, ["Nom", "Prénom", "Téléphone"]);
        assert_eq!(parsed.rows, [row(&["Martin", "Léa", "06 12 34 56 78"]), row(&["Dubois", "", ""])]);
        assert_eq!(parsed.column("Prénom"), Some(1));
        assert_eq!(parsed.column("Email"), None);
    }

    #[test]
    fn commas_tabs_and_pipes_are_found_too() {
        assert_eq!(parse("a,b,c\n1,2,3").unwrap().delimiter, ',');
        assert_eq!(parse("a\tb\n1\t2").unwrap().delimiter, '\t');
        assert_eq!(parse("a|b\n1|2").unwrap().delimiter, '|');
        // A price written the French way does not make the comma the separator.
        let prices = parse("ref;prix\nA1;12,50\nA2;8,00").unwrap();
        assert_eq!(prices.delimiter, ';');
        assert_eq!(prices.rows[0], row(&["A1", "12,50"]));
        // One column only: there is no separator to find, and nothing breaks.
        assert_eq!(parse("nom\nMartin\nDubois").unwrap().rows.len(), 2);
    }

    #[test]
    fn a_quoted_value_keeps_its_separators_quotes_and_line_breaks() {
        let parsed = parse("nom,adresse,note\n\"Martin, Léa\",\"12 rue des Motos\nRennes\",\"dit \"\"Lili\"\"\"").unwrap();
        assert_eq!(parsed.rows, [row(&["Martin, Léa", "12 rue des Motos\nRennes", "dit \"Lili\""])]);
    }

    #[test]
    fn short_lines_are_filled_and_nameless_or_repeated_columns_are_handled() {
        let parsed = parse("nom;;nom;ville\nMartin;x;Léa\n;y;;\n").unwrap();
        assert_eq!(parsed.headers, ["nom", "nom_1", "ville"]);
        // The second line has a value only under the column without a name: it is dropped.
        assert_eq!(parsed.rows, [row(&["Martin", "Léa", ""])]);
    }

    #[test]
    fn an_empty_file_says_so() {
        assert_eq!(parse("").unwrap_err(), "Le fichier est vide.");
        assert_eq!(parse("\n \n").unwrap_err(), "Le fichier est vide.");
        // A first line of separators only is no header: the next line is.
        assert_eq!(parse(";;\na;b;c\n1;2;3").unwrap().headers, ["a", "b", "c"]);
    }

    #[test]
    fn what_is_written_reads_back_the_same() {
        let headers = row(&["ligne_csv", "erreur", "nom"]);
        let rows = [row(&["2", "Nom : requis", ""]), row(&["3", "a; b", " dit \"Lili\" "])];
        let text = to_csv(&headers, &rows, ';');
        assert_eq!(text, "ligne_csv;erreur;nom\r\n2;Nom : requis;\r\n3;\"a; b\";\" dit \"\"Lili\"\" \"");
        let back = parse(&text).unwrap();
        assert_eq!(back.headers, headers);
        assert_eq!(back.rows[1], row(&["3", "a; b", "dit \"Lili\""]));
    }
}
