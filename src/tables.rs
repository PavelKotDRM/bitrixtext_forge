//! Экспорт таблиц Markdown в отдельные Excel-файлы (`.xlsx`).
//!
//! Bitrix24 BBCode не поддерживает таблицы, поэтому вместо инлайновой
//! ASCII/текстовой развёртки таблица сохраняется в отдельный файл, а в
//! тексте сообщения остаётся только ссылка на его имя.

use std::path::{Path, PathBuf};

use rust_xlsxwriter::{Color, Format, FormatAlign, FormatBorder, Workbook};

use crate::model::TableAlignment;

/// Таблица, извлечённая из документа для экспорта в Excel.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ExtractedTable {
    pub rows: Vec<Vec<String>>,
    pub alignments: Vec<TableAlignment>,
}

/// Максимальная ширина колонки при автоподборе (в пикселях).
const AUTOFIT_MAX_WIDTH: u32 = 300;
const HEADER_BACKGROUND: u32 = 0x2f_5c_8a;

/// Имя файла для таблицы с порядковым номером `index` (начиная с 0).
pub fn table_file_name(index: usize) -> String {
    format!("table_{}.xlsx", index + 1)
}

fn horizontal_align(alignment: Option<&TableAlignment>) -> FormatAlign {
    match alignment {
        Some(TableAlignment::Center) => FormatAlign::Center,
        Some(TableAlignment::Right) => FormatAlign::Right,
        _ => FormatAlign::Left,
    }
}

/// Формат заголовка: жирный белый текст на тёмном фоне, с рамкой.
fn header_format(alignment: Option<&TableAlignment>) -> Format {
    Format::new()
        .set_bold()
        .set_font_color(Color::White)
        .set_background_color(Color::RGB(HEADER_BACKGROUND))
        .set_border(FormatBorder::Thin)
        .set_align(horizontal_align(alignment))
        .set_align(FormatAlign::VerticalCenter)
}

/// Формат обычной ячейки: рамка со всех сторон и выравнивание по колонке.
fn body_format(alignment: Option<&TableAlignment>) -> Format {
    Format::new()
        .set_border(FormatBorder::Thin)
        .set_align(horizontal_align(alignment))
        .set_align(FormatAlign::VerticalCenter)
}

/// Сохраняет одну таблицу в `.xlsx`-файл по указанному пути с оформлением:
/// границы у всех ячеек, выделенная шапка и автоподбор ширины колонок.
pub fn write_table_xlsx(table: &ExtractedTable, path: &Path) -> anyhow::Result<()> {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    for (row_index, row) in table.rows.iter().enumerate() {
        let is_header = row_index == 0;
        for (col_index, cell) in row.iter().enumerate() {
            let alignment = table.alignments.get(col_index);
            let format = if is_header { header_format(alignment) } else { body_format(alignment) };
            sheet.write_with_format(row_index as u32, col_index as u16, cell.as_str(), &format)?;
        }
    }
    if !table.rows.is_empty() {
        sheet.set_freeze_panes(1, 0)?;
    }
    sheet.set_autofit_max_width(AUTOFIT_MAX_WIDTH);
    sheet.autofit();
    workbook.save(path)?;
    Ok(())
}

/// Сохраняет все извлечённые таблицы в директорию `dir`, используя имена `table_N.xlsx`.
pub fn write_tables_xlsx(tables: &[ExtractedTable], dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut written = Vec::with_capacity(tables.len());
    for (index, table) in tables.iter().enumerate() {
        let path = dir.join(table_file_name(index));
        write_table_xlsx(table, &path)?;
        written.push(path);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_formatted_xlsx_with_header_and_borders() {
        let table = ExtractedTable {
            rows: vec![
                vec!["Имя".to_string(), "Статус".to_string()],
                vec!["Иван".to_string(), "Готово".to_string()],
            ],
            alignments: vec![TableAlignment::Left, TableAlignment::Center],
        };
        let dir = std::env::temp_dir().join(format!("bitrixtext_forge_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("table_1.xlsx");
        write_table_xlsx(&table, &path).expect("xlsx should be written");
        let bytes = std::fs::read(&path).expect("file should exist");
        // .xlsx is a ZIP container: must start with the local file header signature.
        assert_eq!(&bytes[0..2], b"PK", "output is not a valid xlsx/zip container");
        assert!(bytes.len() > 100, "xlsx file looks too small to contain formatting");
        std::fs::remove_dir_all(&dir).ok();
    }
}
