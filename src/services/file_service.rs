use std::{
    collections::{BTreeSet, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use globwalk::GlobWalkerBuilder;
use lopdf::Document;
use tokio::process::Command;

use crate::{
    error::{LiteCodeError, Result},
    schema::{
        EditInput, EditOutput, GlobInput, GlobOutput, GrepInput, GrepOutput, GrepOutputMode,
        NotebookCellType, NotebookEditInput, NotebookEditMode, NotebookEditOutput,
        StructuredPatchHunk, WriteInput, WriteOutput, WriteResultType,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadFileOutput {
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

            return Ok(ReadFileOutput::Text(numbered_lines(
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

        let text = if path.extension().and_then(|value| value.to_str()) == Some("ipynb") {
            render_notebook(&String::from_utf8_lossy(&raw))?
        } else {
            String::from_utf8_lossy(&raw).into_owned()
        };

        self.mark_read(&path);

        if text.is_empty() {
            return Ok(ReadFileOutput::Text(
                "Warning: file exists but is empty.".to_string(),
            ));
        }

        Ok(ReadFileOutput::Text(numbered_lines(
            &text,
            offset.unwrap_or(0),
            limit.unwrap_or(2_000),
        )))
    }

    pub async fn write_file(&self, input: WriteInput) -> Result<WriteOutput> {
        let path = self.require_absolute_file(Path::new(&input.file_path))?;
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

        tokio::fs::write(&path, &input.content).await?;
        self.mark_read(&path);

        let structured_patch =
            build_structured_patch(original.as_deref().unwrap_or(""), &input.content);
        Ok(WriteOutput {
            r#type: if original.is_some() {
                WriteResultType::Update
            } else {
                WriteResultType::Create
            },
            file_path: path.display().to_string(),
            content: input.content,
            structured_patch: structured_patch,
            original_file: original,
            git_diff: None,
        })
    }

    pub async fn edit_file(&self, input: EditInput) -> Result<EditOutput> {
        if input.old_string == input.new_string {
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
        let occurrences = original.match_indices(&input.old_string).count();
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
            original.replace(&input.old_string, &input.new_string)
        } else {
            original.replacen(&input.old_string, &input.new_string, 1)
        };

        tokio::fs::write(&path, &updated).await?;
        self.mark_read(&path);

        Ok(EditOutput {
            file_path: path.display().to_string(),
            old_string: input.old_string,
            new_string: input.new_string,
            original_file: original.clone(),
            structured_patch: build_structured_patch(&original, &updated),
            user_modified: false,
            replace_all: input.replace_all,
            git_diff: None,
        })
    }

    pub fn glob_files(&self, input: GlobInput) -> Result<GlobOutput> {
        let started_at = Instant::now();
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
        let truncated = total > 100;
        let filenames = files
            .into_iter()
            .take(100)
            .map(|(name, _)| name)
            .collect::<Vec<_>>();

        Ok(GlobOutput {
            duration_ms: started_at.elapsed().as_millis(),
            num_files: total,
            filenames,
            truncated,
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

        let results = self.run_ripgrep(&target, &input, before, after).await?;
        let total_matches = results.total_matches;

        Ok(match mode {
            GrepOutputMode::FilesWithMatches => {
                let filenames = apply_window(&results.matched_files, offset, limit);
                GrepOutput {
                    mode: Some(mode),
                    num_files: filenames.len(),
                    filenames,
                    content: None,
                    num_lines: None,
                    num_matches: Some(total_matches),
                    applied_limit: Some(limit),
                    applied_offset: Some(offset),
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
                    mode: Some(mode),
                    num_files: filenames.len(),
                    filenames,
                    content: Some(windowed.join("\n")),
                    num_lines: Some(windowed.len()),
                    num_matches: Some(total_matches),
                    applied_limit: Some(limit),
                    applied_offset: Some(offset),
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
                    mode: Some(mode),
                    num_files: filenames.len(),
                    filenames,
                    content: Some(
                        windowed
                            .iter()
                            .map(|entry| entry.text.clone())
                            .collect::<Vec<_>>()
                            .join("\n"),
                    ),
                    num_lines: Some(windowed.len()),
                    num_matches: Some(total_matches),
                    applied_limit: Some(limit),
                    applied_offset: Some(offset),
                }
            }
        })
    }

    pub async fn edit_notebook(&self, input: NotebookEditInput) -> Result<NotebookEditOutput> {
        self.edit_notebook_with_cell_number(input, None).await
    }

    pub async fn edit_notebook_with_cell_number(
        &self,
        input: NotebookEditInput,
        cell_number: Option<usize>,
    ) -> Result<NotebookEditOutput> {
        let path = self.require_absolute_file(Path::new(&input.notebook_path))?;
        if path.extension().and_then(|value| value.to_str()) != Some("ipynb") {
            return Err(LiteCodeError::invalid_input(format!(
                "NotebookEdit requires an .ipynb file, got {}.",
                path.display()
            )));
        }

        let original_file = tokio::fs::read_to_string(&path).await?;
        let mut notebook =
            serde_json::from_str::<serde_json::Value>(&original_file).map_err(|error| {
                LiteCodeError::invalid_input(format!("Invalid notebook JSON: {error}"))
            })?;
        let language = notebook_language(&notebook);
        let cells = notebook
            .get_mut("cells")
            .and_then(|value| value.as_array_mut())
            .ok_or_else(|| {
                LiteCodeError::invalid_input("Notebook does not contain a cells array.")
            })?;
        let output = match input.edit_mode {
            NotebookEditMode::Replace => {
                let ResolvedNotebookCell { index, cell_id } = resolve_notebook_cell(
                    cells,
                    input.cell_id.as_deref(),
                    cell_number,
                    false,
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
                    new_source: input.new_source.clone(),
                    cell_id,
                    cell_type,
                    language,
                    edit_mode: NotebookEditMode::Replace,
                    error: None,
                    notebook_path: path.display().to_string(),
                    original_file: original_file.clone(),
                    updated_file: String::new(),
                }
            }
            NotebookEditMode::Insert => {
                let cell_type = input.cell_type.ok_or_else(|| {
                    LiteCodeError::invalid_input("NotebookEdit insert mode requires cell_type.")
                })?;
                let cell_id = generated_cell_id();
                let new_cell = new_notebook_cell(&cell_id, cell_type, &input.new_source);
                let index = resolve_insert_index(cells, input.cell_id.as_deref(), cell_number)?;
                cells.insert(index, new_cell);

                NotebookEditOutput {
                    new_source: input.new_source.clone(),
                    cell_id: Some(cell_id),
                    cell_type,
                    language,
                    edit_mode: NotebookEditMode::Insert,
                    error: None,
                    notebook_path: path.display().to_string(),
                    original_file: original_file.clone(),
                    updated_file: String::new(),
                }
            }
            NotebookEditMode::Delete => {
                let ResolvedNotebookCell { index, cell_id } = resolve_notebook_cell(
                    cells,
                    input.cell_id.as_deref(),
                    cell_number,
                    false,
                    "delete",
                )?;
                let removed = cells.remove(index);
                let cell_type = removed
                    .as_object()
                    .and_then(notebook_cell_type)
                    .unwrap_or(NotebookCellType::Code);

                NotebookEditOutput {
                    new_source: input.new_source.clone(),
                    cell_id,
                    cell_type,
                    language,
                    edit_mode: NotebookEditMode::Delete,
                    error: None,
                    notebook_path: path.display().to_string(),
                    original_file: original_file.clone(),
                    updated_file: String::new(),
                }
            }
        };

        let updated_file = serde_json::to_string_pretty(&notebook)
            .map_err(|error| LiteCodeError::internal(error.to_string()))?;
        tokio::fs::write(&path, &updated_file).await?;
        self.mark_read(&path);

        Ok(NotebookEditOutput {
            updated_file,
            ..output
        })
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

    async fn run_ripgrep(
        &self,
        target: &Path,
        input: &GrepInput,
        before: usize,
        after: usize,
    ) -> Result<RipgrepSearchResult> {
        let mut command = Command::new("rg");
        command.arg("--json");
        command.args(["--color", "never"]);

        if input.output_mode == GrepOutputMode::Content {
            command.arg("-n");
            if before > 0 {
                command.arg("-B").arg(before.to_string());
            }
            if after > 0 {
                command.arg("-A").arg(after.to_string());
            }
        }

        if input.case_insensitive.unwrap_or(false) {
            command.arg("-i");
        }
        if let Some(glob_pattern) = input.glob.as_deref() {
            command.arg("--glob").arg(glob_pattern);
        }
        if let Some(file_type) = input.file_type.as_deref() {
            command.arg("--type").arg(file_type);
        }
        if input.multiline {
            command.args(["-U", "--multiline", "--multiline-dotall"]);
        }

        command.arg(&input.pattern);
        command.arg(target);

        let output = command.output().await.map_err(|error| {
            LiteCodeError::internal(format!("Failed to execute ripgrep: {error}"))
        })?;
        let status = output.status.code().unwrap_or_default();
        if status != 0 && status != 1 {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let message = if stderr.is_empty() {
                "ripgrep search failed.".to_string()
            } else {
                format!("ripgrep search failed: {stderr}")
            };
            return Err(LiteCodeError::invalid_input(message));
        }

        parse_ripgrep_output(
            &String::from_utf8_lossy(&output.stdout),
            input.output_mode,
            input.line_numbers.unwrap_or(true),
        )
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

fn render_notebook(content: &str) -> Result<String> {
    let json = serde_json::from_str::<serde_json::Value>(content)
        .map_err(|error| LiteCodeError::invalid_input(format!("Invalid notebook JSON: {error}")))?;
    let cells = json
        .get("cells")
        .and_then(|value| value.as_array())
        .ok_or_else(|| LiteCodeError::invalid_input("Notebook does not contain a cells array."))?;

    let mut rendered = Vec::new();
    for (index, cell) in cells.iter().enumerate() {
        let cell_type = cell
            .get("cell_type")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown");
        let source = value_as_lines(cell.get("source")).join("");
        let outputs = cell
            .get("outputs")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .map(|item| {
                        serde_json::to_string_pretty(item).unwrap_or_else(|_| item.to_string())
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();

        rendered.push(format!(
            "Cell {index} [{cell_type}]\n{source}{}",
            if outputs.is_empty() {
                String::new()
            } else {
                format!("\n[outputs]\n{outputs}")
            }
        ));
    }

    Ok(rendered.join("\n\n"))
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

fn numbered_lines(content: &str, offset: usize, limit: usize) -> String {
    let lines = content.lines().collect::<Vec<_>>();
    let start = offset.min(lines.len());
    let end = (start + limit).min(lines.len());

    lines[start..end]
        .iter()
        .enumerate()
        .map(|(index, line)| format!("{:>6}\t{line}", start + index + 1))
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_structured_patch(original: &str, updated: &str) -> Vec<StructuredPatchHunk> {
    let old_lines = collect_lines(original);
    let new_lines = collect_lines(updated);
    let patch_lines = old_lines
        .iter()
        .map(|line| format!("-{line}"))
        .chain(new_lines.iter().map(|line| format!("+{line}")))
        .collect::<Vec<_>>();

    vec![StructuredPatchHunk {
        old_start: 1,
        old_lines: old_lines.len(),
        new_start: 1,
        new_lines: new_lines.len(),
        lines: patch_lines,
    }]
}

fn parse_ripgrep_output(
    stdout: &str,
    mode: GrepOutputMode,
    line_numbers: bool,
) -> Result<RipgrepSearchResult> {
    let mut matched_files = Vec::new();
    let mut file_indexes = HashMap::new();
    let mut match_counts = Vec::<usize>::new();
    let mut content_lines = Vec::new();
    let mut total_matches = 0usize;

    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let value = serde_json::from_str::<serde_json::Value>(line).map_err(|error| {
            LiteCodeError::internal(format!("Failed to parse ripgrep JSON output: {error}"))
        })?;

        match value.get("type").and_then(serde_json::Value::as_str) {
            Some("match") => {
                let Some(path) = ripgrep_path(&value) else {
                    continue;
                };
                let file_index = match file_indexes.get(&path) {
                    Some(index) => *index,
                    None => {
                        let index = matched_files.len();
                        file_indexes.insert(path.clone(), index);
                        matched_files.push(path.clone());
                        match_counts.push(0);
                        index
                    }
                };

                let submatch_count = value
                    .get("data")
                    .and_then(|data| data.get("submatches"))
                    .and_then(serde_json::Value::as_array)
                    .map(|items| items.len().max(1))
                    .unwrap_or(1);
                match_counts[file_index] += submatch_count;

                if mode == GrepOutputMode::Content {
                    let line_text = ripgrep_lines_text(&value).unwrap_or_default();
                    let line_number = value
                        .get("data")
                        .and_then(|data| data.get("line_number"))
                        .and_then(serde_json::Value::as_u64)
                        .and_then(|value| usize::try_from(value).ok());
                    extend_rendered_lines(
                        &mut content_lines,
                        &path,
                        &line_text,
                        line_number,
                        line_numbers,
                    );
                }
            }
            Some("context") if mode == GrepOutputMode::Content => {
                let Some(path) = ripgrep_path(&value) else {
                    continue;
                };
                let line_text = ripgrep_lines_text(&value).unwrap_or_default();
                let line_number = value
                    .get("data")
                    .and_then(|data| data.get("line_number"))
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| usize::try_from(value).ok());
                extend_rendered_lines(
                    &mut content_lines,
                    &path,
                    &line_text,
                    line_number,
                    line_numbers,
                );
            }
            Some("summary") => {
                total_matches = value
                    .get("data")
                    .and_then(|data| data.get("stats"))
                    .and_then(|stats| stats.get("matches"))
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| usize::try_from(value).ok())
                    .unwrap_or(total_matches);
            }
            _ => {}
        }
    }

    if total_matches == 0 {
        total_matches = match_counts.iter().sum();
    }

    let match_counts = matched_files
        .iter()
        .cloned()
        .zip(match_counts)
        .collect::<HashMap<_, _>>();

    Ok(RipgrepSearchResult {
        matched_files,
        match_counts,
        content_lines,
        total_matches,
    })
}

fn ripgrep_path(value: &serde_json::Value) -> Option<String> {
    value
        .get("data")
        .and_then(|data| data.get("path"))
        .and_then(|path| path.get("text"))
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string)
}

fn ripgrep_lines_text(value: &serde_json::Value) -> Option<String> {
    let lines = value.get("data")?.get("lines")?;
    if let Some(text) = lines.get("text").and_then(serde_json::Value::as_str) {
        return Some(text.to_string());
    }

    lines
        .get("bytes")
        .and_then(serde_json::Value::as_str)
        .and_then(|encoded| {
            STANDARD
                .decode(encoded)
                .ok()
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        })
}

fn extend_rendered_lines(
    rendered: &mut Vec<RipgrepRenderedLine>,
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
        rendered.push(RipgrepRenderedLine {
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
    allow_end: bool,
    operation: &str,
) -> Result<ResolvedNotebookCell> {
    if let Some(index) = cell_number {
        let max_index = if allow_end {
            cells.len()
        } else {
            cells.len().saturating_sub(1)
        };
        if index > max_index || (!allow_end && index >= cells.len()) {
            return Err(LiteCodeError::invalid_input(format!(
                "NotebookEdit {operation} mode requires cell_number to be within 0..{}.",
                if allow_end { cells.len() } else { max_index }
            )));
        }

        let resolved_cell_id = cells
            .get(index)
            .and_then(|cell| cell.get("id"))
            .and_then(|value| value.as_str())
            .map(ToString::to_string);
        return Ok(ResolvedNotebookCell {
            index,
            cell_id: resolved_cell_id,
        });
    }

    let cell_id = cell_id.ok_or_else(|| {
        LiteCodeError::invalid_input(format!(
            "NotebookEdit {operation} mode requires cell_id or cell_number."
        ))
    })?;
    let index = find_cell_index(cells, cell_id)?;
    Ok(ResolvedNotebookCell {
        index,
        cell_id: Some(cell_id.to_string()),
    })
}

fn resolve_insert_index(
    cells: &[serde_json::Value],
    cell_id: Option<&str>,
    cell_number: Option<usize>,
) -> Result<usize> {
    if let Some(index) = cell_number {
        if index > cells.len() {
            return Err(LiteCodeError::invalid_input(format!(
                "NotebookEdit insert mode requires cell_number to be within 0..{}.",
                cells.len()
            )));
        }
        return Ok(index);
    }

    match cell_id {
        Some(existing_id) => Ok(find_cell_index(cells, existing_id)? + 1),
        None => Ok(0),
    }
}

fn collect_lines(content: &str) -> Vec<String> {
    if content.is_empty() {
        Vec::new()
    } else {
        content.lines().map(ToString::to_string).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RipgrepRenderedLine {
    path: String,
    text: String,
}

#[derive(Debug, Default)]
struct RipgrepSearchResult {
    matched_files: Vec<String>,
    match_counts: HashMap<String, usize>,
    content_lines: Vec<RipgrepRenderedLine>,
    total_matches: usize,
}

#[cfg(test)]
mod tests {
    use std::{
        convert::TryFrom,
        path::{Path, PathBuf},
    };

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
    async fn read_numbers_lines() {
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

        assert_eq!(content, ReadFileOutput::Text("     2\tbeta".to_string()));
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
            })
            .await
            .unwrap();

        assert_eq!(output.new_string, "during");
        assert_eq!(
            tokio::fs::read_to_string(&file).await.unwrap(),
            "during after"
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
                .unwrap()
                .contains("main.rs:2:let needle = 1;")
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
                new_source: "print('bye')\n".to_string(),
                cell_type: None,
                edit_mode: NotebookEditMode::Replace,
            })
            .await
            .unwrap();
        assert_eq!(replaced.language, "python");

        let inserted = service
            .edit_notebook(NotebookEditInput {
                notebook_path: file.display().to_string(),
                cell_id: Some("cell-a".to_string()),
                new_source: "# title\n".to_string(),
                cell_type: Some(NotebookCellType::Markdown),
                edit_mode: NotebookEditMode::Insert,
            })
            .await
            .unwrap();
        let inserted_id = inserted.cell_id.clone().unwrap();

        let deleted = service
            .edit_notebook(NotebookEditInput {
                notebook_path: file.display().to_string(),
                cell_id: Some(inserted_id),
                new_source: String::new(),
                cell_type: None,
                edit_mode: NotebookEditMode::Delete,
            })
            .await
            .unwrap();
        assert_eq!(deleted.edit_mode, NotebookEditMode::Delete);

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
            .edit_notebook_with_cell_number(
                NotebookEditInput {
                    notebook_path: file.display().to_string(),
                    cell_id: None,
                    new_source: "print('bye')\n".to_string(),
                    cell_type: None,
                    edit_mode: NotebookEditMode::Replace,
                },
                Some(1),
            )
            .await
            .unwrap();
        assert_eq!(replaced.cell_id.as_deref(), Some("cell-b"));

        let inserted = service
            .edit_notebook_with_cell_number(
                NotebookEditInput {
                    notebook_path: file.display().to_string(),
                    cell_id: None,
                    new_source: "## middle\n".to_string(),
                    cell_type: Some(NotebookCellType::Markdown),
                    edit_mode: NotebookEditMode::Insert,
                },
                Some(1),
            )
            .await
            .unwrap();
        assert!(inserted.cell_id.is_some());

        let deleted = service
            .edit_notebook_with_cell_number(
                NotebookEditInput {
                    notebook_path: file.display().to_string(),
                    cell_id: None,
                    new_source: String::new(),
                    cell_type: None,
                    edit_mode: NotebookEditMode::Delete,
                },
                Some(0),
            )
            .await
            .unwrap();
        assert_eq!(deleted.cell_id.as_deref(), Some("cell-a"));

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
