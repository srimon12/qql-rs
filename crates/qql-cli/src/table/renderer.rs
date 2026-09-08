//! Core table layout engine, auto-sizing, padding, and rendering.

use std::io::{self, Write};

use super::cell::{Alignment, Cell, display_width, truncate_width};

/// A `psql`-style table that auto-sizes columns and produces aligned terminal output.
pub struct Table {
    columns: Vec<String>,
    rows: Vec<Vec<Cell>>,
    max_col_width: Option<usize>,
}

impl Table {
    /// Create a new table with the given column names.
    pub fn new(columns: Vec<String>) -> Self {
        Self {
            columns,
            rows: Vec::new(),
            max_col_width: None,
        }
    }

    /// Set an optional maximum display width per column (truncating longer text).
    pub fn with_max_col_width(mut self, max: usize) -> Self {
        self.max_col_width = Some(max);
        self
    }

    /// Add a row of text values.
    pub fn add_row(&mut self, row: Vec<String>) {
        self.add_cells(row.into_iter().map(Cell::text).collect());
    }

    /// Add a row of pre-formatted cells.
    pub fn add_cells(&mut self, mut row: Vec<Cell>) {
        row.truncate(self.columns.len());
        row.resize_with(self.columns.len(), || Cell::text(""));
        self.rows.push(row);
    }

    /// Check whether the table has any data rows.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Print the table to stdout.
    pub fn print(&self) -> io::Result<()> {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        self.render(&mut handle)
    }

    /// Render the table to any destination implementing `std::io::Write`.
    pub fn render(&self, w: &mut impl Write) -> io::Result<()> {
        if self.columns.is_empty() {
            return Ok(());
        }

        let widths = self.compute_widths();
        let alignments = self.compute_alignments();

        self.write_header(w, &widths)?;
        self.write_separator(w, &widths)?;

        if self.is_empty() {
            writeln!(w, "(0 rows)")?;
            return Ok(());
        }

        for row in &self.rows {
            self.write_row(w, &widths, &alignments, row)?;
        }

        let label = if self.rows.len() == 1 { "row" } else { "rows" };
        writeln!(w, "({} {label})", self.rows.len())?;
        Ok(())
    }

    pub(crate) fn compute_widths(&self) -> Vec<usize> {
        let n = self.columns.len();
        let mut widths = vec![0usize; n];
        for (i, col) in self.columns.iter().enumerate() {
            widths[i] = display_width(col);
        }
        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                let mut width = display_width(&cell.value);
                if let Some(max_w) = self.max_col_width {
                    width = width.min(max_w);
                }
                if i < n && width > widths[i] {
                    widths[i] = width;
                }
            }
        }
        widths
    }

    pub(crate) fn compute_alignments(&self) -> Vec<Alignment> {
        (0..self.columns.len())
            .map(|col_idx| {
                let mut has_value = false;
                let mut all_numbers = true;
                for row in &self.rows {
                    let cell = &row[col_idx];
                    if !cell.value.is_empty() {
                        has_value = true;
                        if cell.alignment != Alignment::Right {
                            all_numbers = false;
                            break;
                        }
                    }
                }
                if has_value && all_numbers {
                    Alignment::Right
                } else {
                    Alignment::Left
                }
            })
            .collect()
    }

    fn write_header(&self, w: &mut impl Write, widths: &[usize]) -> io::Result<()> {
        for (i, (column, width)) in self.columns.iter().zip(widths).enumerate() {
            if i > 0 {
                write!(w, "|")?;
            }
            write!(w, " {} ", center(column, *width))?;
        }
        writeln!(w)
    }

    fn write_separator(&self, w: &mut impl Write, widths: &[usize]) -> io::Result<()> {
        for (i, width) in widths.iter().enumerate() {
            if i > 0 {
                write!(w, "+")?;
            }
            for _ in 0..(*width + 2) {
                write!(w, "-")?;
            }
        }
        writeln!(w)
    }

    fn write_row(
        &self,
        w: &mut impl Write,
        widths: &[usize],
        alignments: &[Alignment],
        cells: &[Cell],
    ) -> io::Result<()> {
        for (i, ((cell, width), alignment)) in cells.iter().zip(widths).zip(alignments).enumerate()
        {
            if i > 0 {
                write!(w, "|")?;
            }
            let cell_val = if let Some(max_w) = self.max_col_width {
                truncate_width(&cell.value, max_w)
            } else {
                cell.value.clone()
            };
            let c_width = display_width(&cell_val);
            let padding = width.saturating_sub(c_width);
            match alignment {
                Alignment::Left => {
                    write!(w, " {cell_val}")?;
                    write_spaces(w, padding + 1)?;
                }
                Alignment::Right => {
                    write_spaces(w, padding + 1)?;
                    write!(w, "{cell_val} ")?;
                }
            }
        }
        writeln!(w)
    }
}

/// Centers a text string within a specified display column width.
fn center(value: &str, width: usize) -> String {
    let padding = width.saturating_sub(display_width(value));
    let left = padding / 2;
    let right = padding - left;
    let mut s = String::with_capacity(width);
    for _ in 0..left {
        s.push(' ');
    }
    s.push_str(value);
    for _ in 0..right {
        s.push(' ');
    }
    s
}

/// Writes `count` spaces to writer without heap allocations.
fn write_spaces(w: &mut impl Write, count: usize) -> io::Result<()> {
    const SPACES: &str = "                                                                "; // 64 spaces
    let mut remaining = count;
    while remaining > 0 {
        let chunk = remaining.min(SPACES.len());
        w.write_all(&SPACES.as_bytes()[..chunk])?;
        remaining -= chunk;
    }
    Ok(())
}
