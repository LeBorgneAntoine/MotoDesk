//! ESC/POS 80 mm ticket and cash-drawer pulse.
//!
//! The byte buffer is the unit under test. Writing it to a COM port or a RAW
//! Windows printer happens after the sale is already sealed.

use crate::local_server::error::{LocalError, LocalResult};

const ESC: u8 = 0x1b;
const GS: u8 = 0x1d;

/// Build a ticket plus an optional drawer kick (`ESC p`).
pub fn render_ticket(text: &str, kick_drawer: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + 64);
    // Initialise, select French code page-ish (PC858 is common on 80 mm).
    out.extend_from_slice(&[ESC, b'@']);
    out.extend_from_slice(&[ESC, b't', 19]); // PC858
    out.extend_from_slice(&[ESC, b'a', 0]); // left align
    for line in text.lines() {
        out.extend_from_slice(&encode_pc858(line));
        out.extend_from_slice(b"\n");
    }
    out.extend_from_slice(b"\n\n");
    // Partial cut.
    out.extend_from_slice(&[GS, b'V', 0]);
    if kick_drawer {
        out.extend_from_slice(&drawer_pulse());
    }
    out
}

/// Encode text for the printer's PC858 code page (selected by `ESC t 19`).
///
/// The printer does not speak UTF-8: sending it raw prints each accented letter
/// as two garbage characters. Characters the page lacks are approximated rather
/// than dropped.
pub fn encode_pc858(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for c in text.chars() {
        let byte = match c {
            ' '..='~' => c as u8,
            'Ç' => 0x80,
            'ü' => 0x81,
            'é' => 0x82,
            'â' => 0x83,
            'ä' => 0x84,
            'à' => 0x85,
            'ç' => 0x87,
            'ê' => 0x88,
            'ë' => 0x89,
            'è' => 0x8a,
            'ï' => 0x8b,
            'î' => 0x8c,
            'É' => 0x90,
            'ô' => 0x93,
            'ö' => 0x94,
            'û' => 0x96,
            'ù' => 0x97,
            'Ü' => 0x9a,
            '«' => 0xae,
            '»' => 0xaf,
            'Â' => 0xb6,
            'À' => 0xb7,
            'Ê' => 0xd2,
            'È' => 0xd4,
            '€' => 0xd5,
            'Î' => 0xd7,
            'Ï' => 0xd8,
            'Ô' => 0xe2,
            'Û' => 0xea,
            'Ù' => 0xeb,
            '°' => 0xf8,
            '\u{a0}' | '\u{202f}' => b' ',
            '\u{2019}' | '\u{2018}' => b'\'',
            '\u{2013}' | '\u{2014}' => b'-',
            'œ' => {
                out.extend_from_slice(b"oe");
                continue;
            }
            'Œ' => {
                out.extend_from_slice(b"OE");
                continue;
            }
            _ => b'?',
        };
        out.push(byte);
    }
    out
}

/// ESC p m t1 t2 — open drawer on pin 2, pulse ~120 ms.
pub fn drawer_pulse() -> [u8; 5] {
    [ESC, b'p', 0, 60, 60]
}

/// Best-effort write. A missing printer must not look like a failed sale.
pub fn write_bytes(target: &str, bytes: &[u8]) -> LocalResult<()> {
    if target.trim().is_empty() {
        return Err(LocalError::BadRequest(
            "aucune imprimante ticket n'est configurée sur cette caisse".into(),
        ));
    }
    check_target(target)?;
    // Opened for writing only, never created or truncated: a printer is a device
    // that already exists, so this cannot make or empty an ordinary file.
    use std::io::Write;
    std::fs::OpenOptions::new()
        .write(true)
        .open(target)
        .and_then(|mut printer| printer.write_all(bytes))
        .map_err(|e| {
            LocalError::BadRequest(format!("imprimante ticket « {target} » injoignable : {e}"))
        })
}

/// The target arrives in a request body, so it must look like a printer and not
/// like a path to any file on the disk (it used to be written with `fs::write`,
/// which overwrote whatever was named).
fn check_target(target: &str) -> LocalResult<()> {
    let t = target.trim();
    let upper = t.to_ascii_uppercase();
    let numbered = |prefix: &str| {
        upper
            .strip_prefix(prefix)
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
    };
    let device = numbered("COM")
        || numbered("LPT")
        || numbered("\\\\.\\COM")
        || numbered("\\\\.\\LPT")
        || numbered("\\\\.\\USBPRINT")
        || t.starts_with("/dev/");
    // A shared printer: \\server\printer (never a drive path).
    let share = t.starts_with("\\\\")
        && !t.starts_with("\\\\.\\")
        && !t.starts_with("\\\\?\\")
        && t.trim_start_matches('\\').split('\\').filter(|p| !p.is_empty()).count() == 2;
    if device && !t.contains("..") || share {
        Ok(())
    } else {
        Err(LocalError::BadRequest(
            "la cible d'impression doit être un port (COM3, LPT1, /dev/usb/lp0) ou une imprimante partagée (\\\\poste\\imprimante)".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticket_bytes_carry_siret_totals_and_drawer_pulse() {
        let text = "Garage Dupont\nSIRET 12345678900012\nTotal TTC    25,20 €\nEspèces";
        let bytes = render_ticket(text, true);
        let as_text = String::from_utf8_lossy(&bytes);
        assert!(as_text.contains("SIRET 12345678900012"));
        assert!(as_text.contains("25,20"));
        assert!(bytes.windows(5).any(|w| w == drawer_pulse()));
        assert_eq!(bytes[0], ESC);
        assert_eq!(bytes[1], b'@');
    }

    #[test]
    fn accents_and_the_euro_sign_reach_the_printer_in_its_own_code_page() {
        assert_eq!(encode_pc858("é"), vec![0x82]);
        assert_eq!(encode_pc858("Caisse n°1 – 12,50 €"), {
            let mut v = b"Caisse n".to_vec();
            v.push(0xf8);
            v.extend_from_slice(b"1 - 12,50 ");
            v.push(0xd5);
            v
        });
        assert_eq!(encode_pc858("œuvre"), b"oeuvre".to_vec());
        assert_eq!(encode_pc858("日"), b"?".to_vec());
    }

    #[test]
    fn only_printers_are_valid_targets() {
        for ok in ["COM3", "com12", "LPT1", "\\\\.\\COM10", "\\\\caisse\\ticket", "/dev/usb/lp0"] {
            assert!(check_target(ok).is_ok(), "{ok} should be accepted");
        }
        for bad in [
            "C:\\Users\\x\\local.db",
            "C:\\Windows\\win.ini",
            "ticket.bin",
            "/etc/passwd",
            "/dev/../etc/passwd",
            "COM",
            "COM3\\..\\x",
            "\\\\?\\C:\\x",
            "\\\\server\\share\\dir\\file.db",
        ] {
            assert!(check_target(bad).is_err(), "{bad} must be refused");
        }
    }

    #[test]
    fn a_missing_printer_does_not_create_a_file() {
        let dir = std::env::temp_dir().join(format!("mg-escpos-{}", std::process::id()));
        let path = dir.join("ticket.bin");
        assert!(write_bytes(path.to_str().unwrap(), b"x").is_err());
        assert!(!path.exists());
    }

    #[test]
    fn a_card_sale_does_not_kick_the_drawer() {
        let bytes = render_ticket("Carte", false);
        assert!(!bytes.windows(5).any(|w| w == drawer_pulse()));
    }
}
