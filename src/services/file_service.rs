use std::{
    collections::{BTreeSet, HashMap, HashSet},
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use globwalk::GlobWalkerBuilder;
use grep::{
    matcher::Matcher,
    regex::{RegexMatcher, RegexMatcherBuilder},
    searcher::{Searcher, SearcherBuilder, Sink, SinkContext, SinkMatch},
};
use ignore::{
    WalkBuilder,
    overrides::{Override, OverrideBuilder},
    types::{Types, TypesBuilder},
};
use lopdf::Document;

use crate::{
    error::{LiteCodeError, Result},
    schema::{
        EditInput, EditOutput, GlobInput, GlobOutput, GrepInput, GrepOutput, GrepOutputMode,
        NotebookCellType, NotebookEditInput, NotebookEditMode, NotebookEditOutput, WriteInput,
        WriteOutput,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadFileOutput {
    Text(String),
    Image { data: Vec<u8>, mime_type: String },
    Contents(Vec<ReadContent>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadContent {
    Text(String),
    Image { data: Vec<u8>, mime_type: String },
}

#[derive(Debug)]
struct FileServiceState {
    working_dir: Arc<Mutex<PathBuf>>,
    read_files: Mutex<HashSet<PathBuf>>,
}

#[derive(Clone, Debug)]
pub struct FileService {
    state: Arc<FileServiceState>,
}

impl FileService {
    pub fn new(working_dir: Arc<Mutex<PathBuf>>) -> Self {
        Self {
            state: Arc::new(FileServiceState {
                working_dir,
                read_files: Mutex::new(HashSet::new()),
            }),
        }
    }

    pub fn working_dir(&self) -> PathBuf {
        self.state
            .working_dir
            .lock()
            .expect("working directory lock poisoned")
            .clone()
    }

    pub async fn read_file(
        &self,
        file_path: impl AsRef<Path>,
        offset: Option<usize>,
        limit: Option<usize>,
        pages: Option<&str>,
    ) -> Result<ReadFileOutput> {
        let path = self.require_absolute_file(file_path.as_ref())?;
        let metadata = tokio::fs::metadata(&path).await?;
        if metadata.is_dir() {
            return Err(LiteCodeError::invalid_input(
                "Read can only be used with files, not directories.",
            ));
        }

        if path.extension().and_then(|value| value.to_str()) == Some("pdf") {
            let text = read_pdf_text(&path, pages).await?;
            self.mark_read(&path);

            return Ok(ReadFileOutput::Text(window_file_content(
                &text,
                offset.unwrap_or(0),
                limit.unwrap_or(2_000),
            )));
        }

        let raw = tokio::fs::read(&path).await?;
        if let Some(mime_type) = detect_image_mime_type(&path) {
            self.mark_read(&path);
            return Ok(ReadFileOutput::Image {
                data: raw,
                mime_type: mime_type.to_string(),
            });
        }

        if path.extension().and_then(|value| value.to_str()) == Some("ipynb") {
            let content = render_notebook(&String::from_utf8_lossy(&raw))?;
            self.mark_read(&path);

            return Ok(match content {
                ReadFileOutput::Text(text) if text.is_empty() => {
                    ReadFileOutput::Text("Warning: file exists but is empty.".to_string())
                }
                other => other,
            });
        }

        let text = String::from_utf8_lossy(&raw).into_owned();

        self.mark_read(&path);

        if text.is_empty() {
            return Ok(ReadFileOutput::Text(
                "Warning: file exists but is empty.".to_string(),
            ));
        }

        Ok(ReadFileOutput::Text(window_file_content(
            &text,
            offset.unwrap_or(0),
            limit.unwrap_or(2_000),
        )))
    }

    pub async fn write_file(&self, input: WriteInput) -> Result<WriteOutput> {
        let path = self.require_absolute_file(Path::new(&input.file_path))?;
        let content =
            maybe_decode_unicode_escapes(input.decode_unicode_escapes, &input.content, "content")?;
        let original = match tokio::fs::read_to_string(&path).await {
            Ok(content) => Some(content),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };

        if original.is_some() && !self.has_read(&path) {
            return Err(LiteCodeError::invalid_input(format!(
                "Write requires a prior Read for existing file {}.",
                path.display()
            )));
        }

        let parent = path.parent().ok_or_else(|| {
            LiteCodeError::invalid_input(format!(
                "Cannot determine parent directory for {}.",
                path.display()
            ))
        })?;
        if !parent.exists() {
            return Err(LiteCodeError::invalid_input(format!(
                "Parent directory does not exist for {}.",
                path.display()
            )));
        }

        tokio::fs::write(&path, &content).await?;
        self.mark_read(&path);

        let action = if original.is_some() {
            "overwrote existing file"
        } else {
            "created new file"
        };

        Ok(WriteOutput {
            success: true,
            message: format!(
                "Wrote {} ({action}, {} bytes).",
                path.display(),
                content.len()
            ),
        })
    }

    pub async fn edit_file(&self, input: EditInput) -> Result<EditOutput> {
        let old_string = maybe_decode_unicode_escapes(
            input.decode_unicode_escapes,
            &input.old_string,
            "old_string",
        )?;
        let new_string = maybe_decode_unicode_escapes(
            input.decode_unicode_escapes,
            &input.new_string,
            "new_string",
        )?;

        if old_string == new_string {
            return Err(LiteCodeError::invalid_input(
                "new_string must differ from old_string.",
            ));
        }

        let path = self.require_absolute_file(Path::new(&input.file_path))?;
        if !self.has_read(&path) {
            return Err(LiteCodeError::invalid_input(format!(
                "Edit requires a prior Read for {}.",
                path.display()
            )));
        }

        let original = tokio::fs::read_to_string(&path).await?;
        let occurrences = original.match_indices(&old_string).count();
        if occurrences == 0 {
            return Err(LiteCodeError::invalid_input(format!(
                "old_string was not found in {}.",
                path.display()
            )));
        }
        if occurrences > 1 && !input.replace_all {
            return Err(LiteCodeError::invalid_input(format!(
                "old_string matched multiple locations in {}. Provide more context or set replace_all=true.",
                path.display()
            )));
        }

        let updated = if input.replace_all {
            original.replace(&old_string, &new_string)
        } else {
            original.replacen(&old_string, &new_string, 1)
        };

        tokio::fs::write(&path, &updated).await?;
        self.mark_read(&path);

        let replacements = if input.replace_all { occurrences } else { 1 };
        let location_label = if replacements == 1 {
            "location"
        } else {
            "locations"
        };

        Ok(EditOutput {
            success: true,
            message: format!(
                "Edited {} (replaced {} {}).",
                path.display(),
                replacements,
                location_label
            ),
        })
    }

    pub fn glob_files(&self, input: GlobInput) -> Result<GlobOutput> {
        let root = self.resolve_search_root(input.path.as_deref())?;
        let walker = GlobWalkerBuilder::from_patterns(&root, &[input.pattern.as_str()])
            .build()
            .map_err(|error| LiteCodeError::internal(error.to_string()))?;

        let mut files = walker
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| {
                let modified = entry
                    .metadata()
                    .ok()
                    .and_then(|metadata| metadata.modified().ok());
                (entry.path().display().to_string(), modified)
            })
            .collect::<Vec<_>>();

        files.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));

        let total = files.len();
        let filenames = files
            .into_iter()
            .take(100)
            .map(|(name, _)| name)
            .collect::<Vec<_>>();

        Ok(GlobOutput {
            num_files: total,
            filenames,
        })
    }

    pub async fn grep_files(&self, input: GrepInput) -> Result<GrepOutput> {
        let mode = input.output_mode;
        let target = self.resolve_grep_target(input.path.as_deref())?;
        let limit = input.head_limit.unwrap_or(0);
        let offset = input.offset.unwrap_or(0);

        let before = if mode == GrepOutputMode::Content {
            input
                .before
                .or(input.context)
                .or(input.context_alias)
                .unwrap_or(0)
        } else {
            0
        };
        let after = if mode == GrepOutputMode::Content {
            input
                .after
                .or(input.context)
                .or(input.context_alias)
                .unwrap_or(0)
        } else {
            0
        };

        let results = self
            .run_grep_search(&target, input.clone(), before, after)
            .await?;
        let total_matches = results.total_matches;

        Ok(match mode {
            GrepOutputMode::FilesWithMatches => {
                let filenames = apply_window(&results.matched_files, offset, limit);
                GrepOutput {
                    num_files: filenames.len(),
                    filenames,
                    content: None,
                    num_matches: Some(total_matches),
                }
            }
            GrepOutputMode::Count => {
                let count_entries = results
                    .matched_files
                    .iter()
                    .map(|path| format!("{path}:{}", results.match_counts[path]))
                    .collect::<Vec<_>>();
                let windowed = apply_window(&count_entries, offset, limit);
                let filenames = windowed
                    .iter()
                    .filter_map(|entry| entry.split_once(':').map(|(path, _)| path.to_string()))
                    .collect::<Vec<_>>();
                GrepOutput {
                    num_files: filenames.len(),
                    filenames,
                    content: Some(windowed.join("\n")),
                    num_matches: Some(total_matches),
                }
            }
            GrepOutputMode::Content => {
                let windowed = apply_window(&results.content_lines, offset, limit);
                let filenames = windowed
                    .iter()
                    .map(|entry| entry.path.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>();
                GrepOutput {
                    num_files: filenames.len(),
                    filenames,
                    content: Some(
                        windowed
                            .iter()
                            .map(|entry| entry.text.clone())
                            .collect::<Vec<_>>()
                            .join("\n"),
                    ),
                    num_matches: Some(total_matches),
                }
            }
        })
    }

    pub async fn edit_notebook(&self, input: NotebookEditInput) -> Result<NotebookEditOutput> {
        let path = self.require_absolute_file(Path::new(&input.notebook_path))?;
        if path.extension().and_then(|value| value.to_str()) != Some("ipynb") {
            return Err(LiteCodeError::invalid_input(format!(
                "NotebookEdit requires an .ipynb file, got {}.",
                path.display()
            )));
        }

        let original_notebook = tokio::fs::read_to_string(&path).await?;
        let mut notebook =
            serde_json::from_str::<serde_json::Value>(&original_notebook).map_err(|error| {
                LiteCodeError::invalid_input(format!("Invalid notebook JSON: {error}"))
            })?;
        let cells = notebook
            .get_mut("cells")
            .and_then(|value| value.as_array_mut())
            .ok_or_else(|| {
                LiteCodeError::invalid_input("Notebook does not contain a cells array.")
            })?;
        let output = match input.edit_mode {
            NotebookEditMode::Replace => {
                let ResolvedNotebookCell { index, .. } = resolve_notebook_cell(
                    cells,
                    input.cell_id.as_deref(),
                    input.cell_number,
                    "replace",
                )?;
                let cell = cells
                    .get_mut(index)
                    .and_then(|value| value.as_object_mut())
                    .ok_or_else(|| LiteCodeError::internal("Notebook cell was not an object."))?;
                let cell_type = input
                    .cell_type
                    .unwrap_or_else(|| notebook_cell_type(cell).unwrap_or(NotebookCellType::Code));

                cell.insert(
                    "cell_type".to_string(),
                    serde_json::Value::String(cell_type_string(cell_type)),
                );
                cell.insert("source".to_string(), source_value(&input.new_source));
                normalize_cell_shape(cell, cell_type);

                NotebookEditOutput {
                    success: true,
                    cell_id: None,
                }
            }
            NotebookEditMode::Insert => {
                let cell_type = input.cell_type.ok_or_else(|| {
                    LiteCodeError::invalid_input("NotebookEdit insert mode requires cell_type.")
                })?;
                let cell_id = generated_cell_id();
                let new_cell = new_notebook_cell(&cell_id, cell_type, &input.new_source);
                let index =
                    resolve_insert_index(cells, input.cell_id.as_deref(), input.cell_number)?;
                cells.insert(index, new_cell);

                NotebookEditOutput {
                    success: true,
                    cell_id: Some(cell_id),
                }
            }
            NotebookEditMode::Delete => {
                let ResolvedNotebookCell { index, .. } = resolve_notebook_cell(
                    cells,
                    input.cell_id.as_deref(),
                    input.cell_number,
                    "delete",
                )?;
                cells.remove(index);

                NotebookEditOutput {
                    success: true,
                    cell_id: None,
                }
            }
        };

        let updated_notebook = serde_json::to_string_pretty(&notebook)
            .map_err(|error| LiteCodeError::internal(error.to_string()))?;
        tokio::fs::write(&path, &updated_notebook).await?;
        self.mark_read(&path);

        Ok(output)
    }

    fn resolve_search_root(&self, path: Option<&str>) -> Result<PathBuf> {
        let root = match path {
            Some(value) => {
                let candidate = PathBuf::from(value);
                if candidate.is_absolute() {
                    candidate
                } else {
                    self.working_dir().join(candidate)
                }
            }
            None => self.working_dir(),
        };

        if !root.is_dir() {
            return Err(LiteCodeError::invalid_input(format!(
                "Search path {} is not a directory.",
                root.display()
            )));
        }

        Ok(root)
    }

    fn resolve_grep_target(&self, path: Option<&str>) -> Result<PathBuf> {
        let target = match path {
            Some(value) => {
                let candidate = PathBuf::from(value);
                if candidate.is_absolute() {
                    candidate
                } else {
                    self.working_dir().join(candidate)
                }
            }
            None => self.working_dir(),
        };

        if !target.exists() {
            return Err(LiteCodeError::invalid_input(format!(
                "Search path {} does not exist.",
                target.display()
            )));
        }

        Ok(target)
    }

    async fn run_grep_search(
        &self,
        target: &Path,
        input: GrepInput,
        before: usize,
        after: usize,
    ) -> Result<GrepSearchResult> {
        let target = target.to_path_buf();
        tokio::task::spawn_blocking(move || {
            run_grep_search_blocking(&target, &input, before, after)
        })
        .await?
    }

    fn mark_read(&self, path: &Path) {
        self.state
            .read_files
            .lock()
            .expect("read files lock poisoned")
            .insert(path.to_path_buf());
    }

    fn has_read(&self, path: &Path) -> bool {
        self.state
            .read_files
            .lock()
            .expect("read files lock poisoned")
            .contains(path)
    }

    fn require_absolute_file(&self, path: &Path) -> Result<PathBuf> {
        if !path.is_absolute() {
            return Err(LiteCodeError::invalid_input(format!(
                "Expected an absolute file path, got {}.",
                path.display()
            )));
        }
        Ok(path.to_path_buf())
    }
}

fn maybe_decode_unicode_escapes(enabled: bool, value: &str, field_name: &str) -> Result<String> {
    if !enabled {
        return Ok(value.to_string());
    }

    decode_unicode_escapes(value).map_err(|message| {
        LiteCodeError::invalid_input(format!("Invalid Unicode escape in {field_name}: {message}"))
    })
}

fn decode_unicode_escapes(input: &str) -> std::result::Result<String, String> {
    let mut output = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 1 < bytes.len() && bytes[index + 1] == b'u' {
            let (code_point, next_index) = parse_unicode_code_point(input, index)?;
            let character = char::from_u32(code_point)
                .ok_or_else(|| format!("code point U+{code_point:04X} is invalid"))?;
            output.push(character);
            index = next_index;
            continue;
        }

        let character = input[index..]
            .chars()
            .next()
            .ok_or_else(|| "encountered invalid UTF-8 boundary".to_string())?;
        output.push(character);
        index += character.len_utf8();
    }

    Ok(output)
}

fn parse_unicode_code_point(
    input: &str,
    start: usize,
) -> std::result::Result<(u32, usize), String> {
    let (first, mut next_index) = parse_unicode_escape_unit(input, start)?;

    if !(0xD800..=0xDFFF).contains(&first) {
        return Ok((u32::from(first), next_index));
    }

    if (0xDC00..=0xDFFF).contains(&first) {
        return Err(format!(
            "unexpected low surrogate \\u{first:04X} at byte {start}"
        ));
    }

    if !input[next_index..].starts_with("\\u") {
        return Err(format!(
            "high surrogate \\u{first:04X} at byte {start} must be followed by a low surrogate"
        ));
    }

    let (second, following_index) = parse_unicode_escape_unit(input, next_index)?;
    if !(0xDC00..=0xDFFF).contains(&second) {
        return Err(format!(
            "high surrogate \\u{first:04X} at byte {start} must be followed by a low surrogate, found \\u{second:04X}"
        ));
    }

    next_index = following_index;
    let high = u32::from(first) - 0xD800;
    let low = u32::from(second) - 0xDC00;
    let code_point = 0x10000 + ((high << 10) | low);

    Ok((code_point, next_index))
}

fn parse_unicode_escape_unit(
    input: &str,
    start: usize,
) -> std::result::Result<(u16, usize), String> {
    let bytes = input.as_bytes();
    let end = start + 6;
    if end > bytes.len() {
        return Err(format!(
            "truncated escape at byte {start}; expected four hexadecimal digits after \\u"
        ));
    }

    if bytes[start] != b'\\' || bytes[start + 1] != b'u' {
        return Err(format!("expected Unicode escape at byte {start}"));
    }

    let hex = &input[start + 2..end];
    let value = u16::from_str_radix(hex, 16)
        .map_err(|_| format!("invalid escape \\u{hex} at byte {start}"))?;

    Ok((value, end))
}

const NOTEBOOK_OUTPUT_TEXT_CHAR_LIMIT: usize = 4_000;
const NOTEBOOK_OUTPUT_TRUNCATION_NOTICE: &str = "[output truncated for readability. Use Bash with cat and jq on the .ipynb file to inspect the full output.]";

#[derive(Default)]
struct NotebookRenderBuilder {
    parts: Vec<ReadContent>,
    text_lines: Vec<String>,
}

#[derive(Debug)]
struct NotebookEmbeddedMedia {
    data: Vec<u8>,
    mime_type: String,
}

impl NotebookRenderBuilder {
    fn push_line(&mut self, line: impl Into<String>) {
        self.text_lines.push(line.into());
    }

    fn push_image(&mut self, data: Vec<u8>, mime_type: String) {
        self.flush_text();
        self.parts.push(ReadContent::Image { data, mime_type });
    }

    fn finish(mut self) -> Vec<ReadContent> {
        self.flush_text();
        self.parts
    }

    fn flush_text(&mut self) {
        if self.text_lines.is_empty() {
            return;
        }

        push_text_part(
            &mut self.parts,
            ReadContent::Text(self.text_lines.join("\n")),
        );
        self.text_lines.clear();
    }
}

fn render_notebook(content: &str) -> Result<ReadFileOutput> {
    let notebook = serde_json::from_str::<serde_json::Value>(content)
        .map_err(|error| LiteCodeError::invalid_input(format!("Invalid notebook JSON: {error}")))?;
    let language = notebook_language(&notebook);
    let cells = notebook
        .get("cells")
        .and_then(|value| value.as_array())
        .ok_or_else(|| LiteCodeError::invalid_input("Notebook does not contain a cells array."))?;

    let mut parts = Vec::new();
    for (index, cell) in cells.iter().enumerate() {
        if index > 0 {
            push_text_part(&mut parts, ReadContent::Text("\n".to_string()));
        }
        for part in render_notebook_cell(cell, index, &language) {
            push_text_part(&mut parts, part);
        }
    }

    Ok(normalize_read_contents(parts))
}

fn render_notebook_cell(
    cell: &serde_json::Value,
    index: usize,
    language: &str,
) -> Vec<ReadContent> {
    let mut header = format!("<cell number=\"{index}\"");
    if let Some(cell_id) = cell.get("id").and_then(|value| value.as_str()) {
        header.push_str(&format!(" id=\"{cell_id}\""));
    }
    header.push('>');

    let cell_type = cell
        .get("cell_type")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let source = value_as_lines(cell.get("source")).join("");

    let mut builder = NotebookRenderBuilder::default();
    builder.push_line(header);
    builder.push_line(format!("<cell_type>{cell_type}</cell_type>"));
    if cell_type == "code" {
        builder.push_line(format!("<language>{language}</language>"));
    }
    push_tag_block(&mut builder, "source", &source);

    if let Some(outputs) = cell.get("outputs").and_then(|value| value.as_array()) {
        if !outputs.is_empty() {
            builder.push_line("<outputs>");
            if outputs.len() == 1 {
                render_output(&mut builder, &outputs[0], None);
            } else {
                for (output_index, output) in outputs.iter().enumerate() {
                    render_output(&mut builder, output, Some(output_index));
                }
            }
            builder.push_line("</outputs>");
        }
    }

    builder.push_line("</cell>");
    builder.finish()
}

fn render_output(
    builder: &mut NotebookRenderBuilder,
    output: &serde_json::Value,
    index: Option<usize>,
) {
    if let Some(index) = index {
        builder.push_line(format!("<output number=\"{index}\">"));
    }

    let Some(output) = output.as_object() else {
        push_tag_block(
            builder,
            "output",
            &serde_json::to_string_pretty(output).unwrap_or_else(|_| output.to_string()),
        );
        if index.is_some() {
            builder.push_line("</output>");
        }
        return;
    };

    let mut keys = output.keys().cloned().collect::<Vec<_>>();
    keys.sort_by(|left, right| output_field_order(left).cmp(&output_field_order(right)));

    for key in keys {
        let value = output.get(&key).expect("output field should exist");
        render_output_field(builder, &key, value);
    }

    if index.is_some() {
        builder.push_line("</output>");
    }
}

fn output_field_order(key: &str) -> (usize, &str) {
    let rank = match key {
        "output_type" => 0,
        "name" => 1,
        "execution_count" => 2,
        "text" => 3,
        "data" => 4,
        "ename" => 5,
        "evalue" => 6,
        "traceback" => 7,
        _ => 8,
    };
    (rank, key)
}

fn render_tag_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Array(_) => value_as_lines(Some(value)).join(""),
        serde_json::Value::Object(_) => {
            serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
        }
    }
}

