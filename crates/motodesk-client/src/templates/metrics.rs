//! How wide text is, so that lines break and align the same on screen and on paper.
//!
//! Documents are set in Helvetica, one of the fonts every PDF reader has: nothing is embedded
//! in the file, and the widths below (from Adobe's published metrics, in thousandths of the
//! font size) are all that is needed to lay a page out. Arial, which the screen uses, has the
//! same widths by design.

/// One point, in millimetres.
pub const PT_TO_MM: f64 = 25.4 / 72.0;

/// The height of a line of text, as a multiple of its font size.
pub const LINE_HEIGHT: f64 = 1.25;

/// How far below the top of its line a text's baseline sits, as a multiple of its font size.
pub const BASELINE: f64 = 0.9;

/// Widths of the characters 32 to 255 of the Windows Latin-1 set.
#[rustfmt::skip]
const REGULAR: [u16; 224] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556,
    1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778,
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556,
    333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556,
    556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584, 350,
    556, 350, 222, 556, 333, 1000, 556, 556, 333, 1000, 667, 333, 1000, 350, 611, 350,
    350, 222, 222, 333, 333, 350, 556, 1000, 333, 1000, 500, 333, 944, 350, 500, 667,
    278, 333, 556, 556, 556, 556, 260, 556, 333, 737, 370, 556, 584, 333, 737, 333,
    400, 584, 333, 333, 333, 556, 537, 278, 333, 333, 365, 556, 834, 834, 834, 611,
    667, 667, 667, 667, 667, 667, 1000, 722, 667, 667, 667, 667, 278, 278, 278, 278,
    722, 722, 778, 778, 778, 778, 778, 584, 778, 722, 722, 722, 722, 667, 667, 611,
    556, 556, 556, 556, 556, 556, 889, 500, 556, 556, 556, 556, 278, 278, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 584, 611, 556, 556, 556, 556, 500, 556, 500,
];

#[rustfmt::skip]
const BOLD: [u16; 224] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611,
    975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778,
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584, 556,
    333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611,
    611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584, 350,
    556, 350, 278, 556, 500, 1000, 556, 556, 333, 1000, 667, 333, 1000, 350, 611, 350,
    350, 278, 278, 500, 500, 350, 556, 1000, 333, 1000, 556, 333, 944, 350, 500, 667,
    278, 333, 556, 556, 556, 556, 280, 556, 333, 737, 370, 556, 584, 333, 737, 333,
    400, 584, 333, 333, 333, 611, 556, 278, 333, 333, 365, 556, 834, 834, 834, 611,
    722, 722, 722, 722, 722, 722, 1000, 722, 667, 667, 667, 667, 278, 278, 278, 278,
    722, 722, 778, 778, 778, 778, 778, 584, 778, 722, 722, 722, 722, 667, 667, 611,
    556, 556, 556, 556, 556, 556, 889, 556, 556, 556, 556, 556, 278, 278, 278, 278,
    611, 611, 611, 611, 611, 611, 611, 584, 611, 611, 611, 611, 611, 556, 611, 556,
];

/// The byte of a character in the Windows Latin-1 set PDF text is written in. A character the
/// set does not have (an emoji, an arrow) is written as a question mark.
pub fn latin1_byte(c: char) -> u8 {
    match c {
        ' '..='~' => c as u8,
        '\u{a0}'..='\u{ff}' => c as u32 as u8,
        // Narrow and thin spaces are unbreakable spaces to a printer.
        '\u{202f}' | '\u{2009}' | '\u{2007}' => 0xa0,
        '\u{2212}' => b'-',
        '€' => 0x80,
        '‚' => 0x82,
        'ƒ' => 0x83,
        '„' => 0x84,
        '…' => 0x85,
        '†' => 0x86,
        '‡' => 0x87,
        'ˆ' => 0x88,
        '‰' => 0x89,
        'Š' => 0x8a,
        '‹' => 0x8b,
        'Œ' => 0x8c,
        'Ž' => 0x8e,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' => 0x95,
        '–' => 0x96,
        '—' => 0x97,
        '˜' => 0x98,
        '™' => 0x99,
        'š' => 0x9a,
        '›' => 0x9b,
        'œ' => 0x9c,
        'ž' => 0x9e,
        'Ÿ' => 0x9f,
        _ => b'?',
    }
}

