//! What moving things on a page comes down to, in millimetres: dragging, resizing, snapping to
//! what is around, zooming, and going back on a change. Nothing here draws or knows the mouse.

use std::time::{Duration, Instant};

use motodesk_client::templates::schema::{Element, Layout, Page, MIN_HEIGHT_MM, MIN_WIDTH_MM};

/// The zoom levels the buttons and the wheel step through.
const ZOOM_STEPS: [f32; 13] = [0.25, 0.33, 0.5, 0.66, 0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 2.0, 2.5, 3.0];
/// The zoom the editor opens at, and comes back to: the page fits a usual window.
pub const ZOOM_FIT: f32 = 0.75;

/// Changes of the same thing closer together than this are one step back.
const COALESCE: Duration = Duration::from_millis(500);
const MAX_HISTORY: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Frame {
    pub fn of(element: &Element) -> Self {
        Frame { x: element.x, y: element.y, w: element.w, h: element.h }
    }

    pub fn put_on(self, element: &mut Element) {
        (element.x, element.y, element.w, element.h) = (tenth(self.x), tenth(self.y), tenth(self.w), tenth(self.h));
    }
}

/// Positions are kept to the tenth of a millimetre: finer than paper shows, and it keeps the
/// numbers in the fields readable.
pub fn tenth(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// The edge or corner of a frame a resize pulls on: which sides move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Handle {
    /// -1 moves the left edge, 1 the right one, 0 neither.
    pub horizontal: i8,
    /// -1 moves the top edge, 1 the bottom one, 0 neither.
    pub vertical: i8,
}

impl Handle {
    /// The eight handles, clockwise from the top left.
    pub const ALL: [Handle; 8] = [
        Handle { horizontal: -1, vertical: -1 },
        Handle { horizontal: 0, vertical: -1 },
        Handle { horizontal: 1, vertical: -1 },
        Handle { horizontal: 1, vertical: 0 },
        Handle { horizontal: 1, vertical: 1 },
        Handle { horizontal: 0, vertical: 1 },
        Handle { horizontal: -1, vertical: 1 },
        Handle { horizontal: -1, vertical: 0 },
    ];

    /// Where the handle sits on a frame, as fractions of its width and height.
    pub fn anchor(self) -> (f64, f64) {
        (f64::from(self.horizontal + 1) / 2.0, f64::from(self.vertical + 1) / 2.0)
    }
}

/// A frame moved by (`dx`, `dy`), kept whole on the page.
pub fn moved(original: Frame, dx: f64, dy: f64, page: &Page) -> Frame {
    Frame {
        x: (original.x + dx).min(page.width - original.w).max(0.0),
        y: (original.y + dy).min(page.height - original.h).max(0.0),
        ..original
    }
}

/// A frame resized by pulling `handle` by (`dx`, `dy`). The opposite side does not move; the
/// frame never gets smaller than the minimum, nor leaves the page.
pub fn resized(original: Frame, handle: Handle, dx: f64, dy: f64, page: &Page) -> Frame {
    let (mut left, mut top) = (original.x, original.y);
    let (mut right, mut bottom) = (original.x + original.w, original.y + original.h);
    match handle.horizontal {
        -1 => left = (left + dx).max(0.0).min(right - MIN_WIDTH_MM),
        1 => right = (right + dx).min(page.width).max(left + MIN_WIDTH_MM),
        _ => {}
    }
    match handle.vertical {
        -1 => top = (top + dy).max(0.0).min(bottom - MIN_HEIGHT_MM),
        1 => bottom = (bottom + dy).min(page.height).max(top + MIN_HEIGHT_MM),
        _ => {}
    }
    Frame { x: left, y: top, w: right - left, h: bottom - top }
}

/// The lines a frame can align with along one axis: the edges, the margins and the middle of
/// the page, and the edges and middle of every other frame.
fn targets(page_size: f64, margin: f64, others: impl Iterator<Item = (f64, f64)>) -> Vec<f64> {
    let mut lines = vec![0.0, margin, page_size / 2.0, page_size - margin, page_size];
    for (start, size) in others {
        lines.extend([start, start + size / 2.0, start + size]);
    }
    lines
}