fn render_output_field(builder: &mut NotebookRenderBuilder, key: &str, value: &serde_json::Value) {
    if key == "data" {
        render_output_data(builder, value);
        return;
    }

    let content = render_output_field_value(key, value);
    push_tag_block(builder, key, &content);
}

fn render_output_data(builder: &mut NotebookRenderBuilder, value: &serde_json::Value) {
    let Some(object) = value.as_object() else {
        push_tag_block(builder, "data", &render_tag_value(value));
        return;
    };

    let mut rendered = serde_json::Map::new();
    let mut media = Vec::new();
    for (mime_type, item) in object {
        let (rendered_value, mut embedded_media) = render_output_data_item(mime_type, item);
        rendered.insert(mime_type.clone(), rendered_value);
        media.append(&mut embedded_media);
    }

    push_tag_block(
        builder,
        "data",
        &serde_json::to_string_pretty(&serde_json::Value::Object(rendered))
            .unwrap_or_else(|_| value.to_string()),
    );

    for item in media {
        builder.push_image(item.data, item.mime_type);
    }
}

fn render_output_data_item(
    mime_type: &str,
    value: &serde_json::Value,
) -> (serde_json::Value, Vec<NotebookEmbeddedMedia>) {
    if mime_type.starts_with("image/") {
        let encoded = value_as_lines(Some(value)).join("");
        let normalized = encoded
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>();
        if let Ok(data) = BASE64_STANDARD.decode(normalized.as_bytes()) {
            if !data.is_empty() {
                return (
                    serde_json::Value::String(format!("[{mime_type} output rendered below]")),
                    vec![NotebookEmbeddedMedia {
                        data,
                        mime_type: mime_type.to_string(),
                    }],
                );
            }
        }
    }

    if notebook_mime_type_is_textual(mime_type) {
        let rendered = render_tag_value(value);
        if rendered.chars().count() > NOTEBOOK_OUTPUT_TEXT_CHAR_LIMIT {
            return (
                serde_json::Value::String(truncate_notebook_output_text(rendered)),
                Vec::new(),
            );
        }
    }

    (value.clone(), Vec::new())
}

