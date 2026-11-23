//! Bounded intrinsic column sizing for the automatic table layout path.
//!
//! CSS 2.2 section 17.5.2.2 permits different automatic algorithms. This
//! kernel uses cell min/max-content contributions, grows spanning constraints
//! across their columns, and distributes available width between the resulting
//! minimum and maximum sizes. Integer remainders are deterministic. It does not
//! implement border conflict resolution or row layout.

const MAX_COLUMNS: usize = 64;
const MAX_CELLS: usize = 4096;
const MAX_SPAN: usize = 64;
const MAX_WIDTH: u32 = 1_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CellIntrinsic {
    pub column: usize,
    pub span: usize,
    /// Outer min-content width, including the cell's padding and borders.
    pub minimum: u32,
    /// Outer max-content width, including the cell's padding and borders.
    pub maximum: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Columns {
    pub widths: Vec<u32>,
    /// Includes the gaps between columns, but not the table's outer edges.
    pub width: u32,
    /// The minimum grid is wider than its available containing width.
    pub overflows: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SizingError {
    Limit,
    InvalidCell,
    Overflow,
}

fn sum(values: &[u32]) -> Result<u32, SizingError> {
    values
        .iter()
        .try_fold(0_u32, |n, v| n.checked_add(*v).ok_or(SizingError::Overflow))
}

fn grow_equal(values: &mut [u32], amount: u32) -> Result<(), SizingError> {
    let count = u32::try_from(values.len()).map_err(|_| SizingError::Limit)?;
    if count == 0 {
        return Err(SizingError::InvalidCell);
    }
    let each = amount / count;
    let remainder = amount % count;
    for (i, value) in values.iter_mut().enumerate() {
        *value = value
            .checked_add(
                each + u32::from(u32::try_from(i).map_err(|_| SizingError::Limit)? < remainder),
            )
            .ok_or(SizingError::Overflow)?;
    }
    Ok(())
}

/// Resolve an automatic grid against available space and optional specified
/// table width. A specified width is a minimum: min-content can overflow it.
/// Auto width shrink-wraps at max-content when there is spare space.
///
/// # Errors
/// Returns `Limit` for excessive dimensions or work, `InvalidCell` for an
/// invalid span or min/max pair, and `Overflow` for unrepresentable geometry.
pub fn automatic_columns(
    count: usize,
    cells: &[CellIntrinsic],
    available: u32,
    specified: Option<u32>,
    gap: u32,
) -> Result<Columns, SizingError> {
    if count == 0
        || count > MAX_COLUMNS
        || cells.len() > MAX_CELLS
        || available > MAX_WIDTH
        || specified.is_some_and(|v| v > MAX_WIDTH)
        || gap > MAX_WIDTH
    {
        return Err(SizingError::Limit);
    }
    let gaps = gap
        .checked_mul(u32::try_from(count - 1).map_err(|_| SizingError::Limit)?)
        .ok_or(SizingError::Overflow)?;
    let mut minimum = vec![0_u32; count];
    let mut maximum = vec![0_u32; count];
    for cell in cells {
        if cell.span == 0
            || cell.span > MAX_SPAN
            || cell
                .column
                .checked_add(cell.span)
                .is_none_or(|end| end > count)
            || cell.minimum > cell.maximum
            || cell.maximum > MAX_WIDTH
        {
            return Err(SizingError::InvalidCell);
        }
        if cell.span == 1 {
            minimum[cell.column] = minimum[cell.column].max(cell.minimum);
            maximum[cell.column] = maximum[cell.column].max(cell.maximum);
        }
    }
    // Shorter spans establish constraints before wider spans. This preserves
    // every earlier lower bound, including intersecting spans.
    for span in 2..=count {
        for cell in cells.iter().filter(|cell| cell.span == span) {
            let range = cell.column..cell.column + span;
            let inner_gaps = gap
                .checked_mul(u32::try_from(span - 1).map_err(|_| SizingError::Limit)?)
                .ok_or(SizingError::Overflow)?;
            let current = sum(&minimum[range.clone()])?
                .checked_add(inner_gaps)
                .ok_or(SizingError::Overflow)?;
            if current < cell.minimum {
                grow_equal(&mut minimum[range.clone()], cell.minimum - current)?;
            }
            for i in range.clone() {
                maximum[i] = maximum[i].max(minimum[i]);
            }
            let current = sum(&maximum[range.clone()])?
                .checked_add(inner_gaps)
                .ok_or(SizingError::Overflow)?;
            if current < cell.maximum {
                grow_equal(&mut maximum[range], cell.maximum - current)?;
            }
        }
    }
    let min_total = sum(&minimum)?
        .checked_add(gaps)
        .ok_or(SizingError::Overflow)?;
    let max_total = sum(&maximum)?
        .checked_add(gaps)
        .ok_or(SizingError::Overflow)?;
    let target = specified
        .map_or(available.min(max_total), |width| width)
        .max(min_total);
    let extra = target - min_total;
    let capacity = max_total - min_total;
    let mut widths = minimum;
    if extra >= capacity {
        widths.clone_from(&maximum);
        if extra > capacity {
            grow_equal(&mut widths, extra - capacity)?;
        }
    } else if capacity > 0 {
        // Cumulative integer apportionment is exact and monotonic. It avoids
        // allocating a temporary per pixel, even for wide containing blocks.
        let mut used_capacity = 0_u64;
        let mut allocated = 0_u32;
        for (width, max) in widths.iter_mut().zip(maximum) {
            used_capacity += u64::from(max - *width);
            let end = u32::try_from(u64::from(extra) * used_capacity / u64::from(capacity))
                .map_err(|_| SizingError::Overflow)?;
            *width += end - allocated;
            allocated = end;
        }
    }
    let width = sum(&widths)?
        .checked_add(gaps)
        .ok_or(SizingError::Overflow)?;
    Ok(Columns {
        widths,
        width,
        overflows: width > available,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cell(column: usize, span: usize, minimum: u32, maximum: u32) -> CellIntrinsic {
        CellIntrinsic {
            column,
            span,
            minimum,
            maximum,
        }
    }
    #[test]
    fn rows_share_columns_and_auto_shrink_wraps() {
        let cells = [
            cell(0, 1, 20, 80),
            cell(1, 1, 30, 90),
            cell(0, 1, 40, 100),
            cell(1, 1, 10, 40),
        ];
        let layout = automatic_columns(2, &cells, 400, None, 2).unwrap();
        assert_eq!(layout.widths, [100, 90]);
        assert_eq!(layout.width, 192);
        assert!(!layout.overflows);
    }
    #[test]
    fn available_space_interpolates_without_losing_pixels() {
        let cells = [cell(0, 1, 10, 30), cell(1, 1, 20, 60), cell(2, 1, 30, 90)];
        for available in 60..=180 {
            let layout = automatic_columns(3, &cells, available, None, 0).unwrap();
            assert_eq!(layout.width, available);
            for (width, cell) in layout.widths.iter().zip(cells) {
                assert!(*width >= cell.minimum && *width <= cell.maximum);
            }
        }
    }
    #[test]
    fn minimum_content_overflows_instead_of_squeezing_words() {
        let layout = automatic_columns(
            2,
            &[cell(0, 1, 100, 100), cell(1, 1, 100, 100)],
            80,
            None,
            2,
        )
        .unwrap();
        assert_eq!(layout.widths, [100, 100]);
        assert_eq!(layout.width, 202);
        assert!(layout.overflows);
    }
    #[test]
    fn spanning_constraints_and_specified_width_are_preserved() {
        let cells = [cell(0, 1, 10, 20), cell(1, 1, 10, 20), cell(0, 2, 101, 201)];
        let layout = automatic_columns(2, &cells, 400, Some(301), 1).unwrap();
        assert_eq!(layout.width, 301);
        assert!(layout.widths.iter().all(|width| *width >= 50));
        let small = automatic_columns(2, &cells, 40, Some(30), 1).unwrap();
        assert_eq!(small.width, 101);
    }
    #[test]
    fn invalid_and_excessive_inputs_are_refused() {
        assert_eq!(
            automatic_columns(65, &[], 100, None, 0),
            Err(SizingError::Limit)
        );
        for cell in [cell(0, 0, 1, 1), cell(1, 2, 1, 1), cell(0, 1, 10, 1)] {
            assert_eq!(
                automatic_columns(2, &[cell], 100, None, 0),
                Err(SizingError::InvalidCell)
            );
        }
    }
    #[test]
    fn deterministic_mixed_spans_preserve_every_constraint_and_total() {
        // Dependency-free seeded stress coverage. No renderer or parity gate:
        // check the sizing kernel's own bounds, overlap and accounting rules.
        let mut seed = 0x6a09_e667_f3bc_c909_u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for case in 0..512 {
            let count = usize::try_from(next() % 16 + 1).unwrap();
            let gap = u32::try_from(next() % 5).unwrap();
            let mut cells = Vec::new();
            for _ in 0..64 {
                let column = usize::try_from(next() % count as u64).unwrap();
                let span = usize::try_from(next() % (count - column) as u64 + 1).unwrap();
                let minimum = u32::try_from(next() % 200).unwrap();
                let maximum = minimum + u32::try_from(next() % 400).unwrap();
                cells.push(cell(column, span, minimum, maximum));
            }
            let available = u32::try_from(next() % 2000).unwrap();
            let specified = (case % 2 == 0).then(|| u32::try_from(next() % 2400).unwrap());
            let layout = automatic_columns(count, &cells, available, specified, gap).unwrap();
            assert_eq!(layout.widths.len(), count);
            assert_eq!(
                layout.width,
                layout.widths.iter().sum::<u32>() + gap * (u32::try_from(count).unwrap() - 1)
            );
            assert_eq!(layout.overflows, layout.width > available);
            if let Some(specified) = specified {
                assert!(layout.width >= specified);
            }
            for cell in &cells {
                let actual = layout.widths[cell.column..cell.column + cell.span]
                    .iter()
                    .sum::<u32>()
                    + gap * (u32::try_from(cell.span).unwrap() - 1);
                assert!(actual >= cell.minimum, "case {case}: {cell:?} has {actual}");
            }
            assert_eq!(
                layout,
                automatic_columns(count, &cells, available, specified, gap).unwrap()
            );
        }
    }
}