/// The smallest shift that puts the start, the middle or the end of a frame on one of `lines`,
/// when there is one within `reach`: the shift, and the line it lands on.
fn nearest(start: f64, size: f64, lines: &[f64], reach: f64) -> Option<(f64, f64)> {
    [start, start + size / 2.0, start + size]
        .into_iter()
        .flat_map(|point| lines.iter().map(move |line| (line - point, *line)))
        .filter(|(shift, _)| shift.abs() <= reach)
        .min_by(|a, b| a.0.abs().total_cmp(&b.0.abs()))
}

/// A frame being moved, pulled onto what it nearly aligns with. Answers the frame, and the
/// vertical and horizontal lines it now sits on (to show them).
pub fn snapped(frame: Frame, others: &[Frame], page: &Page, reach: f64) -> (Frame, Option<f64>, Option<f64>) {
    let across = targets(page.width, page.margin, others.iter().map(|other| (other.x, other.w)));
    let down = targets(page.height, page.margin, others.iter().map(|other| (other.y, other.h)));
    let x = nearest(frame.x, frame.w, &across, reach);
    let y = nearest(frame.y, frame.h, &down, reach);
    let pulled = Frame { x: frame.x + x.map_or(0.0, |(shift, _)| shift), y: frame.y + y.map_or(0.0, |(shift, _)| shift), ..frame };
    (pulled, x.map(|(_, line)| line), y.map(|(_, line)| line))
}

/// The next zoom level up (`direction` above 0) or down from `zoom`.
pub fn zoom_step(zoom: f32, direction: i32) -> f32 {
    let next = if direction > 0 {
        ZOOM_STEPS.iter().copied().find(|step| *step > zoom + 1e-4)
    } else {
        ZOOM_STEPS.iter().rev().copied().find(|step| *step < zoom - 1e-4)
    };
    next.unwrap_or(if direction > 0 { ZOOM_STEPS[ZOOM_STEPS.len() - 1] } else { ZOOM_STEPS[0] })
}

/// Move the element `id` in the order things are drawn: `by` places towards the front
/// (negative: towards the back). Says whether anything moved.
pub fn restack(layout: &mut Layout, id: &str, by: isize) -> bool {
    let Some(from) = layout.elements.iter().position(|element| element.id == id) else { return false };
    let last = layout.elements.len() - 1;
    // « All the way » is asked as the largest step there is: it must not wrap around.
    let to = (from as isize).saturating_add(by).clamp(0, last as isize) as usize;
    if to == from {
        return false;
    }
    let element = layout.elements.remove(from);
    layout.elements.insert(to, element);
    true
}

struct Step {
    before: Layout,
    /// What the change was about: two changes of the same thing in a row are one step.
    about: Option<String>,
    at: Instant,
}

/// What was, to go back to; and what was undone, to do again.
#[derive(Default)]
pub struct History {
    past: Vec<Step>,
    future: Vec<Layout>,
}