fn render_output_field_value(key: &str, value: &serde_json::Value) -> String {
    let content = render_tag_value(value);
    if notebook_output_field_is_textual(key) {
        truncate_notebook_output_text(content)
    } else {
        content
    }
}

fn notebook_output_field_is_textual(key: &str) -> bool {
    matches!(key, "text" | "traceback" | "evalue")
}

fn notebook_mime_type_is_textual(mime_type: &str) -> bool {
    mime_type.starts_with("text/")
        || mime_type == "application/json"
        || mime_type.ends_with("+json")
        || mime_type.ends_with("+xml")
        || mime_type == "application/javascript"
}

fn truncate_notebook_output_text(content: String) -> String {
    let total_chars = content.chars().count();
    if total_chars <= NOTEBOOK_OUTPUT_TEXT_CHAR_LIMIT {
        return content;
    }

    let truncated = content
        .chars()
        .take(NOTEBOOK_OUTPUT_TEXT_CHAR_LIMIT)
        .collect::<String>();
    let omitted_chars = total_chars - NOTEBOOK_OUTPUT_TEXT_CHAR_LIMIT;

    format!(
        "{truncated}\n\n{NOTEBOOK_OUTPUT_TRUNCATION_NOTICE}\nOmitted {omitted_chars} characters."
    )
}

fn push_tag_block(builder: &mut NotebookRenderBuilder, tag: &str, content: &str) {
    if !content.contains('\n') {
        builder.push_line(format!("<{tag}>{content}</{tag}>"));
        return;
    }

    builder.push_line(format!("<{tag}>"));
    let block = content.strip_suffix('\n').unwrap_or(content);
    if !block.is_empty() {
        for line in block.lines() {
            builder.push_line(line.to_string());
        }
    }
    builder.push_line(format!("</{tag}>"));
}

fn push_text_part(parts: &mut Vec<ReadContent>, content: ReadContent) {
    match content {
        ReadContent::Text(text) if text.is_empty() => {}
        ReadContent::Text(text) => match parts.last_mut() {
            Some(ReadContent::Text(existing)) => existing.push_str(&text),
            _ => parts.push(ReadContent::Text(text)),
        },
        other => parts.push(other),
    }
}

fn normalize_read_contents(parts: Vec<ReadContent>) -> ReadFileOutput {
    if parts.is_empty() {
        return ReadFileOutput::Text(String::new());
    }

    if parts.len() == 1 {
        match parts
            .into_iter()
            .next()
            .expect("one content item should exist")
        {
            ReadContent::Text(text) => ReadFileOutput::Text(text),
            ReadContent::Image { data, mime_type } => {
                ReadFileOutput::Contents(vec![ReadContent::Image { data, mime_type }])
            }
        }
    } else {
        ReadFileOutput::Contents(parts)
    }
}

async fn read_pdf_text(path: &Path, pages: Option<&str>) -> Result<String> {
    let path = path.to_path_buf();
    let requested_pages = pages.map(ToOwned::to_owned);

    tokio::task::spawn_blocking(move || read_pdf_text_blocking(&path, requested_pages.as_deref()))
        .await?
}

