//! Shared-column rectangular table paint for the basic engine.
//! Unsupported structures are counted, never painted as independent flex rows.
use super::{rgb, Painter};
use crate::box_tree::BoxKind;
use crate::computed_style::{BorderColor, BorderStyle, Direction, Length, Position};
use crate::css_text_page::FontProvider;
use crate::display_list::{Command, Rect, Source};
use crate::table_layout::{automatic_columns, CellIntrinsic};

type TableRow = (Option<usize>, Vec<usize>);

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

    /// CSS 2.2 section 17.2.1: consecutive cells lacking a row parent
    /// receive an anonymous row. Whitespace-only inter-cell text is ignored.
    fn table_rows(&self, table: usize) -> Result<Vec<TableRow>, &'static str> {
        let mut rows = Vec::new();
        let mut anonymous = Vec::new();
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
                BoxKind::TableCell => anonymous.push(index),
                BoxKind::TableRow => {
                    if !anonymous.is_empty() {
                        rows.push((None, std::mem::take(&mut anonymous)));
                    }
                    rows.push((Some(index), node.children.clone()));
                }
                BoxKind::TableRowGroup => {
                    if !anonymous.is_empty() {
                        rows.push((None, std::mem::take(&mut anonymous)));
                    }
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
            if rows.len() > 512 {
                return Err("table-row-bound");
            }
        }
        if !anonymous.is_empty() {
            rows.push((None, anonymous));
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
        _origin: (i64, i64),
    ) -> Result<i64, &'static str> {
        let style = self.tree.boxes[table].style;
        if style.direction != Direction::Ltr
            || style.positioned.position != Position::Static
            || style.positioned.hidden
        {
            return Err("table-style");
        }
        let border = style.decor.used_border();
        if [
            style.padding_top,
            style.padding_right,
            style.padding_bottom,
            style.padding_left,
        ] != border
            || style
                .decor
                .border_style
                .iter()
                .any(|value| !matches!(value, BorderStyle::None | BorderStyle::Solid))
        {
            return Err("table-outer-edges");
        }
        let horizontal_edges = border[1]
            .checked_add(border[3])
            .ok_or("table-outer-edges")?;
        let width_edges = horizontal_edges.checked_add(4).ok_or("table-outer-edges")?;
        let grid_x = x + i64::from(border[3]);
        let rows = self.table_rows(table)?;
        let mut grid = Vec::new();
        let mut cells = Vec::new();
        let mut columns = None;
        let mut total_cells = 0_usize;
        for (row, children) in &rows {
            if row.is_some_and(|index| self.tree.boxes[index].style.decor.has_painted_border()) {
                return Err("table-row-border");
            }
            let mut row_cells = Vec::new();
            for &cell in children {
                let node = &self.tree.boxes[cell];
                if node.kind == BoxKind::Text
                    && node.text.as_deref().is_some_and(|s| s.trim().is_empty())
                {
                    continue;
                }
                if node.kind != BoxKind::TableCell {
                    return Err("table-row-child");
                }
                if node
                    .style
                    .decor
                    .border_style
                    .iter()
                    .any(|style| !matches!(style, BorderStyle::None | BorderStyle::Solid))
                {
                    return Err("table-cell-border-style");
                }
                if [
                    node.style.padding_top,
                    node.style.padding_right,
                    node.style.padding_bottom,
                    node.style.padding_left,
                ]
                .contains(&crate::computed_style::PADDING_UNREPRESENTABLE)
                {
                    return Err("table-cell-padding");
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
                total_cells += 1;
                if total_cells > 4096 {
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
            Length::Px(width) => Some(width.saturating_sub(width_edges)),
            Length::Percent(value) => Some(
                u32::try_from(u64::from(available) * u64::from(value) / 10000)
                    .map_err(|_| "table-width")?
                    .saturating_sub(width_edges),
            ),
        };
        // Default HTML separated-border spacing is 2px on each axis. Author
        // spacing/collapsing declarations remain refused by the style census.
        let columns = automatic_columns(
            count,
            &cells,
            available.saturating_sub(width_edges),
            specified,
            2,
        )
        .map_err(|_| "table-column-sizing")?;
        if columns.overflows {
            self.stats.approx("table-min-content-overflow");
        }
        self.stats.approx("table-cell-top-align");
        let entry = commands.len();
        let mut y = top + i64::from(border[0]) + 2;
        for (row_number, row) in grid.iter().enumerate() {
            let row_entry = commands.len();
            let mut cell_x = grid_x + 2;
            let mut height = 0_i64;
            let mut painted_cells = Vec::new();
            for (&cell, &width) in row.iter().zip(&columns.widths) {
                let node = &self.tree.boxes[cell];
                let cs = node.style;
                let inner_width = width
                    .checked_sub(cs.padding_left)
                    .and_then(|value| value.checked_sub(cs.padding_right))
                    .ok_or("table-cell-padding")?;
                let mut ink = Vec::new();
                let text_height = if node.children.is_empty() {
                    0
                } else {
                    self.paint_text_block(
                        cell,
                        &node.children,
                        cell_x + i64::from(cs.padding_left),
                        y + i64::from(cs.padding_top),
                        inner_width,
                        &mut ink,
                    )?
                };
                let natural =
                    text_height + i64::from(cs.padding_top) + i64::from(cs.padding_bottom);
                let declared = match cs.height {
                    Length::Px(value) => i64::from(value),
                    _ => 0,
                };
                height = height.max(natural.max(declared));
                painted_cells.push((cell, cell_x, width, ink));
                cell_x += i64::from(width) + 2;
            }
            for (cell, cell_x, width, ink) in painted_cells {
                if let Some(color) = self.tree.boxes[cell].style.used_background_color() {
                    table_fill(
                        commands,
                        commands.len(),
                        cell_x,
                        y,
                        width,
                        height,
                        color,
                        self.tree.boxes[cell].source,
                    )?;
                    self.stats.fills += 1;
                }
                self.table_box_border(cell, cell_x, y, width, height, commands)?;
                commands.extend(ink);
                self.stats.blocks_painted += 1;
            }
            if let Some((row, color)) = rows[row_number].0.and_then(|row| {
                self.tree.boxes[row]
                    .style
                    .used_background_color()
                    .map(|color| (row, color))
            }) {
                table_fill(
                    commands,
                    row_entry,
                    grid_x + 2,
                    y,
                    columns.width,
                    height,
                    color,
                    self.tree.boxes[row].source,
                )?;
            }
            y += height + 2;
        }
        y += i64::from(border[2]);
        let outer_width = columns
            .width
            .checked_add(width_edges)
            .ok_or("table-width")?;
        if let Some(color) = style.used_background_color() {
            table_fill(
                commands,
                entry,
                x,
                top,
                outer_width,
                y - top,
                color,
                self.tree.boxes[table].source,
            )?;
        }
        self.table_box_border(table, x, top, outer_width, y - top, commands)?;
        self.stats.blocks_painted += 1;
        Ok(y - top)
    }

    #[allow(clippy::too_many_arguments)]
    fn table_box_border(
        &mut self,
        cell: usize,
        x: i64,
        y: i64,
        width: u32,
        height: i64,
        commands: &mut Vec<Command>,
    ) -> Result<(), &'static str> {
        let node = &self.tree.boxes[cell];
        let [top, right, bottom, left] = node.style.decor.used_border();
        let horizontal = i64::from(left) + i64::from(right);
        let vertical = i64::from(top) + i64::from(bottom);
        if horizontal > i64::from(width) || vertical > height {
            return Err("table-cell-border-size");
        }
        // Box-tree padding already includes border widths. Paint inside that
        // reserved edge, not outward into the neighbouring cell's spacing.
        for (side, bx, by, bw, bh) in [
            (0, x, y, width, i64::from(top)),
            (1, x + i64::from(width - right), y, right, height),
            (
                2,
                x,
                y + height - i64::from(bottom),
                width,
                i64::from(bottom),
            ),
            (3, x, y, left, height),
        ] {
            let color = match node.style.decor.border_color[side] {
                BorderColor::Transparent => continue,
                BorderColor::CurrentColor => node.style.color,
                BorderColor::Opaque(color) => color,
            };
            if bw > 0 && bh > 0 {
                table_fill(commands, commands.len(), bx, by, bw, bh, color, node.source)?;
                self.stats.fills += 1;
            }
        }
        Ok(())
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