impl History {
    /// Remember `before` as the layout a change is about to replace. `about` names what
    /// changes (an element's text, its position): typing a word is then one step, not one per
    /// letter.
    pub fn record(&mut self, before: &Layout, about: Option<&str>, now: Instant) {
        if let (Some(about), Some(last)) = (about, self.past.last_mut())
            && last.about.as_deref() == Some(about)
            && now.duration_since(last.at) < COALESCE
        {
            last.at = now;
            return;
        }
        if self.past.len() >= MAX_HISTORY {
            self.past.remove(0);
        }
        self.past.push(Step { before: before.clone(), about: about.map(str::to_string), at: now });
        self.future.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// The layout before the last change; `current` is kept to be done again.
    pub fn undo(&mut self, current: &Layout) -> Option<Layout> {
        let step = self.past.pop()?;
        self.future.push(current.clone());
        Some(step.before)
    }

    /// The layout an undo left; `current` goes back among the steps to undo.
    pub fn redo(&mut self, current: &Layout, now: Instant) -> Option<Layout> {
        let next = self.future.pop()?;
        self.past.push(Step { before: current.clone(), about: None, at: now });
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: Page = Page { width: 210.0, height: 297.0, margin: 12.0 };

    fn frame(x: f64, y: f64, w: f64, h: f64) -> Frame {
        Frame { x, y, w, h }
    }

    #[test]
    fn a_frame_being_moved_stays_on_the_page() {
        let start = frame(20.0, 20.0, 80.0, 10.0);
        assert_eq!(moved(start, 5.0, -3.0, &PAGE), frame(25.0, 17.0, 80.0, 10.0));
        assert_eq!(moved(start, -500.0, -500.0, &PAGE), frame(0.0, 0.0, 80.0, 10.0));
        assert_eq!(moved(start, 500.0, 500.0, &PAGE), frame(130.0, 287.0, 80.0, 10.0));
    }

    #[test]
    fn pulling_a_handle_moves_its_sides_only() {
        let start = frame(20.0, 20.0, 80.0, 10.0);
        let [top_left, top, _, right, bottom_right, ..] = Handle::ALL;
        assert_eq!(resized(start, bottom_right, 10.0, 5.0, &PAGE), frame(20.0, 20.0, 90.0, 15.0));
        assert_eq!(resized(start, top_left, 10.0, 5.0, &PAGE), frame(30.0, 25.0, 70.0, 5.0));
        assert_eq!(resized(start, right, 10.0, 99.0, &PAGE), frame(20.0, 20.0, 90.0, 10.0), "a side handle ignores the other axis");
        assert_eq!(resized(start, top, 99.0, -5.0, &PAGE), frame(20.0, 15.0, 80.0, 15.0));
    }

    #[test]
    fn a_frame_is_never_smaller_than_the_minimum_nor_larger_than_the_page() {
        let start = frame(20.0, 20.0, 80.0, 10.0);
        let [top_left, _, _, _, bottom_right, ..] = Handle::ALL;
        // Pulled through its opposite corner: it stops at the smallest size, the far side unmoved.
        assert_eq!(resized(start, bottom_right, -500.0, -500.0, &PAGE), frame(20.0, 20.0, MIN_WIDTH_MM, MIN_HEIGHT_MM));
        assert_eq!(resized(start, top_left, 500.0, 500.0, &PAGE), frame(100.0 - MIN_WIDTH_MM, 30.0 - MIN_HEIGHT_MM, MIN_WIDTH_MM, MIN_HEIGHT_MM));
        assert_eq!(resized(start, bottom_right, 500.0, 500.0, &PAGE), frame(20.0, 20.0, 190.0, 277.0));
        assert_eq!(resized(start, top_left, -500.0, -500.0, &PAGE), frame(0.0, 0.0, 100.0, 30.0));
    }

    #[test]
    fn a_frame_nearly_aligned_is_pulled_into_line() {
        // Its left edge is half a millimetre from the margin: it goes onto it.
        let (pulled, across, down) = snapped(frame(12.5, 100.0, 40.0, 10.0), &[], &PAGE, 1.0);
        assert_eq!((tenth(pulled.x), across, down), (12.0, Some(12.0), None));
        // Its middle is nearly the middle of the page.
        let (pulled, across, _) = snapped(frame(84.6, 100.0, 40.0, 10.0), &[], &PAGE, 1.0);
        assert_eq!((tenth(pulled.x), across), (85.0, Some(105.0)));
        // Its top is nearly the bottom of another frame; too far across from anything to move.
        let other = frame(30.0, 40.0, 50.0, 20.0);
        let (pulled, across, down) = snapped(frame(140.3, 60.4, 20.0, 10.0), &[other], &PAGE, 1.0);
        assert_eq!((pulled.x, tenth(pulled.y), across, down), (140.3, 60.0, None, Some(60.0)));
        // Nothing within reach: it stays where it was put.
        let (pulled, across, down) = snapped(frame(140.3, 133.3, 20.0, 10.0), &[], &PAGE, 1.0);
        assert_eq!((pulled, across, down), (frame(140.3, 133.3, 20.0, 10.0), None, None));
    }

    #[test]
    fn zoom_goes_from_step_to_step_and_stops_at_the_ends() {
        assert_eq!(zoom_step(0.75, 1), 0.9);
        assert_eq!(zoom_step(0.75, -1), 0.66);
        assert_eq!(zoom_step(0.8, 1), 0.9, "from between two steps, the next one");
        assert_eq!(zoom_step(3.0, 1), 3.0);
        assert_eq!(zoom_step(0.25, -1), 0.25);
    }

    #[test]
    fn positions_are_kept_to_the_tenth_of_a_millimetre() {
        assert_eq!(tenth(12.34), 12.3);
        assert_eq!(tenth(12.35), 12.4);
        let mut element = Element::new_rect(0.0, 0.0, 1);
        frame(1.26, 2.04, 30.449, 9.99).put_on(&mut element);
        assert_eq!((element.x, element.y, element.w, element.h), (1.3, 2.0, 30.4, 10.0));
        assert_eq!(Frame::of(&element), frame(1.3, 2.0, 30.4, 10.0));
    }

    fn layout_of(ids: &[&str]) -> Layout {
        let mut layout = Layout::default();
        for id in ids {
            let mut element = Element::new_rect(0.0, 0.0, 1);
            element.id = id.to_string();
            layout.elements.push(element);
        }
        layout
    }

    fn order(layout: &Layout) -> Vec<&str> {
        layout.elements.iter().map(|element| element.id.as_str()).collect()
    }

    #[test]
    fn an_element_moves_towards_the_front_or_the_back() {
        let mut layout = layout_of(&["a", "b", "c"]);
        assert!(restack(&mut layout, "a", 1));
        assert_eq!(order(&layout), ["b", "a", "c"]);
        assert!(restack(&mut layout, "a", isize::MAX));
        assert_eq!(order(&layout), ["b", "c", "a"]);
        assert!(!restack(&mut layout, "a", 1), "already in front");
        assert!(restack(&mut layout, "c", isize::MIN + 1));
        assert_eq!(order(&layout), ["c", "b", "a"]);
        assert!(!restack(&mut layout, "missing", 1));
    }

    #[test]
    fn a_change_is_undone_and_done_again() {
        let now = Instant::now();
        let (empty, one, two) = (layout_of(&[]), layout_of(&["a"]), layout_of(&["a", "b"]));
        let mut history = History::default();
        assert!(!history.can_undo() && !history.can_redo());
        history.record(&empty, None, now);
        history.record(&one, None, now);
        // Now at `two`.
        let back = history.undo(&two).unwrap();
        assert_eq!(order(&back), ["a"]);
        assert!(history.can_redo());
        let again = history.redo(&back, now).unwrap();
        assert_eq!(order(&again), ["a", "b"]);
        assert_eq!(order(&history.undo(&again).unwrap()), ["a"]);
        assert_eq!(order(&history.undo(&one).unwrap()), Vec::<&str>::new());
        assert!(history.undo(&empty).is_none());

        // A new change after going back forgets what could be done again.
        history.record(&empty, None, now);
        assert!(!history.can_redo());
    }

    #[test]
    fn typing_in_one_field_is_one_step_back() {
        let start = Instant::now();
        let (before, during) = (layout_of(&["a"]), layout_of(&["a", "b"]));
        let mut history = History::default();
        history.record(&before, Some("text:a"), start);
        history.record(&during, Some("text:a"), start + Duration::from_millis(200));
        history.record(&during, Some("text:a"), start + Duration::from_millis(600));
        assert_eq!(order(&history.undo(&during).unwrap()), ["a"], "three keystrokes, one step");
        assert!(!history.can_undo());

        // A pause, or another field, starts a new step.
        history.record(&before, Some("text:a"), start);
        history.record(&during, Some("text:a"), start + Duration::from_secs(2));
        history.record(&during, Some("size:a"), start + Duration::from_secs(2));
        assert!(history.undo(&during).is_some() && history.undo(&during).is_some() && history.undo(&during).is_some());
        assert!(!history.can_undo());
    }
}