fn read_pdf_text_blocking(path: &Path, pages: Option<&str>) -> Result<String> {
    let document = Document::load(path).map_err(|error| {
        LiteCodeError::invalid_input(format!("Failed to read PDF {}: {error}", path.display()))
    })?;
    let total_pages = document.get_pages().len();
    if total_pages == 0 {
        return Err(LiteCodeError::invalid_input(format!(
            "PDF {} does not contain any pages.",
            path.display()
        )));
    }

    let selected_pages = select_pdf_pages(total_pages, pages)?;
    let mut rendered_pages = Vec::new();
    for page_number in selected_pages {
        let text = document.extract_text(&[page_number]).map_err(|error| {
            LiteCodeError::invalid_input(format!(
                "Failed to extract page {page_number} from PDF {}: {error}",
                path.display()
            ))
        })?;
        let body = text.trim_end();
        rendered_pages.push(if body.is_empty() {
            format!("Page {page_number}\n[no extractable text]")
        } else {
            format!("Page {page_number}\n{body}")
        });
    }

    Ok(rendered_pages.join("\n\n"))
}

fn select_pdf_pages(total_pages: usize, pages: Option<&str>) -> Result<Vec<u32>> {
    if total_pages > 10 && pages.is_none() {
        return Err(LiteCodeError::invalid_input(format!(
            "PDF has {total_pages} pages. You must provide the pages parameter when reading PDFs with more than 10 pages."
        )));
    }

    let selected = match pages {
        Some(spec) => parse_pdf_page_ranges(spec, total_pages)?,
        None => (1..=u32::try_from(total_pages).expect("page count exceeds u32")).collect(),
    };

    if selected.len() > 20 {
        return Err(LiteCodeError::invalid_input(format!(
            "PDF page selection contains {} pages, exceeding the 20-page maximum.",
            selected.len()
        )));
    }

    Ok(selected)
}

fn parse_pdf_page_ranges(spec: &str, total_pages: usize) -> Result<Vec<u32>> {
    let total_pages = u32::try_from(total_pages).expect("page count exceeds u32");
    let mut pages = BTreeSet::new();

    for part in spec.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            return Err(LiteCodeError::invalid_input(format!(
                "Invalid PDF page range {spec:?}."
            )));
        }

        let (start, end) = match trimmed.split_once('-') {
            Some((start, end)) => (
                parse_pdf_page_number(start, spec)?,
                parse_pdf_page_number(end, spec)?,
            ),
            None => {
                let page = parse_pdf_page_number(trimmed, spec)?;
                (page, page)
            }
        };

        if start > end {
            return Err(LiteCodeError::invalid_input(format!(
                "Invalid PDF page range {trimmed:?}: start must be less than or equal to end."
            )));
        }
        if end > total_pages {
            return Err(LiteCodeError::invalid_input(format!(
                "PDF page range {trimmed:?} exceeds the document page count of {total_pages}."
            )));
        }

        pages.extend(start..=end);
    }

    if pages.is_empty() {
        return Err(LiteCodeError::invalid_input(format!(
            "Invalid PDF page range {spec:?}."
        )));
    }

    Ok(pages.into_iter().collect())
}

fn parse_pdf_page_number(value: &str, spec: &str) -> Result<u32> {
    let page = value
        .trim()
        .parse::<u32>()
        .map_err(|_| LiteCodeError::invalid_input(format!("Invalid PDF page range {spec:?}.")))?;
    if page == 0 {
        return Err(LiteCodeError::invalid_input(format!(
            "Invalid PDF page range {spec:?}: page numbers start at 1."
        )));
    }
    Ok(page)
}

fn detect_image_mime_type(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?;
    if extension.eq_ignore_ascii_case("png") {
        Some("image/png")
    } else if extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg") {
        Some("image/jpeg")
    } else if extension.eq_ignore_ascii_case("gif") {
        Some("image/gif")
    } else if extension.eq_ignore_ascii_case("webp") {
        Some("image/webp")
    } else if extension.eq_ignore_ascii_case("bmp") {
        Some("image/bmp")
    } else {
        None
    }
}

fn window_file_content(content: &str, offset: usize, limit: usize) -> String {
    let lines = content.lines().collect::<Vec<_>>();
    let start = offset.min(lines.len());
    let end = (start + limit).min(lines.len());

    lines[start..end].join("\n")
}

fn run_grep_search_blocking(
    target: &Path,
    input: &GrepInput,
    before: usize,
    after: usize,
) -> Result<GrepSearchResult> {
    let matcher = build_regex_matcher(input)?;
    let overrides = build_override_matcher(target, input)?;
    let types = build_type_matcher(input)?;
    let search_paths = collect_search_paths(target, overrides.as_ref(), types.as_ref())?;
    let include_line_numbers =
        input.output_mode == GrepOutputMode::Content && input.line_numbers.unwrap_or(true);
    let collect_content = input.output_mode == GrepOutputMode::Content;
    let mut searcher = build_searcher(input, before, after, include_line_numbers);
    let mut results = GrepSearchResult::default();

    for path in search_paths {
        let rendered_path = render_search_path(target, &path);
        let mut sink = GrepSink::new(
            rendered_path,
            include_line_numbers,
            collect_content,
            &matcher,
            &mut results,
        );
        searcher
            .search_path(&matcher, &path, &mut sink)
            .map_err(|error| {
                LiteCodeError::internal(format!("Search failed for {}: {error}", path.display()))
            })?;
    }

    Ok(results)
}

fn build_regex_matcher(input: &GrepInput) -> Result<RegexMatcher> {
    let mut builder = RegexMatcherBuilder::new();
    builder.case_insensitive(input.case_insensitive.unwrap_or(false));
    builder.multi_line(input.multiline);
    builder.dot_matches_new_line(input.multiline);
    builder
        .build(&input.pattern)
        .map_err(|error| LiteCodeError::invalid_input(error.to_string()))
}

fn build_override_matcher(target: &Path, input: &GrepInput) -> Result<Option<Override>> {
    let Some(glob_pattern) = input.glob.as_deref() else {
        return Ok(None);
    };

    let root = override_root(target);
    let mut builder = OverrideBuilder::new(root);
    builder
        .add(glob_pattern)
        .map_err(|error| LiteCodeError::invalid_input(error.to_string()))?;
    builder
        .build()
        .map(Some)
        .map_err(|error| LiteCodeError::invalid_input(error.to_string()))
}

fn build_type_matcher(input: &GrepInput) -> Result<Option<Types>> {
    let Some(file_type) = input.file_type.as_deref() else {
        return Ok(None);
    };

    let mut builder = TypesBuilder::new();
    builder.add_defaults();
    builder.select(file_type);
    builder
        .build()
        .map(Some)
        .map_err(|error| LiteCodeError::invalid_input(error.to_string()))
}

fn collect_search_paths(
    target: &Path,
    overrides: Option<&Override>,
    types: Option<&Types>,
) -> Result<Vec<PathBuf>> {
    if target.is_file() {
        return Ok(if explicit_file_matches(target, overrides, types) {
            vec![target.to_path_buf()]
        } else {
            Vec::new()
        });
    }

    let mut builder = WalkBuilder::new(target);
    if let Some(overrides) = overrides {
        builder.overrides(overrides.clone());
    }
    if let Some(types) = types {
        builder.types(types.clone());
    }

    let mut paths = Vec::new();
    for entry in builder.build() {
        let entry = entry.map_err(|error| {
            LiteCodeError::internal(format!(
                "Failed to traverse search path {}: {error}",
                target.display()
            ))
        })?;
        if !entry
            .file_type()
            .map(|file_type| file_type.is_file())
            .unwrap_or(false)
        {
            continue;
        }
        paths.push(entry.into_path());
    }
    Ok(paths)
}

fn explicit_file_matches(path: &Path, overrides: Option<&Override>, types: Option<&Types>) -> bool {
    if let Some(overrides) = overrides
        && overrides.matched(path, false).is_ignore()
    {
        return false;
    }
    if let Some(types) = types
        && types.matched(path, false).is_ignore()
    {
        return false;
    }
    true
}

fn override_root(target: &Path) -> &Path {
    if target.is_dir() {
        target
    } else {
        target.parent().unwrap_or(target)
    }
}

fn build_searcher(
    input: &GrepInput,
    before: usize,
    after: usize,
    include_line_numbers: bool,
) -> Searcher {
    let mut builder = SearcherBuilder::new();
    builder.multi_line(input.multiline);
    builder.line_number(include_line_numbers);
    if input.output_mode == GrepOutputMode::Content {
        builder.before_context(before);
        builder.after_context(after);
    }
    builder.build()
}

fn render_search_path(target: &Path, path: &Path) -> String {
    if target.is_file() {
        path.display().to_string()
    } else {
        path.strip_prefix(target)
            .unwrap_or(path)
            .display()
            .to_string()
    }
}

fn to_usize_line_number(line_number: Option<u64>) -> Option<usize> {
    line_number.and_then(|value| usize::try_from(value).ok())
}

fn extend_rendered_lines(
    rendered: &mut Vec<GrepRenderedLine>,
    path: &str,
    text: &str,
    line_number: Option<usize>,
    include_line_numbers: bool,
) {
    let base_line_number = line_number.unwrap_or(1);
    let segments = text
        .split('\n')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    for (index, segment) in segments.into_iter().enumerate() {
        let text = if include_line_numbers {
            format!("{path}:{}:{segment}", base_line_number + index)
        } else {
            format!("{path}:{segment}")
        };
        rendered.push(GrepRenderedLine {
            path: path.to_string(),
            text,
        });
    }
}

