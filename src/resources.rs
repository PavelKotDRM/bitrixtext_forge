//! Извлечение и явный экспорт изображений и таблиц Markdown.
//!
//! Таблицы экспортируются в Excel. Локальные изображения копируются в
//! выбранный каталог, а внешние адреса сохраняются как `.url`-файлы:
//! приложение не загружает содержимое из сети автоматически.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::model::{BlockNode, Document, ImageSize, InlineNode};
use crate::tables::{ExtractedTable, write_tables_xlsx};

#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedImage {
    pub url: String,
    pub alt: String,
    pub size: Option<ImageSize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceExport {
    pub table_paths: Vec<PathBuf>,
    pub image_paths: Vec<PathBuf>,
    pub manifest_path: PathBuf,
}

/// Собирает изображения во всех блоках документа в порядке их появления.
pub fn collect_images(document: &Document) -> Vec<ExtractedImage> {
    let mut images = Vec::new();
    collect_blocks(&document.blocks, &mut images);
    images
}

fn collect_blocks(blocks: &[BlockNode], images: &mut Vec<ExtractedImage>) {
    for block in blocks {
        match block {
            BlockNode::Paragraph(inlines)
            | BlockNode::Heading {
                content: inlines, ..
            } => {
                collect_inlines(inlines, images);
            }
            BlockNode::Quote(blocks) => collect_blocks(blocks, images),
            BlockNode::List { items, .. } => {
                for item in items {
                    collect_blocks(item, images);
                }
            }
            BlockNode::Table { rows, .. } => {
                for row in rows {
                    for cell in row {
                        collect_inlines(cell, images);
                    }
                }
            }
            BlockNode::Image { url, alt, size } => images.push(ExtractedImage {
                url: url.clone(),
                alt: alt.clone(),
                size: *size,
            }),
            BlockNode::CodeBlock { .. } | BlockNode::HorizontalRule => {}
        }
    }
}

fn collect_inlines(inlines: &[InlineNode], images: &mut Vec<ExtractedImage>) {
    for inline in inlines {
        match inline {
            InlineNode::Bold(children)
            | InlineNode::Italic(children)
            | InlineNode::Underline(children)
            | InlineNode::Strike(children) => collect_inlines(children, images),
            InlineNode::Link { text, .. } | InlineNode::User { text, .. } => {
                collect_inlines(text, images);
            }
            InlineNode::Color { content, .. } | InlineNode::Size { content, .. } => {
                collect_inlines(content, images);
            }
            InlineNode::Image { url, alt, size } => images.push(ExtractedImage {
                url: url.clone(),
                alt: alt.clone(),
                size: *size,
            }),
            InlineNode::Text(_)
            | InlineNode::Icon { .. }
            | InlineNode::Code(_)
            | InlineNode::SoftBreak
            | InlineNode::HardBreak => {}
        }
    }
}

/// Экспортирует все ресурсы документа в выбранный каталог.
///
/// Для относительных ссылок `source_dir` используется как база поиска
/// локального файла. Если файл не найден или URL внешний, создаётся
/// Internet Shortcut с исходным адресом.
pub fn export_resources(
    tables: &[ExtractedTable],
    images: &[ExtractedImage],
    destination: &Path,
    source_dir: Option<&Path>,
) -> Result<ResourceExport> {
    fs::create_dir_all(destination)
        .with_context(|| format!("Не удалось создать каталог {}", destination.display()))?;

    let table_paths =
        write_tables_xlsx(tables, destination).context("Не удалось экспортировать таблицы")?;
    let mut image_paths = Vec::with_capacity(images.len());
    let mut manifest = String::from("BitrixText Forge — экспорт ресурсов\n\n");

    for (index, image) in images.iter().enumerate() {
        let number = index + 1;
        let source = resolve_local_image(&image.url, source_dir);
        let (path, kind) = if let Some(source) = source {
            let name = format!("image_{number}.{}", image_extension(&image.url, &source));
            let path = destination.join(name);
            if source != path {
                fs::copy(&source, &path).with_context(|| {
                    format!(
                        "Не удалось скопировать изображение {} в {}",
                        source.display(),
                        path.display()
                    )
                })?;
            }
            (path, "скопировано")
        } else {
            let path = destination.join(format!("image_{number}.url"));
            let shortcut = format!("[InternetShortcut]\r\nURL={}\r\n", image.url);
            fs::write(&path, shortcut)
                .with_context(|| format!("Не удалось сохранить ссылку {}", path.display()))?;
            (path, "ссылка")
        };

        let relative = path
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_else(|| path.to_string_lossy());
        writeln!(
            manifest,
            "Изображение {number}\t{}\t{}\t{}\t{}",
            relative,
            kind,
            clean_manifest_field(&image.alt),
            clean_manifest_field(&image.url)
        )
        .expect("writing to String cannot fail");
        image_paths.push(path);
    }

    for (index, path) in table_paths.iter().enumerate() {
        let name = path
            .file_name()
            .map(|value| value.to_string_lossy())
            .unwrap_or_else(|| path.to_string_lossy());
        writeln!(manifest, "Таблица {}\t{}", index + 1, name)
            .expect("writing to String cannot fail");
    }

    let manifest_path = destination.join("resources.txt");
    fs::write(&manifest_path, manifest)
        .with_context(|| format!("Не удалось сохранить реестр {}", manifest_path.display()))?;

    Ok(ResourceExport {
        table_paths,
        image_paths,
        manifest_path,
    })
}

