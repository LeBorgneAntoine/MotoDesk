//! The network's recovery key (see `local_server/recovery_key.rs`).

use axum::http::Method;
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::error::ApiResult;
use crate::local_account::SessionWithKey;
use crate::session::Client;
use crate::templates::metrics::{line_height, text_width, wrap};
use crate::templates::pdf;
use crate::templates::render::{PageDrawing, Pictures, Rgb, Shape};

#[derive(Debug, Clone, Serialize)]
pub struct RecoverPayload {
    pub recovery_key: String,
    pub username: String,
    pub password: String,
    pub confirm: String,
}

/// "Password lost": a session, with the key that replaces the one just used.
pub async fn recover_with_key(client: &Client, payload: &RecoverPayload) -> ApiResult<SessionWithKey> {
    client.call(Method::POST, "/api/auth/local/recover").public().json(payload).send().await
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RecoveryStatus {
    pub exists: bool,
    /// Someone typed it back: it is written down.
    pub confirmed: bool,
    #[serde(default)]
    pub updated_at: Option<String>,
}

pub async fn status(client: &Client) -> ApiResult<RecoveryStatus> {
    client.get("/api/network/recovery").await
}

#[derive(Debug, Clone, Deserialize)]
pub struct IssuedKey {
    pub recovery_key: String,
}

pub async fn issue(client: &Client, password: &str) -> ApiResult<IssuedKey> {
    client.post("/api/network/recovery", &serde_json::json!({ "password": password })).await
}

pub async fn confirm(client: &Client, recovery_key: &str) -> ApiResult<()> {
    client
        .call(Method::POST, "/api/network/recovery/confirm")
        .json(&serde_json::json!({ "recovery_key": recovery_key }))
        .done()
        .await
}

// ---- the key on paper --------------------------------------------------------------------------

const MONTHS: [&str; 12] =
    ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];

/// « 26 novembre 2026 ».
fn long_date(day: NaiveDate) -> String {
    let number = if day.day() == 1 { "1er".to_string() } else { day.day().to_string() };
    format!("{number} {} {}", MONTHS[day.month0() as usize], day.year())
}

/// A page holding nothing but the key and what it is for, as a PDF. Anything in `key` that a
/// key is not made of is left out.
pub fn key_page(key: &str, created: NaiveDate) -> Vec<u8> {
    pdf::write(&[key_drawing(key, created)], &Pictures::new(), "Clé de récupération Motodesk")
}

const WIDTH: f64 = 210.0;
const HEIGHT: f64 = 297.0;
const MARGIN: f64 = 20.0;

fn key_drawing(key: &str, created: NaiveDate) -> PageDrawing {
    const PAD: f64 = 5.0;
    let ink = Rgb::new(0x1a, 0x1a, 0x1a);
    let column = WIDTH - 2.0 * MARGIN;
    let key: String = key.chars().filter(|c| c.is_ascii_digit() || c.is_ascii_uppercase() || *c == '-').collect();

    let mut shapes = Vec::new();
    let mut y = MARGIN;
    let mut write = |text: &str, x: f64, width: f64, size: f64, bold: bool, y: &mut f64| {
        for line in wrap(text, width, size, bold) {
            shapes.push(Shape::Text { x, y: *y, text: line, size, bold, color: ink });
            *y += line_height(size);
        }
    };
    write("Motodesk : clé de récupération", MARGIN, column, 20.0, true, &mut y);
    y += 4.0;
    write(&format!("Créée le {}.", long_date(created)), MARGIN, column, 11.0, false, &mut y);
    y += 8.0;

    // The key on one line, in a frame: as large as the page allows, never cut.
    let inside = column - 2.0 * PAD;
    let size = (20.0 * inside / text_width(&key, 20.0, true).max(1.0)).min(20.0);
    let frame = (y, line_height(size) + 2.0 * PAD);
    y += PAD;
    write(&key, MARGIN + PAD, f64::MAX, size, true, &mut y);
    y += PAD + 8.0;
    write(
        "Elle permet de reprendre la main sur les comptes de l'atelier si un mot de passe est perdu. Rangez ce papier en lieu sûr, loin des \
         postes. Elle ne sert qu'une fois : la suivante la remplace. Elle n'ouvre pas les sauvegardes, qui ont leur propre mot de passe.",
        MARGIN,
        column,
        11.0,
        false,
        &mut y,
    );
    shapes.push(Shape::Rect { x: MARGIN, y: frame.0, w: column, h: frame.1, fill: None, stroke: Some((ink, 0.3)), dashed: false });
    PageDrawing { width: WIDTH, height: HEIGHT, shapes }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    #[test]
    fn a_day_is_written_out_in_french() {
        assert_eq!(long_date(day(2026, 11, 26)), "26 novembre 2026");
        assert_eq!(long_date(day(2027, 8, 1)), "1er août 2027");
    }

    /// Each line of text: what it says, where it starts, how large, and whether it is bold.
    fn texts(drawing: &PageDrawing) -> Vec<(&str, f64, f64, bool)> {
        let mut lines = Vec::new();
        for shape in &drawing.shapes {
            if let Shape::Text { text, x, size, bold, .. } = shape {
                lines.push((text.as_str(), *x, *size, *bold));
            }
        }
        lines
    }

    #[test]
    fn the_page_of_a_key_holds_the_key_whole_and_nothing_else_of_what_was_given() {
        let drawing = key_drawing("7KQ2M-9XWTR-4HBNC-P6VZD-3FJ8Y
<script>", day(2026, 11, 26));
        let lines = texts(&drawing);
        assert_eq!(lines[0].0, "Motodesk : clé de récupération");
        assert_eq!(lines[1].0, "Créée le 26 novembre 2026.");
        assert_eq!((lines[2].0, lines[2].2), ("7KQ2M-9XWTR-4HBNC-P6VZD-3FJ8Y", 20.0));
        let inside = |(text, x, size, bold): &(&str, f64, f64, bool)| x + text_width(text, *size, *bold) <= WIDTH - MARGIN;
        assert!(lines.iter().all(inside), "everything is inside the margins");
        assert!(lines.last().unwrap().0.ends_with("leur propre mot de passe."));

        // A longer key is set smaller rather than cut.
        let long = "7KQ2M-9XWTR-4HBNC-P6VZD-3FJ8Y-7KQ2M-9XWTR-4HBNC-P6VZD-3FJ8Y";
        let drawing = key_drawing(long, day(2026, 11, 26));
        let key = texts(&drawing)[2];
        assert_eq!(key.0, long);
        assert!(key.2 < 20.0 && key.1 + text_width(long, key.2, true) <= WIDTH - MARGIN);

        assert!(key_page("7KQ2M-9XWTR", day(2026, 11, 26)).starts_with(b"%PDF-"));
    }
}