fn apply_window<T: Clone>(entries: &[T], offset: usize, limit: usize) -> Vec<T> {
    let iter = entries.iter().skip(offset);
    if limit == 0 {
        iter.cloned().collect()
    } else {
        iter.take(limit).cloned().collect()
    }
}

impl GrepSearchResult {
    fn record_match(&mut self, path: &str, count: usize) {
        if !self.match_counts.contains_key(path) {
            self.matched_files.push(path.to_string());
            self.match_counts.insert(path.to_string(), 0);
        }
        if let Some(entry) = self.match_counts.get_mut(path) {
            *entry += count;
        }
        self.total_matches += count;
    }
}

struct GrepSink<'a, M> {
    path: String,
    include_line_numbers: bool,
    collect_content: bool,
    matcher: &'a M,
    results: &'a mut GrepSearchResult,
}

impl<'a, M> GrepSink<'a, M> {
    fn new(
        path: String,
        include_line_numbers: bool,
        collect_content: bool,
        matcher: &'a M,
        results: &'a mut GrepSearchResult,
    ) -> Self {
        Self {
            path,
            include_line_numbers,
            collect_content,
            matcher,
            results,
        }
    }
}

impl<M> Sink for GrepSink<'_, M>
where
    M: Matcher,
    M::Error: std::fmt::Display,
{
    type Error = io::Error;

    fn matched(
        &mut self,
        _searcher: &Searcher,
        mat: &SinkMatch<'_>,
    ) -> std::result::Result<bool, Self::Error> {
        let mut match_count = 0usize;
        self.matcher
            .find_iter(mat.bytes(), |_| {
                match_count += 1;
                true
            })
            .map_err(|error| io::Error::other(error.to_string()))?;
        self.results.record_match(&self.path, match_count.max(1));

        if self.collect_content {
            let text = String::from_utf8_lossy(mat.bytes());
            extend_rendered_lines(
                &mut self.results.content_lines,
                &self.path,
                &text,
                to_usize_line_number(mat.line_number()),
                self.include_line_numbers,
            );
        }
        Ok(true)
    }

    fn context(
        &mut self,
        _searcher: &Searcher,
        context: &SinkContext<'_>,
    ) -> std::result::Result<bool, Self::Error> {
        if self.collect_content {
            let text = String::from_utf8_lossy(context.bytes());
            extend_rendered_lines(
                &mut self.results.content_lines,
                &self.path,
                &text,
                to_usize_line_number(context.line_number()),
                self.include_line_numbers,
            );
        }
        Ok(true)
    }
}

fn value_as_lines(value: Option<&serde_json::Value>) -> Vec<String> {
    match value {
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .map(|item| item.as_str().unwrap_or_default().to_string())
            .collect(),
        Some(serde_json::Value::String(item)) => vec![item.clone()],
        _ => Vec::new(),
    }
}

fn notebook_language(notebook: &serde_json::Value) -> String {
    notebook
        .get("metadata")
        .and_then(|value| value.get("language_info"))
        .and_then(|value| value.get("name"))
        .and_then(|value| value.as_str())
        .or_else(|| {
            notebook
                .get("metadata")
                .and_then(|value| value.get("kernelspec"))
                .and_then(|value| value.get("language"))
                .and_then(|value| value.as_str())
        })
        .unwrap_or("unknown")
        .to_string()
}

fn notebook_cell_type(
    cell: &serde_json::Map<String, serde_json::Value>,
) -> Option<NotebookCellType> {
    match cell.get("cell_type").and_then(|value| value.as_str()) {
        Some("markdown") => Some(NotebookCellType::Markdown),
        Some("code") => Some(NotebookCellType::Code),
        _ => None,
    }
}

fn cell_type_string(cell_type: NotebookCellType) -> String {
    match cell_type {
        NotebookCellType::Code => "code".to_string(),
        NotebookCellType::Markdown => "markdown".to_string(),
    }
}

fn source_value(source: &str) -> serde_json::Value {
    serde_json::Value::Array(
        source
            .split_inclusive('\n')
            .map(|line| serde_json::Value::String(line.to_string()))
            .collect(),
    )
}

fn generated_cell_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("cell-{nanos}")
}

fn new_notebook_cell(
    cell_id: &str,
    cell_type: NotebookCellType,
    source: &str,
) -> serde_json::Value {
    let mut cell = serde_json::Map::new();
    cell.insert(
        "id".to_string(),
        serde_json::Value::String(cell_id.to_string()),
    );
    cell.insert(
        "cell_type".to_string(),
        serde_json::Value::String(cell_type_string(cell_type)),
    );
    cell.insert("metadata".to_string(), serde_json::json!({}));
    cell.insert("source".to_string(), source_value(source));
    normalize_cell_shape(&mut cell, cell_type);
    serde_json::Value::Object(cell)
}

fn normalize_cell_shape(
    cell: &mut serde_json::Map<String, serde_json::Value>,
    cell_type: NotebookCellType,
) {
    match cell_type {
        NotebookCellType::Code => {
            cell.entry("execution_count".to_string())
                .or_insert(serde_json::Value::Null);
            cell.entry("outputs".to_string())
                .or_insert_with(|| serde_json::Value::Array(Vec::new()));
        }
        NotebookCellType::Markdown => {
            cell.remove("execution_count");
            cell.remove("outputs");
        }
    }
}

