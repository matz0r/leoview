//! Justified layout for the overview: rows of (nearly) equal height where every
//! image keeps its aspect ratio and each full row fills the width exactly.

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// Item range `start..end` in this row.
    pub start: usize,
    pub end: usize,
    pub y: f32,
    pub height: f32,
}

/// Packs items with the given aspect ratios (width / height) into rows of `width`,
/// each as close to `target_height` as possible. The last row is not stretched.
/// Returns the rows and the total height.
pub fn justify(aspects: &[f32], width: f32, target_height: f32, gap: f32) -> (Vec<Row>, f32) {
    let width = width.max(1.0);
    // Height at which `n` items with summed aspect `sum` exactly fill the width.
    let fill_height = |sum: f32, n: usize| (width - gap * (n as f32 - 1.0)).max(1.0) / sum;

    let mut rows = Vec::new();
    let mut y = 0.0;
    let mut start = 0;
    let mut sum = 0.0;
    let mut i = 0;
    while i < aspects.len() {
        let n = i - start + 1;
        let with = sum + aspects[i];
        if with * target_height + gap * (n as f32 - 1.0) < width {
            sum = with;
            i += 1;
            continue;
        }

        // The row is full: end it after item `i` or before it, whichever height
        // lands closer to the target.
        let h_with = fill_height(with, n);
        let take = n == 1
            || (h_with - target_height).abs() <= (fill_height(sum, n - 1) - target_height).abs();
        let (end, row_sum) = if take { (i + 1, with) } else { (i, sum) };
        let height = fill_height(row_sum, end - start);
        rows.push(Row {
            start,
            end,
            y,
            height,
        });
        y += height + gap;
        start = end;
        sum = 0.0;
        i = end;
    }
    if start < aspects.len() {
        rows.push(Row {
            start,
            end: aspects.len(),
            y,
            height: target_height,
        });
        y += target_height + gap;
    }
    (rows, (y - gap).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_width(aspects: &[f32], row: &Row, gap: f32) -> f32 {
        let n = row.end - row.start;
        aspects[row.start..row.end].iter().sum::<f32>() * row.height + gap * (n as f32 - 1.0)
    }

    #[test]
    fn full_rows_fill_width_and_cover_all_items() {
        let aspects = [1.5, 0.66, 1.0, 1.33, 2.5, 0.75, 1.5, 1.5, 0.8, 1.2, 3.0];
        let (rows, total) = justify(&aspects, 1000.0, 200.0, 8.0);

        let mut next = 0;
        for row in &rows {
            assert_eq!(row.start, next);
            assert!(row.end > row.start);
            next = row.end;
        }
        assert_eq!(next, aspects.len());

        for row in &rows[..rows.len() - 1] {
            assert!((row_width(&aspects, row, 8.0) - 1000.0).abs() < 0.01);
            assert!(row.height > 100.0 && row.height < 400.0, "{row:?}");
        }
        let last = rows.last().unwrap();
        assert!((total - (last.y + last.height)).abs() < 0.01);
    }

    #[test]
    fn wide_panorama_gets_its_own_row() {
        let (rows, _) = justify(&[10.0], 500.0, 200.0, 8.0);
        assert_eq!(rows.len(), 1);
        assert!((rows[0].height - 50.0).abs() < 0.01);
    }

    #[test]
    fn empty_input() {
        assert_eq!(justify(&[], 500.0, 200.0, 8.0), (vec![], 0.0));
    }
}
