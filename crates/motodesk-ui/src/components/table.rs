//! The data table: a header, and rows drawn only while they are on screen.
//!
//! The table holds no data. It is told how many rows there are and asks for one cell at a
//! time, so the owner keeps its rows in whatever shape it has them:
//!
//! ```ignore
//! let rows = self.rows.clone();            // Rc<Vec<ClientListItem>>
//! DataTable::new("clients", columns, rows.len(), move |row, col, _, _| match col {
//!     0 => rows[row].last_name.clone().into_any_element(),
//!     _ => ...
//! })
//! .selected(self.selected)
//! .on_row_click(cx.listener(|this, row: &usize, _, cx| this.open(*row, cx)))
//! ```

use std::rc::Rc;

use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, CursorStyle, ElementId, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, px, uniform_list,
};

use crate::components::menu::{MenuEntry, fill};
use crate::style::{StyleExt as _, col, row};
use crate::theme::{FONT_MONO, Themed};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Width {
    /// A share of the room left by the fixed columns.
    Flex(f32),
    Fixed(Pixels),
}

/// One column: its header and how its cells are laid out.
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    label: SharedString,
    align: Align,
    width: Width,
    mono: bool,
}

impl Column {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self { label: label.into(), align: Align::Left, width: Width::Flex(1.), mono: false }
    }

    /// Take `share` parts of the free width (default 1).
    pub fn flex(mut self, share: f32) -> Self {
        self.width = Width::Flex(share.max(0.1));
        self
    }

    pub fn fixed(mut self, width: Pixels) -> Self {
        self.width = Width::Fixed(width);
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    /// Right-aligned: amounts and counts.
    pub fn right(self) -> Self {
        self.align(Align::Right)
    }

    /// Fixed-width type: references, serial numbers.
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }
}

type Cell = Rc<dyn Fn(usize, usize, &mut Window, &mut App) -> AnyElement>;
type RowHandler = Rc<dyn Fn(&usize, &mut Window, &mut App)>;
type RowMenu = Rc<dyn Fn(usize, &mut Window, &mut App) -> Vec<MenuEntry>>;
type RowTest = Rc<dyn Fn(usize) -> bool>;

#[derive(IntoElement)]
pub struct DataTable {
    id: ElementId,
    columns: Vec<Column>,
    rows: usize,
    cell: Cell,
    selected: Option<usize>,
    compact: bool,
    row_height: Option<Pixels>,
    auto_height: bool,
    scroll_x: bool,
    muted: Option<RowTest>,
    on_row_click: Option<RowHandler>,
    row_menu: Option<RowMenu>,
}

/// The width a column takes, per share, in a table that scrolls sideways.
const SCROLLED_COLUMN: f32 = 170.;

impl DataTable {
    /// `cell(row, column)` draws one cell. It is called only for the rows on screen.
    pub fn new(
        id: impl Into<ElementId>,
        columns: Vec<Column>,
        rows: usize,
        cell: impl Fn(usize, usize, &mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            columns,
            rows,
            cell: Rc::new(cell),
            selected: None,
            compact: false,
            row_height: None,
            auto_height: false,
            scroll_x: false,
            muted: None,
            on_row_click: None,
            row_menu: None,
        }
    }

    /// The row whose panel is open.
    pub fn selected(mut self, row: Option<usize>) -> Self {
        self.selected = row;
        self
    }

    /// Shorter rows.
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    /// Rows of another height than the two standard ones: for cells that hold two lines.
    pub fn row_height(mut self, height: Pixels) -> Self {
        self.row_height = Some(height);
        self
    }

    /// As tall as its rows, with no scrolling of its own: for a short list inside a page that
    /// scrolls. Every row is drawn, so keep it for lists that stay short.
    pub fn auto_height(mut self) -> Self {
        self.auto_height = true;
        self
    }

    /// Rows written in muted ink: an account switched off, an archived record.
    /// For a table with more columns than fit (a database table as it is): every column
    /// keeps a readable width and the table scrolls sideways, with a scrollbar. A column's
    /// share becomes a multiple of a standard width.
    pub fn scroll_x(mut self) -> Self {
        self.scroll_x = true;
        self
    }

    pub fn muted_rows(mut self, is_muted: impl Fn(usize) -> bool + 'static) -> Self {
        self.muted = Some(Rc::new(is_muted));
        self
    }