fn find_cell_index(cells: &[serde_json::Value], cell_id: &str) -> Result<usize> {
    cells
        .iter()
        .position(|cell| cell.get("id").and_then(|value| value.as_str()) == Some(cell_id))
        .ok_or_else(|| {
            LiteCodeError::invalid_input(format!("Notebook cell {cell_id} was not found."))
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedNotebookCell {
    index: usize,
    cell_id: Option<String>,
}

fn resolve_notebook_cell(
    cells: &[serde_json::Value],
    cell_id: Option<&str>,
    cell_number: Option<usize>,
    operation: &str,
) -> Result<ResolvedNotebookCell> {
    let resolved_from_id = match cell_id {
        Some(cell_id) => Some((find_cell_index(cells, cell_id)?, cell_id.to_string())),
        None => None,
    };
    let resolved_from_number = match cell_number {
        Some(index) => Some((index, resolve_numbered_cell(cells, index, operation)?)),
        None => None,
    };

    match (resolved_from_id, resolved_from_number) {
        (Some((id_index, id)), Some((number_index, number_cell_id))) => {
            if id_index != number_index || number_cell_id.as_deref() != Some(id.as_str()) {
                return Err(LiteCodeError::invalid_input(format!(
                    "NotebookEdit {operation} mode received conflicting cell_id and cell_number."
                )));
            }

            Ok(ResolvedNotebookCell {
                index: id_index,
                cell_id: Some(id),
            })
        }
        (Some((index, id)), None) => Ok(ResolvedNotebookCell {
            index,
            cell_id: Some(id),
        }),
        (None, Some((index, cell_id))) => Ok(ResolvedNotebookCell { index, cell_id }),
        (None, None) => Err(LiteCodeError::invalid_input(format!(
            "NotebookEdit {operation} mode requires cell_id or cell_number."
        ))),
    }
}

fn resolve_numbered_cell(
    cells: &[serde_json::Value],
    index: usize,
    operation: &str,
) -> Result<Option<String>> {
    let max_index = cells.len().saturating_sub(1);
    if index >= cells.len() {
        return Err(LiteCodeError::invalid_input(format!(
            "NotebookEdit {operation} mode requires cell_number to be within 0..{}.",
            max_index
        )));
    }

    Ok(cells
        .get(index)
        .and_then(|cell| cell.get("id"))
        .and_then(|value| value.as_str())
        .map(ToString::to_string))
}

fn resolve_insert_index(
    cells: &[serde_json::Value],
    cell_id: Option<&str>,
    cell_number: Option<usize>,
) -> Result<usize> {
    let index_from_id = match cell_id {
        Some(existing_id) => Some(find_cell_index(cells, existing_id)? + 1),
        None => None,
    };
    let index_from_number = match cell_number {
        Some(index) => {
            if index > cells.len() {
                return Err(LiteCodeError::invalid_input(format!(
                    "NotebookEdit insert mode requires cell_number to be within 0..{}.",
                    cells.len()
                )));
            }
            Some(index)
        }
        None => None,
    };

    match (index_from_id, index_from_number) {
        (Some(id_index), Some(number_index)) => {
            if id_index != number_index {
                return Err(LiteCodeError::invalid_input(
                    "NotebookEdit insert mode received conflicting cell_id and cell_number.",
                ));
            }
            Ok(id_index)
        }
        (Some(index), None) | (None, Some(index)) => Ok(index),
        (None, None) => Ok(0),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GrepRenderedLine {
    path: String,
    text: String,
}

#[derive(Debug, Default)]
struct GrepSearchResult {
    matched_files: Vec<String>,
    match_counts: HashMap<String, usize>,
    content_lines: Vec<GrepRenderedLine>,
    total_matches: usize,
}

#[cfg(test)]
mod tests {
    use std::{
        convert::TryFrom,
        path::{Path, PathBuf},
    };

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use lopdf::{
        Document, Object, Stream,
        content::{Content, Operation},
        dictionary,
    };
    use tempfile::tempdir;

    use crate::schema::{EditInput, GlobInput, GrepInput, GrepOutputMode, WriteInput};
    use crate::schema::{NotebookCellType, NotebookEditInput, NotebookEditMode};

    use super::{FileService, ReadFileOutput};

    #[tokio::test]
    async fn read_returns_plain_content_slice() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sample.txt");
        tokio::fs::write(&file, "alpha\nbeta\ngamma\n")
            .await
            .unwrap();

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(
            dir.path().to_path_buf(),
        )));
        let content = service
            .read_file(&file, Some(1), Some(1), None)
            .await
            .unwrap();

        assert_eq!(content, ReadFileOutput::Text("beta".to_string()));
    }

    #[tokio::test]
    async fn read_returns_image_output_for_supported_extensions() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sample.png");
        let png_bytes = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
        tokio::fs::write(&file, &png_bytes).await.unwrap();

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(
            dir.path().to_path_buf(),
        )));
        let content = service.read_file(&file, None, None, None).await.unwrap();

        assert_eq!(
            content,
            ReadFileOutput::Image {
                data: png_bytes,
                mime_type: "image/png".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn read_extracts_text_from_selected_pdf_pages() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sample.pdf");
        write_test_pdf(&file, &["alpha page", "beta page", "gamma page"]);

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(
            dir.path().to_path_buf(),
        )));
        let content = service
            .read_file(&file, None, None, Some("2-3"))
            .await
            .unwrap();

        let ReadFileOutput::Text(text) = content else {
            panic!("expected text output");
        };
        assert!(text.contains("Page 2"));
        assert!(text.contains("beta page"));
        assert!(text.contains("Page 3"));
        assert!(text.contains("gamma page"));
        assert!(!text.contains("Page 1"));
    }

    #[tokio::test]
    async fn read_requires_pages_for_large_pdfs() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("large.pdf");
        let pages = (1..=11)
            .map(|index| format!("page {index}"))
            .collect::<Vec<_>>();
        let page_refs = pages.iter().map(String::as_str).collect::<Vec<_>>();
        write_test_pdf(&file, &page_refs);

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(
            dir.path().to_path_buf(),
        )));
        let error = service
            .read_file(&file, None, None, None)
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("must provide the pages parameter")
        );
    }

    #[tokio::test]
    async fn read_formats_notebooks_like_structured_cells() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("note.ipynb");
        let notebook = serde_json::json!({
            "cells": [
                {
                    "id": "cell-a",
                    "cell_type": "markdown",
                    "metadata": {},
                    "source": ["# Heading\n", "\n", "Body text.\n"]
                },
                {
                    "id": "cell-b",
                    "cell_type": "code",
                    "metadata": {},
                    "execution_count": 1,
                    "outputs": [
                        {
                            "output_type": "stream",
                            "name": "stdout",
                            "text": ["hello\n"]
                        }
                    ],
                    "source": ["print('hello')\n"]
                }
            ],
            "metadata": {
                "language_info": { "name": "python" }
            },
            "nbformat": 4,
            "nbformat_minor": 5
        });
        tokio::fs::write(&file, serde_json::to_string_pretty(&notebook).unwrap())
            .await
            .unwrap();

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(
            dir.path().to_path_buf(),
        )));
        let content = service.read_file(&file, None, None, None).await.unwrap();

        let ReadFileOutput::Text(text) = content else {
            panic!("expected text output");
        };
        assert!(text.contains("<cell number=\"0\" id=\"cell-a\">"));
        assert!(text.contains("<cell_type>markdown</cell_type>"));
        assert!(text.contains("<source>"));
        assert!(text.contains("# Heading"));
        assert!(text.contains("<cell number=\"1\" id=\"cell-b\">"));
        assert!(text.contains("<language>python</language>"));
        assert!(text.contains("<outputs>"));
        assert!(text.contains("<output_type>stream</output_type>"));
        assert!(text.contains("<text>"));
        assert!(!text.contains("Cell 0 [markdown]"));
    }

    #[tokio::test]
    async fn read_truncates_long_notebook_output_text_without_truncating_source() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("truncated.ipynb");
        let source = format!("print({:?})\n", "source stays whole".repeat(500));
        let long_output = "output block ".repeat(800);
        let notebook = serde_json::json!({
            "cells": [
                {
                    "id": "cell-a",
                    "cell_type": "code",
                    "metadata": {},
                    "execution_count": 1,
                    "outputs": [
                        {
                            "output_type": "stream",
                            "name": "stdout",
                            "text": [long_output]
                        }
                    ],
                    "source": [source]
                }
            ],
            "metadata": {
                "language_info": { "name": "python" }
            },
            "nbformat": 4,
            "nbformat_minor": 5
        });
        tokio::fs::write(&file, serde_json::to_string_pretty(&notebook).unwrap())
            .await
            .unwrap();

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(
            dir.path().to_path_buf(),
        )));
        let content = service.read_file(&file, None, None, None).await.unwrap();

        let ReadFileOutput::Text(text) = content else {
            panic!("expected text output");
        };
        assert!(text.contains("source stays whole"));
        assert!(text.contains("output truncated for readability"));
        assert!(text.contains("Use Bash with cat and jq"));
        assert!(!text.contains(&"output block ".repeat(700)));
    }

    #[tokio::test]
    async fn read_returns_embedded_notebook_images_as_image_content() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("image-output.ipynb");
        let png_bytes = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
        let notebook = serde_json::json!({
            "cells": [
                {
                    "id": "cell-a",
                    "cell_type": "code",
                    "metadata": {},
                    "execution_count": 1,
                    "outputs": [
                        {
                            "output_type": "display_data",
                            "data": {
                                "image/png": STANDARD.encode(&png_bytes),
                                "text/plain": ["figure preview\n"]
                            },
                            "metadata": {}
                        }
                    ],
                    "source": ["display(fig)\n"]
                }
            ],
            "metadata": {
                "language_info": { "name": "python" }
            },
            "nbformat": 4,
            "nbformat_minor": 5
        });
        tokio::fs::write(&file, serde_json::to_string_pretty(&notebook).unwrap())
            .await
            .unwrap();

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(
            dir.path().to_path_buf(),
        )));
        let content = service.read_file(&file, None, None, None).await.unwrap();

        let ReadFileOutput::Contents(parts) = content else {
            panic!("expected mixed notebook content");
        };
        assert!(parts.iter().any(|part| matches!(
            part,
            super::ReadContent::Image { data, mime_type }
                if data == &png_bytes && mime_type == "image/png"
        )));

        let joined_text = parts
            .iter()
            .filter_map(|part| match part {
                super::ReadContent::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<String>();
        assert!(joined_text.contains("[image/png output rendered below]"));
        assert!(joined_text.contains("figure preview"));
        assert!(!joined_text.contains(&STANDARD.encode(&png_bytes)));
    }

    #[tokio::test]
    async fn read_rejects_pdf_requests_larger_than_twenty_pages() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("too-many-pages.pdf");
        let pages = (1..=21)
            .map(|index| format!("page {index}"))
            .collect::<Vec<_>>();
        let page_refs = pages.iter().map(String::as_str).collect::<Vec<_>>();
        write_test_pdf(&file, &page_refs);

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(
            dir.path().to_path_buf(),
        )));
        let error = service
            .read_file(&file, None, None, Some("1-21"))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("20-page maximum"));
    }

    #[tokio::test]
    async fn write_existing_file_requires_read() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sample.txt");
        tokio::fs::write(&file, "before").await.unwrap();

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(
            dir.path().to_path_buf(),
        )));
        let error = service
            .write_file(WriteInput {
                file_path: file.display().to_string(),
                content: "after".to_string(),
                decode_unicode_escapes: false,
            })
            .await
            .unwrap_err();

        assert!(error.to_string().contains("prior Read"));
    }

    #[tokio::test]
    async fn edit_replaces_unique_string() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sample.txt");
        tokio::fs::write(&file, "before after").await.unwrap();

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(
            dir.path().to_path_buf(),
        )));
        service.read_file(&file, None, None, None).await.unwrap();
        let output = service
            .edit_file(EditInput {
                file_path: file.display().to_string(),
                old_string: "before".to_string(),
                new_string: "during".to_string(),
                replace_all: false,
                decode_unicode_escapes: false,
            })
            .await
            .unwrap();

        assert!(output.success);
        assert_eq!(
            output.message,
            format!("Edited {} (replaced 1 location).", file.display())
        );
        assert_eq!(
            tokio::fs::read_to_string(&file).await.unwrap(),
            "during after"
        );

        let output_json = serde_json::to_value(&output).unwrap();
        assert_eq!(output_json.as_object().unwrap().len(), 2);
        assert_eq!(
            output_json.get("success"),
            Some(&serde_json::Value::Bool(true))
        );
        assert_eq!(
            output_json.get("message"),
            Some(&serde_json::Value::String(output.message.clone()))
        );
    }

    #[tokio::test]
    async fn write_reports_created_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sample.txt");

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(
            dir.path().to_path_buf(),
        )));
        let output = service
            .write_file(WriteInput {
                file_path: file.display().to_string(),
                content: "after".to_string(),
                decode_unicode_escapes: false,
            })
            .await
            .unwrap();

        assert!(output.success);
        assert_eq!(
            output.message,
            format!("Wrote {} (created new file, 5 bytes).", file.display())
        );
        assert_eq!(tokio::fs::read_to_string(&file).await.unwrap(), "after");

        let output_json = serde_json::to_value(&output).unwrap();
        assert_eq!(output_json.as_object().unwrap().len(), 2);
        assert_eq!(
            output_json.get("success"),
            Some(&serde_json::Value::Bool(true))
        );
        assert_eq!(
            output_json.get("message"),
            Some(&serde_json::Value::String(output.message.clone()))
        );
    }

    #[tokio::test]
    async fn edit_reports_all_replacements() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sample.txt");
        tokio::fs::write(&file, "before and before again")
            .await
            .unwrap();

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(
            dir.path().to_path_buf(),
        )));
        service.read_file(&file, None, None, None).await.unwrap();
        let output = service
            .edit_file(EditInput {
                file_path: file.display().to_string(),
                old_string: "before".to_string(),
                new_string: "after".to_string(),
                replace_all: true,
                decode_unicode_escapes: false,
            })
            .await
            .unwrap();

        assert!(output.success);
        assert_eq!(
            output.message,
            format!("Edited {} (replaced 2 locations).", file.display())
        );
        assert_eq!(
            tokio::fs::read_to_string(&file).await.unwrap(),
            "after and after again"
        );
    }

    #[tokio::test]
    async fn write_reports_overwritten_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sample.txt");
        tokio::fs::write(&file, "before").await.unwrap();

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(
            dir.path().to_path_buf(),
        )));
        service.read_file(&file, None, None, None).await.unwrap();
        let output = service
            .write_file(WriteInput {
                file_path: file.display().to_string(),
                content: "after!".to_string(),
                decode_unicode_escapes: false,
            })
            .await
            .unwrap();

        assert!(output.success);
        assert_eq!(
            output.message,
            format!(
                "Wrote {} (overwrote existing file, 6 bytes).",
                file.display()
            )
        );
        assert_eq!(tokio::fs::read_to_string(&file).await.unwrap(), "after!");
    }

    #[tokio::test]
    async fn write_decodes_unicode_escapes_when_enabled() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sample.txt");

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(
            dir.path().to_path_buf(),
        )));
        service
            .write_file(WriteInput {
                file_path: file.display().to_string(),
                content: "\\u4F60\\u597D \\uD83D\\uDE00".to_string(),
                decode_unicode_escapes: true,
            })
            .await
            .unwrap();

        assert_eq!(tokio::fs::read_to_string(&file).await.unwrap(), "你好 😀");
    }

    #[tokio::test]
    async fn edit_decodes_unicode_escapes_when_enabled() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sample.txt");
        tokio::fs::write(&file, "你好").await.unwrap();

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(
            dir.path().to_path_buf(),
        )));
        service.read_file(&file, None, None, None).await.unwrap();
        service
            .edit_file(EditInput {
                file_path: file.display().to_string(),
                old_string: "\\u4F60\\u597D".to_string(),
                new_string: "\\u4E16\\u754C".to_string(),
                replace_all: false,
                decode_unicode_escapes: true,
            })
            .await
            .unwrap();

        assert_eq!(tokio::fs::read_to_string(&file).await.unwrap(), "世界");
    }

    #[tokio::test]
    async fn write_rejects_invalid_unicode_escape_when_enabled() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sample.txt");

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(
            dir.path().to_path_buf(),
        )));
        let error = service
            .write_file(WriteInput {
                file_path: file.display().to_string(),
                content: "\\u12ZZ".to_string(),
                decode_unicode_escapes: true,
            })
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Invalid Unicode escape in content")
        );
    }

    #[test]
    fn glob_returns_matching_files() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.path().join("README.md"), "# hi").unwrap();

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(
            dir.path().to_path_buf(),
        )));
        let output = service
            .glob_files(GlobInput {
                pattern: "**/*.rs".to_string(),
                path: None,
            })
            .unwrap();

        assert_eq!(output.num_files, 1);
        assert!(output.filenames[0].ends_with("lib.rs"));

        let output_json = serde_json::to_value(&output).unwrap();
        let keys = output_json
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            keys,
            ["filenames", "numFiles"]
                .into_iter()
                .map(str::to_string)
                .collect()
        );
    }

    #[tokio::test]
    async fn grep_returns_matching_content() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("main.rs");
        tokio::fs::write(&file, "fn main() {}\nlet needle = 1;\n")
            .await
            .unwrap();

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(PathBuf::from(
            dir.path(),
        ))));
        let output = service
            .grep_files(GrepInput {
                pattern: "needle".to_string(),
                path: None,
                glob: None,
                output_mode: GrepOutputMode::Content,
                before: None,
                after: None,
                context_alias: None,
                context: None,
                line_numbers: Some(true),
                case_insensitive: None,
                file_type: Some("rust".to_string()),
                head_limit: None,
                offset: None,
                multiline: false,
            })
            .await
            .unwrap();

        assert_eq!(output.num_files, 1);
        assert!(
            output
                .content
                .as_deref()
                .unwrap()
                .contains("main.rs:2:let needle = 1;")
        );
        let output_json = serde_json::to_value(&output).unwrap();
        let keys = output_json
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            keys,
            ["content", "filenames", "numFiles", "numMatches"]
                .into_iter()
                .map(str::to_string)
                .collect()
        );
    }

    #[tokio::test]
    async fn grep_supports_single_file_paths() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("main.rs");
        let other = dir.path().join("other.rs");
        tokio::fs::write(&file, "let needle = 1;\n").await.unwrap();
        tokio::fs::write(&other, "let needle = 2;\n").await.unwrap();

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(PathBuf::from(
            dir.path(),
        ))));
        let output = service
            .grep_files(GrepInput {
                pattern: "needle".to_string(),
                path: Some(file.display().to_string()),
                glob: None,
                output_mode: GrepOutputMode::FilesWithMatches,
                before: None,
                after: None,
                context_alias: None,
                context: None,
                line_numbers: None,
                case_insensitive: None,
                file_type: None,
                head_limit: None,
                offset: None,
                multiline: false,
            })
            .await
            .unwrap();

        assert_eq!(output.num_files, 1);
        assert_eq!(output.filenames, vec![file.display().to_string()]);
    }

    #[tokio::test]
    async fn grep_filters_matches_with_glob_patterns() {
        let dir = tempdir().unwrap();
        let rust_file = dir.path().join("main.rs");
        let text_file = dir.path().join("notes.txt");
        tokio::fs::write(&rust_file, "needle in rust\n")
            .await
            .unwrap();
        tokio::fs::write(&text_file, "needle in text\n")
            .await
            .unwrap();

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(PathBuf::from(
            dir.path(),
        ))));
        let output = service
            .grep_files(GrepInput {
                pattern: "needle".to_string(),
                path: None,
                glob: Some("*.txt".to_string()),
                output_mode: GrepOutputMode::FilesWithMatches,
                before: None,
                after: None,
                context_alias: None,
                context: None,
                line_numbers: None,
                case_insensitive: None,
                file_type: None,
                head_limit: None,
                offset: None,
                multiline: false,
            })
            .await
            .unwrap();

        assert_eq!(output.num_files, 1);
        assert_eq!(output.filenames, vec!["notes.txt".to_string()]);
    }

    #[tokio::test]
    async fn grep_counts_individual_matches_with_ripgrep() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("main.rs");
        tokio::fs::write(&file, "needle needle\nneedle\n")
            .await
            .unwrap();

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(PathBuf::from(
            dir.path(),
        ))));
        let output = service
            .grep_files(GrepInput {
                pattern: "needle".to_string(),
                path: None,
                glob: None,
                output_mode: GrepOutputMode::Count,
                before: None,
                after: None,
                context_alias: None,
                context: None,
                line_numbers: None,
                case_insensitive: None,
                file_type: Some("rust".to_string()),
                head_limit: None,
                offset: None,
                multiline: false,
            })
            .await
            .unwrap();

        assert_eq!(output.num_matches, Some(3));
        assert!(output.content.unwrap().contains("main.rs:3"));
    }

    #[tokio::test]
    async fn grep_supports_ripgrep_multiline_searches() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("main.rs");
        tokio::fs::write(&file, "fn start() {\nfoo\nbar\n}\n")
            .await
            .unwrap();

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(PathBuf::from(
            dir.path(),
        ))));
        let output = service
            .grep_files(GrepInput {
                pattern: "foo\\nbar".to_string(),
                path: None,
                glob: None,
                output_mode: GrepOutputMode::Content,
                before: None,
                after: None,
                context_alias: None,
                context: None,
                line_numbers: Some(true),
                case_insensitive: None,
                file_type: Some("rust".to_string()),
                head_limit: None,
                offset: None,
                multiline: true,
            })
            .await
            .unwrap();

        let content = output.content.unwrap();
        assert!(content.contains("main.rs:2:foo"));
        assert!(content.contains("main.rs:3:bar"));
        assert_eq!(output.num_matches, Some(1));
    }

    #[tokio::test]
    async fn notebook_replace_insert_delete_work() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("note.ipynb");
        let original = serde_json::json!({
            "cells": [
                {
                    "id": "cell-a",
                    "cell_type": "code",
                    "metadata": {},
                    "execution_count": null,
                    "outputs": [],
                    "source": ["print('hi')\n"]
                }
            ],
            "metadata": {
                "language_info": { "name": "python" }
            },
            "nbformat": 4,
            "nbformat_minor": 5
        });
        tokio::fs::write(&file, serde_json::to_string_pretty(&original).unwrap())
            .await
            .unwrap();

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(
            dir.path().to_path_buf(),
        )));

        let replaced = service
            .edit_notebook(NotebookEditInput {
                notebook_path: file.display().to_string(),
                cell_id: Some("cell-a".to_string()),
                cell_number: None,
                new_source: "print('bye')\n".to_string(),
                cell_type: None,
                edit_mode: NotebookEditMode::Replace,
            })
            .await
            .unwrap();
        assert!(replaced.success);
        let replaced_json = serde_json::to_value(&replaced).unwrap();
        assert_eq!(replaced_json.as_object().unwrap().len(), 1);
        assert_eq!(
            replaced_json.get("success"),
            Some(&serde_json::Value::Bool(true))
        );
        assert!(replaced_json.get("cellId").is_none());

        let inserted = service
            .edit_notebook(NotebookEditInput {
                notebook_path: file.display().to_string(),
                cell_id: Some("cell-a".to_string()),
                cell_number: None,
                new_source: "# title\n".to_string(),
                cell_type: Some(NotebookCellType::Markdown),
                edit_mode: NotebookEditMode::Insert,
            })
            .await
            .unwrap();
        assert!(inserted.success);
        assert!(inserted.cell_id.is_some());
        let inserted_json = serde_json::to_value(&inserted).unwrap();
        assert_eq!(inserted_json.as_object().unwrap().len(), 2);
        assert_eq!(
            inserted_json.get("success"),
            Some(&serde_json::Value::Bool(true))
        );
        let inserted_id = inserted
            .cell_id
            .clone()
            .expect("insert should return inserted cell id");
        assert_eq!(
            inserted_json.get("cellId"),
            Some(&serde_json::Value::String(inserted_id.clone()))
        );

        let notebook_after_insert = serde_json::from_str::<serde_json::Value>(
            &tokio::fs::read_to_string(&file).await.unwrap(),
        )
        .unwrap();
        let notebook_inserted_id = notebook_after_insert
            .get("cells")
            .and_then(|value| value.as_array())
            .and_then(|cells| {
                cells.iter().find(|cell| {
                    cell.get("source")
                        .and_then(|value| value.as_array())
                        .and_then(|source| source.first())
                        .and_then(|value| value.as_str())
                        == Some("# title\n")
                })
            })
            .and_then(|cell| cell.get("id"))
            .and_then(|value| value.as_str())
            .unwrap()
            .to_string();
        assert_eq!(notebook_inserted_id, inserted_id);

        let deleted = service
            .edit_notebook(NotebookEditInput {
                notebook_path: file.display().to_string(),
                cell_id: Some(inserted_id),
                cell_number: None,
                new_source: String::new(),
                cell_type: None,
                edit_mode: NotebookEditMode::Delete,
            })
            .await
            .unwrap();
        assert!(deleted.success);
        let deleted_json = serde_json::to_value(&deleted).unwrap();
        assert_eq!(deleted_json.as_object().unwrap().len(), 1);
        assert_eq!(
            deleted_json.get("success"),
            Some(&serde_json::Value::Bool(true))
        );
        assert!(deleted_json.get("cellId").is_none());

        let notebook = serde_json::from_str::<serde_json::Value>(
            &tokio::fs::read_to_string(&file).await.unwrap(),
        )
        .unwrap();
        let cells = notebook
            .get("cells")
            .and_then(|value| value.as_array())
            .unwrap();
        assert_eq!(cells.len(), 1);
        assert_eq!(
            cells[0]
                .get("source")
                .and_then(|value| value.as_array())
                .unwrap()[0]
                .as_str()
                .unwrap(),
            "print('bye')\n"
        );
    }

    #[tokio::test]
    async fn notebook_edit_supports_cell_number_operations() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("note.ipynb");
        let original = serde_json::json!({
            "cells": [
                {
                    "id": "cell-a",
                    "cell_type": "markdown",
                    "metadata": {},
                    "source": ["# heading\n"]
                },
                {
                    "id": "cell-b",
                    "cell_type": "code",
                    "metadata": {},
                    "execution_count": null,
                    "outputs": [],
                    "source": ["print('hi')\n"]
                }
            ],
            "metadata": {
                "language_info": { "name": "python" }
            },
            "nbformat": 4,
            "nbformat_minor": 5
        });
        tokio::fs::write(&file, serde_json::to_string_pretty(&original).unwrap())
            .await
            .unwrap();

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(
            dir.path().to_path_buf(),
        )));

        let replaced = service
            .edit_notebook(NotebookEditInput {
                notebook_path: file.display().to_string(),
                cell_id: None,
                cell_number: Some(1),
                new_source: "print('bye')\n".to_string(),
                cell_type: None,
                edit_mode: NotebookEditMode::Replace,
            })
            .await
            .unwrap();
        assert!(replaced.success);
        assert!(replaced.cell_id.is_none());

        let inserted = service
            .edit_notebook(NotebookEditInput {
                notebook_path: file.display().to_string(),
                cell_id: None,
                cell_number: Some(1),
                new_source: "## middle\n".to_string(),
                cell_type: Some(NotebookCellType::Markdown),
                edit_mode: NotebookEditMode::Insert,
            })
            .await
            .unwrap();
        assert!(inserted.success);
        assert!(inserted.cell_id.is_some());

        let deleted = service
            .edit_notebook(NotebookEditInput {
                notebook_path: file.display().to_string(),
                cell_id: None,
                cell_number: Some(0),
                new_source: String::new(),
                cell_type: None,
                edit_mode: NotebookEditMode::Delete,
            })
            .await
            .unwrap();
        assert!(deleted.success);
        assert!(deleted.cell_id.is_none());

        let notebook = serde_json::from_str::<serde_json::Value>(
            &tokio::fs::read_to_string(&file).await.unwrap(),
        )
        .unwrap();
        let cells = notebook
            .get("cells")
            .and_then(|value| value.as_array())
            .unwrap();
        assert_eq!(cells.len(), 2);
        assert_eq!(
            cells[0]
                .get("source")
                .and_then(|value| value.as_array())
                .unwrap()[0]
                .as_str()
                .unwrap(),
            "## middle\n"
        );
        assert_eq!(
            cells[1]
                .get("source")
                .and_then(|value| value.as_array())
                .unwrap()[0]
                .as_str()
                .unwrap(),
            "print('bye')\n"
        );
    }

    #[tokio::test]
    async fn notebook_edit_rejects_conflicting_cell_selectors() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("note.ipynb");
        let original = serde_json::json!({
            "cells": [
                {
                    "id": "cell-a",
                    "cell_type": "markdown",
                    "metadata": {},
                    "source": ["# heading\n"]
                },
                {
                    "id": "cell-b",
                    "cell_type": "code",
                    "metadata": {},
                    "execution_count": null,
                    "outputs": [],
                    "source": ["print('hi')\n"]
                }
            ],
            "metadata": {
                "language_info": { "name": "python" }
            },
            "nbformat": 4,
            "nbformat_minor": 5
        });
        tokio::fs::write(&file, serde_json::to_string_pretty(&original).unwrap())
            .await
            .unwrap();

        let service = FileService::new(std::sync::Arc::new(std::sync::Mutex::new(
            dir.path().to_path_buf(),
        )));

        let replace_error = service
            .edit_notebook(NotebookEditInput {
                notebook_path: file.display().to_string(),
                cell_id: Some("cell-b".to_string()),
                cell_number: Some(0),
                new_source: "print('bye')\n".to_string(),
                cell_type: None,
                edit_mode: NotebookEditMode::Replace,
            })
            .await
            .unwrap_err();
        assert!(
            replace_error
                .to_string()
                .contains("conflicting cell_id and cell_number")
        );

        let insert_error = service
            .edit_notebook(NotebookEditInput {
                notebook_path: file.display().to_string(),
                cell_id: Some("cell-b".to_string()),
                cell_number: Some(0),
                new_source: "## before\n".to_string(),
                cell_type: Some(NotebookCellType::Markdown),
                edit_mode: NotebookEditMode::Insert,
            })
            .await
            .unwrap_err();
        assert!(
            insert_error
                .to_string()
                .contains("conflicting cell_id and cell_number")
        );
    }

    fn write_test_pdf(path: &Path, page_texts: &[&str]) {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Courier",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! {
                "F1" => font_id,
            },
        });

        let mut page_ids = Vec::new();
        for text in page_texts {
            let content = Content {
                operations: vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["F1".into(), 18.into()]),
                    Operation::new("Td", vec![72.into(), 720.into()]),
                    Operation::new("Tj", vec![Object::string_literal(*text)]),
                    Operation::new("ET", vec![]),
                ],
            };
            let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
            let page_id = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
            });
            page_ids.push(page_id);
        }

        let kids = page_ids.iter().copied().map(Into::into).collect::<Vec<_>>();
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => i64::try_from(page_ids.len()).unwrap(),
                "Resources" => resources_id,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            }),
        );

        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        doc.compress();
        doc.save(path).unwrap();
    }
}
