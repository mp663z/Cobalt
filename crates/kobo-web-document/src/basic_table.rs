//! Shared-column rectangular table paint for the basic engine.
//! Unsupported structures are counted, never painted as independent flex rows.
use super::{rgb, Painter};
use crate::box_tree::BoxKind;
use crate::computed_style::{Direction, Length, Position};
use crate::css_text_page::FontProvider;
use crate::display_list::{Command, Rect, Source};
use crate::table_layout::{automatic_columns, CellIntrinsic};

impl<F: FontProvider> Painter<'_, F> {
    /// Min/max-content measurement with the Page view font hooks. Adjacent
    /// compatible style runs are shaped together, preserving word context.
    fn table_intrinsic(&mut self, cell: usize) -> Result<(u32, u32), &'static str> {
        let node = &self.tree.boxes[cell];
        if node.children.is_empty() {
            return Ok((0, 0));
        }
        if !self.deep_pure_inline(&node.children) {
            return Err("table-cell-content");
        }
        let (runs, _) = self.collect_runs_of(&node.children, node.style.color)?;
        let face = self.face_for(cell);
        let mut min = 0_u32;
        let mut max = 0_u32;
        let mut word = 0_u32;
        let mut pending_space = false;
        let mut start = 0;
        let mut bytes = 0_usize;
        while start < runs.len() {
            let mut end = start + 1;
            while end < runs.len()
                && runs[end].1 == runs[start].1
                && runs[end].3 == runs[start].3
                && runs[end].4 == runs[start].4
                && runs[end].7 == runs[start].7
            {
                end += 1;
            }
            let text: String = runs[start..end].iter().map(|run| run.0.as_str()).collect();
            bytes = bytes.checked_add(text.len()).ok_or("table-measure-bound")?;
            if bytes > 65_536 {
                return Err("table-measure-bound");
            }
            let size = runs[start].3.clamp(1, 256);
            let bold = runs[start].1;
            let italic = runs[start].4;
            let shaped = face.page_shape(&text, size, bold, italic);
            let mut measured = Vec::new();
            if let Some(clusters) = shaped {
                for cluster in clusters {
                    let piece = text
                        .get(cluster.start..cluster.end)
                        .ok_or("table-shape-range")?;
                    measured.push((piece.to_owned(), cluster.advance));
                }
            } else {
                for ch in text.chars() {
                    measured.push((
                        ch.to_string(),
                        face.page_advance(ch, size, bold, italic)
                            .ok_or("table-font-metrics")?,
                    ));
                }
            }
            for (piece, advance) in measured {
                if piece.chars().all(|c| c.is_ascii_whitespace()) {
                    min = min.max(word);
                    word = 0;
                    pending_space = max > 0;
                    continue;
                }
                if pending_space {
                    let space = face
                        .page_advance(' ', size, bold, italic)
                        .ok_or("table-font-metrics")?;
                    max = max.checked_add(space).ok_or("table-measure-overflow")?;
                    pending_space = false;
                }
                max = max.checked_add(advance).ok_or("table-measure-overflow")?;
                word = word.checked_add(advance).ok_or("table-measure-overflow")?;
                if piece.ends_with(['-', '/', '\u{2010}', '\u{2013}']) {
                    min = min.max(word);
                    word = 0;
                }
            }
            start = end;
        }
        Ok((min.max(word), max))
    }

    fn table_rows(&self, table: usize) -> Result<Vec<usize>, &'static str> {
        let mut rows = Vec::new();
        let mut stack: Vec<_> = self.tree.boxes[table]
            .children
            .iter()
            .rev()
            .copied()
            .collect();
        let mut visits = 0;
        while let Some(index) = stack.pop() {
            visits += 1;
            if visits > 4096 {
                return Err("table-structure-bound");
            }
            let node = &self.tree.boxes[index];
            match node.kind {
                BoxKind::TableRow => rows.push(index),
                BoxKind::TableRowGroup => {
                    if node.style.used_background_color().is_some()
                        || node.style.decor.has_painted_border()
                    {
                        return Err("table-group-decoration");
                    }
                    stack.extend(node.children.iter().rev().copied());
                }
                BoxKind::Text
                    if node
                        .text
                        .as_deref()
                        .is_some_and(|text| text.trim().is_empty()) => {}
                _ => return Err("table-structure"),
            }
        }
        if rows.len() > 512 {
            return Err("table-row-bound");
        }
        Ok(rows)
    }

    /// Paints only a rectangular, non-spanning grid. Every row shares one
    /// resolved set of intrinsic columns. Table/cell author selectors survive
    /// because the parser sees actual table elements rather than rewritten divs.
    #[allow(clippy::too_many_lines)] // Validate a bounded grid before emitting any cell ink.
    pub(super) fn layout_table(
        &mut self,
        table: usize,
        x: i64,
        top: i64,
        available: u32,
        commands: &mut Vec<Command>,
        origin: (i64, i64),
    ) -> Result<i64, &'static str> {
        let style = self.tree.boxes[table].style;
        if style.direction != Direction::Ltr
            || style.positioned.position != Position::Static
            || style.positioned.hidden
        {
            return Err("table-style");
        }
        if [
            style.padding_top,
            style.padding_bottom,
            style.padding_left,
            style.padding_right,
        ]
        .iter()
        .any(|v| *v != 0)
            || style.decor.has_painted_border()
        {
            return Err("table-outer-edges");
        }
        let rows = self.table_rows(table)?;
        let mut grid = Vec::new();
        let mut cells = Vec::new();
        let mut columns = None;
        for &row in &rows {
            let row_style = self.tree.boxes[row].style;
            if row_style.decor.has_painted_border() {
                return Err("table-row-border");
            }
            let mut row_cells = Vec::new();
            for &cell in &self.tree.boxes[row].children {
                let node = &self.tree.boxes[cell];
                if node.kind == BoxKind::Text
                    && node.text.as_deref().is_some_and(|s| s.trim().is_empty())
                {
                    continue;
                }
                if node.kind != BoxKind::TableCell {
                    return Err("table-row-child");
                }
                if node.style.decor.has_painted_border()
                    || [
                        node.style.padding_top,
                        node.style.padding_right,
                        node.style.padding_bottom,
                        node.style.padding_left,
                    ]
                    .iter()
                    .any(|v| *v != 0)
                {
                    return Err("table-cell-edges-not-wired");
                }
                if node.table_span != (1, 1) {
                    return Err("table-span-not-wired");
                }
                if node.style.positioned.position != Position::Static
                    || node.style.positioned.hidden
                    || node.style.white_space_pre
                    || node.style.white_space_nowrap
                {
                    return Err("table-cell-style");
                }
                row_cells.push(cell);
                if cells.len() + row_cells.len() > 4096 {
                    return Err("table-cell-bound");
                }
            }
            let count = row_cells.len();
            if count == 0 || columns.is_some_and(|old| old != count) {
                return Err("table-ragged-grid");
            }
            columns = Some(count);
            grid.push(row_cells);
        }
        let Some(count) = columns else {
            return Ok(0);
        };
        for row in &grid {
            for (column, &cell) in row.iter().enumerate() {
                let cs = self.tree.boxes[cell].style;
                let (mut minimum, mut maximum) = self.table_intrinsic(cell)?;
                let edges = cs
                    .padding_left
                    .checked_add(cs.padding_right)
                    .ok_or("table-cell-edges")?;
                minimum = minimum.checked_add(edges).ok_or("table-cell-edges")?;
                maximum = maximum.checked_add(edges).ok_or("table-cell-edges")?;
                match cs.width {
                    Length::Px(width) => {
                        minimum = minimum.max(width);
                        maximum = maximum.max(width);
                    }
                    Length::Auto => {}
                    Length::Percent(_) => return Err("table-cell-percent-width"),
                }
                cells.push(CellIntrinsic {
                    column,
                    span: 1,
                    minimum,
                    maximum,
                });
            }
        }
        let specified = match style.width {
            Length::Auto => None,
            Length::Px(width) => Some(width.saturating_sub(4)),
            Length::Percent(value) => Some(
                u32::try_from(u64::from(available) * u64::from(value) / 10000)
                    .map_err(|_| "table-width")?
                    .saturating_sub(4),
            ),
        };
        // Default HTML separated-border spacing is 2px on each axis. Author
        // spacing/collapsing declarations remain refused by the style census.
        let columns = automatic_columns(count, &cells, available.saturating_sub(4), specified, 2)
            .map_err(|_| "table-column-sizing")?;
        if columns.overflows {
            self.stats.approx("table-min-content-overflow");
        }
        self.stats.approx("table-cell-top-align");
        let entry = commands.len();
        let mut y = top + 2;
        for (row_number, row) in grid.iter().enumerate() {
            let row_entry = commands.len();
            let mut cell_x = x + 2;
            let mut height = 0;
            for (&cell, &width) in row.iter().zip(&columns.widths) {
                height = height.max(self.layout_block(cell, cell_x, y, width, commands, origin));
                cell_x += i64::from(width) + 2;
            }
            if let Some(color) = self.tree.boxes[rows[row_number]]
                .style
                .used_background_color()
            {
                table_fill(
                    commands,
                    row_entry,
                    x + 2,
                    y,
                    columns.width,
                    height,
                    color,
                    self.tree.boxes[rows[row_number]].source,
                )?;
            }
            y += height + 2;
        }
        if let Some(color) = style.used_background_color() {
            table_fill(
                commands,
                entry,
                x,
                top,
                columns.width + 4,
                y - top,
                color,
                self.tree.boxes[table].source,
            )?;
        }
        self.stats.blocks_painted += 1;
        Ok(y - top)
    }
}

#[allow(clippy::too_many_arguments)]
fn table_fill(
    commands: &mut Vec<Command>,
    at: usize,
    x: i64,
    y: i64,
    width: u32,
    height: i64,
    color: u32,
    node: Option<usize>,
) -> Result<(), &'static str> {
    if height <= 0 || width == 0 {
        return Ok(());
    }
    commands.insert(
        at,
        Command::Fill {
            rect: Rect {
                x: i32::try_from(x).map_err(|_| "table-x")?,
                y: i32::try_from(y).map_err(|_| "table-y")?,
                width,
                height: u32::try_from(height).map_err(|_| "table-height")?,
            },
            color: rgb(color),
            source: Source { node, action: None },
        },
    );
    Ok(())
}