    pub fn on_row_click(mut self, handler: impl Fn(&usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_row_click = Some(Rc::new(handler));
        self
    }

    /// The right-click menu of a row.
    pub fn row_menu(mut self, items: impl Fn(usize, &mut Window, &mut App) -> Vec<MenuEntry> + 'static) -> Self {
        self.row_menu = Some(Rc::new(items));
        self
    }
}

fn sized(cell: gpui_kit::Div, column: &Column) -> gpui_kit::Div {
    let cell = match column.width {
        Width::Flex(share) => cell.flex_grow(share).flex_shrink(1.).flex_basis(px(0.)).min_w_0(),
        Width::Fixed(width) => cell.flex_none().w(width),
    };
    match column.align {
        Align::Left => cell.justify_start(),
        Align::Center => cell.justify_center(),
        Align::Right => cell.justify_end(),
    }
}

impl RenderOnce for DataTable {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.ui();
        let (c, m) = (theme.colors, theme.m);
        // Sideways scrolling: every column has a width of its own, and the table their sum.
        let mut columns = self.columns;
        let scrolled_width = self.scroll_x.then(|| {
            let mut total = px(0.);
            for column in &mut columns {
                let width = match column.width {
                    Width::Flex(share) => px(SCROLLED_COLUMN * share),
                    Width::Fixed(width) => width,
                };
                column.width = Width::Fixed(width);
                total += width;
            }
            total
        });
        let columns = Rc::new(columns);
        let last = columns.len().saturating_sub(1);
        let (pad_y, standard_h) = if self.compact { (px(8.), px(33.)) } else { (px(12.), px(41.)) };
        let row_h = self.row_height.unwrap_or(standard_h);
        let (inner, outer) = (m.space_3, m.space_5);
        let edge = move |i: usize, cell: gpui_kit::Div| {
            cell.px(inner).when(i == 0, |d| d.pl(outer)).when(i == last, |d| d.pr(outer))
        };

        let header = row().flex_none().w_full().bg(c.bg_surface).hairline_bottom(c.border).children(
            columns.iter().enumerate().map(|(i, column)| {
                edge(i, sized(row(), column))
                    .py(if self.compact { px(10.) } else { px(12.) })
                    .text_size(m.fs_xs)
                    .line_height(m.fs_xs)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c.text_muted)
                    .whitespace_nowrap()
                    .child(SharedString::from(column.label.to_uppercase()))
            }),
        );

        let (cell, selected, rows, muted) = (self.cell, self.selected, self.rows, self.muted);
        let (on_row_click, row_menu) = (self.on_row_click, self.row_menu);
        let clickable = on_row_click.is_some();

        let draw_row = move |ix: usize, window: &mut Window, cx: &mut App| -> AnyElement {
            let is_selected = selected == Some(ix);
            let ink = if muted.as_ref().is_some_and(|is_muted| is_muted(ix)) { c.text_muted } else { c.text };
            let line = row()
                .id(("row", ix))
                .relative()
                .flex_none()
                .w_full()
                .h(row_h)
                .bg(if is_selected { c.bg_selected } else { c.bg_surface })
                .when(ix + 1 != rows, |this| this.hairline_bottom(c.border_subtle))
                .when(clickable, |this| this.cursor(CursorStyle::PointingHand))
                .when(clickable && !is_selected, |this| this.hover(|s| s.bg(c.bg_hover)))
                .children(columns.iter().enumerate().map(|(i, column)| {
                    edge(i, sized(row(), column))
                        .py(pad_y)
                        .text_size(if column.mono { m.fs_xs } else { m.fs_sm })
                        .text_color(ink)
                        .when(column.mono, |d| d.font_family(FONT_MONO))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(cell(ix, i, window, cx))
                }))
                // The accent bar of the selected row. Drawn over the row: an element has one
                // border colour, and the row's is its bottom hairline.
                .when(is_selected, |this| {
                    this.child(div().absolute().top_0().bottom_0().left_0().w(px(3.)).bg(c.accent))
                })
                .when_some(on_row_click.clone(), |this, handler| {
                    this.on_click(move |_, window, cx| handler(&ix, window, cx))
                });
            match row_menu.clone() {
                Some(items) => {
                    line.context_menu(move |menu, window, cx| fill(menu, items(ix, window, cx))).into_any_element()
                }
                None => line.into_any_element(),
            }
        };

        let frame = col().w_full().bg(c.bg_surface).text_color(c.text).child(header);
        if self.auto_height {
            let lines: Vec<AnyElement> = (0..rows).map(|ix| draw_row(ix, window, cx)).collect();
            return frame.children(lines).into_any_element();
        }
        let body = uniform_list(self.id, rows, move |range, window, cx| {
            range.map(|ix| draw_row(ix, window, cx)).collect::<Vec<_>>()
        });
        let table = frame.size_full().min_h_0().child(div().flex_1().min_h_0().child(body.size_full()));
        match scrolled_width {
            // The table keeps its full width inside a frame that scrolls it; a frame wider
            // than the table is filled.
            Some(width) => div()
                .size_full()
                .bg(c.bg_surface)
                .overflow_x_scrollbar()
                .child(table.w(width).min_w_full().flex_none())
                .into_any_element(),
            None => table.into_any_element(),
        }
    }
}