fn char_units(c: char, bold: bool) -> u16 {
    let byte = latin1_byte(c);
    if byte < 32 {
        return 0;
    }
    (if bold { &BOLD } else { &REGULAR })[usize::from(byte) - 32]
}

/// The width of `text` set at `size` points, in millimetres.
pub fn text_width(text: &str, size: f64, bold: bool) -> f64 {
    let units: u32 = text.chars().map(|c| u32::from(char_units(c, bold))).sum();
    f64::from(units) / 1000.0 * size * PT_TO_MM
}

/// The height of one line of text set at `size` points, in millimetres.
pub fn line_height(size: f64) -> f64 {
    size * LINE_HEIGHT * PT_TO_MM
}

/// Break `text` into the lines that fit in `width` millimetres: at its own line breaks, then
/// between words; a word wider than the line is cut. There is always at least one line.
pub fn wrap(text: &str, width: f64, size: f64, bold: bool) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let paragraph = paragraph.trim_end_matches('\r');
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if text_width(&candidate, size, bold) <= width || line.is_empty() && text_width(word, size, bold) <= width {
                line = candidate;
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            // A word that does not fit on a line of its own is cut where the line ends.
            for c in word.chars() {
                let mut longer = line.clone();
                longer.push(c);
                if text_width(&longer, size, bold) > width && !line.is_empty() {
                    lines.push(std::mem::replace(&mut line, c.to_string()));
                } else {
                    line = longer;
                }
            }
        }
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_all_take_the_same_room_so_amounts_line_up() {
        let width = |text: &str| text_width(text, 10.0, false);
        assert_eq!(width("1111"), width("8888"));
        assert_eq!(width("12,50\u{a0}€"), width("98,76\u{a0}€"));
        // 556 thousandths of 10 pt, in millimetres.
        assert!((width("0") - 5.56 * PT_TO_MM).abs() < 1e-9);
        assert!(text_width("Total", 10.0, true) > width("Total"));
        assert_eq!(width(""), 0.0);
        assert_eq!(text_width("abc", 20.0, false), 2.0 * width("abc"));
    }

    #[test]
    fn french_text_is_in_the_printable_set() {
        for c in "éèàçùâêîôûëïüÉÈÀÇœŒ€«»°·—–…’×Ø".chars() {
            assert_ne!(latin1_byte(c), b'?', "{c}");
        }
        assert_eq!(latin1_byte('€'), 0x80);
        assert_eq!(latin1_byte('é'), 0xe9);
        assert_eq!(latin1_byte('\u{202f}'), 0xa0);
        assert_eq!(latin1_byte('→'), b'?');
        assert_eq!(latin1_byte('\t'), b'?');
    }

    #[test]
    fn a_text_breaks_between_words_and_at_its_own_line_breaks() {
        let lines = wrap("Révision des 24 000 km. RAS transmission.\nProchain contrôle", 40.0, 8.0, false);
        assert!(lines.len() >= 3, "{lines:?}");
        assert_eq!(lines.last().map(String::as_str), Some("Prochain contrôle"));
        for line in &lines {
            assert!(text_width(line, 8.0, false) <= 40.0, "{line:?} is too wide");
            assert!(!line.starts_with(' ') && !line.ends_with(' '), "{line:?}");
        }
        assert_eq!(lines.join(" ").replace('\n', " "), "Révision des 24 000 km. RAS transmission. Prochain contrôle");

        assert_eq!(wrap("", 40.0, 8.0, false), [""]);
        assert_eq!(wrap("a\n\nb", 40.0, 8.0, false), ["a", "", "b"]);
        // An unbreakable space keeps an amount and its currency together.
        assert_eq!(wrap("12,50\u{a0}€", 100.0, 8.0, false), ["12,50\u{a0}€"]);
    }

    #[test]
    fn a_word_wider_than_the_line_is_cut() {
        let lines = wrap("JYARM34E8PA000001JYARM34E8PA000001", 20.0, 8.0, false);
        assert!(lines.len() > 1);
        assert_eq!(lines.concat(), "JYARM34E8PA000001JYARM34E8PA000001");
        for line in &lines {
            assert!(text_width(line, 8.0, false) <= 20.0);
        }
    }
}