fn clean_manifest_field(value: &str) -> String {
    value.replace(['\r', '\n', '\t'], " ")
}

fn resolve_local_image(url: &str, source_dir: Option<&Path>) -> Option<PathBuf> {
    let raw = url.split(['?', '#']).next().unwrap_or(url);
    let path = if let Some(path_text) = raw.strip_prefix("file://") {
        #[cfg(windows)]
        let path_text = path_text
            .strip_prefix('/')
            .filter(|value| value.as_bytes().get(1) == Some(&b':'))
            .unwrap_or(path_text);
        PathBuf::from(path_text)
    } else {
        PathBuf::from(raw)
    };
    let candidates = if path.is_absolute() {
        vec![path]
    } else {
        let mut candidates = vec![path.clone()];
        if let Some(dir) = source_dir {
            candidates.insert(0, dir.join(path));
        }
        candidates
    };
    candidates.into_iter().find(|candidate| candidate.is_file())
}

fn image_extension(url: &str, source: &Path) -> String {
    source
        .extension()
        .and_then(|value| value.to_str())
        .or_else(|| {
            url.split(['?', '#'])
                .next()
                .and_then(|value| Path::new(value).extension())
                .and_then(|value| value.to_str())
        })
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 10
                && value.chars().all(|c| c.is_ascii_alphanumeric())
        })
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| "bin".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TableAlignment;
    use crate::parser::parse_markdown;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "bitrixtext_forge_resources_{}_{}_{}",
            label,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("test directory should be created");
        dir
    }

    #[test]
    fn collects_block_inline_and_table_images() {
        let document = parse_markdown(
            "![block](block.png)\n\ntext ![inline](inline.png)\n\n| A |\n| --- |\n| ![cell](cell.png) |",
        )
        .document;

        let images = collect_images(&document);
        assert_eq!(images.len(), 3);
        assert_eq!(images[0].alt, "block");
        assert_eq!(images[1].url, "inline.png");
        assert_eq!(images[2].url, "cell.png");
    }

    #[test]
    fn exports_local_images_external_links_tables_and_manifest() {
        let source_dir = temp_dir("source");
        let destination = temp_dir("destination");
        let source_image = source_dir.join("photo.png");
        fs::write(&source_image, b"fake-png").expect("test image should be written");

        let tables = vec![ExtractedTable {
            rows: vec![vec!["A".into()], vec!["1".into()]],
            alignments: vec![TableAlignment::Left],
        }];
        let images = vec![
            ExtractedImage {
                url: "photo.png".into(),
                alt: "Локальное".into(),
                size: None,
            },
            ExtractedImage {
                url: "https://example.com/photo.jpg".into(),
                alt: "Внешнее".into(),
                size: None,
            },
        ];

        let exported = export_resources(&tables, &images, &destination, Some(&source_dir)).unwrap();

        assert!(destination.join("table_1.xlsx").is_file());
        assert_eq!(exported.image_paths.len(), 2);
        assert_eq!(
            fs::read(destination.join("image_1.png")).unwrap(),
            b"fake-png"
        );
        assert_eq!(
            fs::read_to_string(destination.join("image_2.url")).unwrap(),
            "[InternetShortcut]\r\nURL=https://example.com/photo.jpg\r\n"
        );
        let manifest = fs::read_to_string(exported.manifest_path).unwrap();
        assert!(manifest.contains("Локальное"));
        assert!(manifest.contains("table_1.xlsx"));

        fs::remove_dir_all(source_dir).ok();
        fs::remove_dir_all(destination).ok();
    }
}
