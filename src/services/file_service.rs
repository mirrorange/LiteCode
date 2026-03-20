use std::{
    collections::{BTreeSet, HashSet},
    ops::RangeInclusive,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};

use globwalk::GlobWalkerBuilder;
use regex::{Regex, RegexBuilder};

use crate::{
    error::{LiteCodeError, Result},
    schema::{
        EditInput, EditOutput, GlobInput, GlobOutput, GrepInput, GrepOutput, GrepOutputMode,
        StructuredPatchHunk, WriteInput, WriteOutput, WriteResultType,
    },
};

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
    ) -> Result<String> {
        let path = self.require_absolute_file(file_path.as_ref())?;
        let metadata = tokio::fs::metadata(&path).await?;
        if metadata.is_dir() {
            return Err(LiteCodeError::invalid_input(
                "Read can only be used with files, not directories.",
            ));
        }

        if path.extension().and_then(|value| value.to_str()) == Some("pdf") {
            return Err(LiteCodeError::invalid_input(format!(
                "PDF reading is not implemented yet for {}{}",
                path.display(),
                pages
                    .map(|range| format!(" (requested pages {range})"))
                    .unwrap_or_default()
            )));
        }

        let raw = tokio::fs::read(&path).await?;
        let text = if path.extension().and_then(|value| value.to_str()) == Some("ipynb") {
            render_notebook(&String::from_utf8_lossy(&raw))?
        } else {
            String::from_utf8_lossy(&raw).into_owned()
        };

        self.mark_read(&path);

        if text.is_empty() {
            return Ok("Warning: file exists but is empty.".to_string());
        }

        Ok(numbered_lines(
            &text,
            offset.unwrap_or(0),
            limit.unwrap_or(2_000),
        ))
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
        let root = self.resolve_search_root(input.path.as_deref())?;
        let files =
            self.collect_candidate_files(&root, input.glob.as_deref(), input.file_type.as_deref())?;
        let matcher = build_regex(&input)?;
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

        let mut matched_files = Vec::new();
        let mut content_entries = Vec::new();
        let mut count_entries = Vec::new();
        let mut total_matches = 0usize;

        for path in files {
            let file_text = match tokio::fs::read_to_string(&path).await {
                Ok(content) => content,
                Err(_) => continue,
            };

            let file_matches = if input.multiline {
                matcher.find_iter(&file_text).count()
            } else {
                line_matches(&matcher, &file_text).len()
            };

            if file_matches == 0 {
                continue;
            }

            total_matches += file_matches;
            matched_files.push(path.display().to_string());

            match mode {
                GrepOutputMode::FilesWithMatches => {}
                GrepOutputMode::Count => {
                    count_entries.push(format!("{}:{file_matches}", path.display()));
                }
                GrepOutputMode::Content => {
                    if input.multiline {
                        for matched in matcher.find_iter(&file_text) {
                            let start_line = file_text[..matched.start()]
                                .bytes()
                                .filter(|byte| *byte == b'\n')
                                .count()
                                + 1;
                            let snippet = matched.as_str().replace('\n', "\\n");
                            content_entries
                                .push(format!("{}:{start_line}:{snippet}", path.display()));
                        }
                    } else {
                        let lines = collect_content_lines(
                            &path,
                            &file_text,
                            &matcher,
                            before,
                            after,
                            input.line_numbers.unwrap_or(true),
                        );
                        content_entries.extend(lines);
                    }
                }
            }
        }

        Ok(match mode {
            GrepOutputMode::FilesWithMatches => {
                let filenames = apply_window(&matched_files, offset, limit);
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
                let windowed = apply_window(&content_entries, offset, limit);
                let filenames = windowed
                    .iter()
                    .filter_map(|entry| entry.split_once(':').map(|(path, _)| path.to_string()))
                    .collect::<BTreeSet<_>>()
                    .into_iter()
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

    fn collect_candidate_files(
        &self,
        root: &Path,
        glob_pattern: Option<&str>,
        file_type: Option<&str>,
    ) -> Result<Vec<PathBuf>> {
        let pattern = glob_pattern.unwrap_or("**/*");
        let walker = GlobWalkerBuilder::from_patterns(root, &[pattern])
            .build()
            .map_err(|error| LiteCodeError::internal(error.to_string()))?;

        let mut files = walker
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| entry.path().to_path_buf())
            .filter(|path| matches_file_type(path, file_type))
            .collect::<Vec<_>>();

        files.sort();
        Ok(files)
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

fn build_regex(input: &GrepInput) -> Result<Regex> {
    RegexBuilder::new(&input.pattern)
        .case_insensitive(input.case_insensitive.unwrap_or(false))
        .dot_matches_new_line(input.multiline)
        .build()
        .map_err(|error| LiteCodeError::invalid_input(format!("Invalid grep pattern: {error}")))
}

fn line_matches(regex: &Regex, content: &str) -> Vec<usize> {
    content
        .lines()
        .enumerate()
        .filter_map(|(index, line)| regex.is_match(line).then_some(index))
        .collect()
}

fn collect_content_lines(
    path: &Path,
    content: &str,
    regex: &Regex,
    before: usize,
    after: usize,
    line_numbers: bool,
) -> Vec<String> {
    let lines = content.lines().collect::<Vec<_>>();
    let matches = line_matches(regex, content);
    let mut ranges = Vec::<RangeInclusive<usize>>::new();

    for matched_line in matches {
        let start = matched_line.saturating_sub(before);
        let end = (matched_line + after).min(lines.len().saturating_sub(1));

        match ranges.last_mut() {
            Some(last) if start <= *last.end() + 1 => {
                let merged_start = *last.start();
                *last = merged_start..=end.max(*last.end());
            }
            _ => ranges.push(start..=end),
        }
    }

    let mut rendered = Vec::new();
    for range in ranges {
        for index in range {
            let line = lines.get(index).copied().unwrap_or_default();
            if line_numbers {
                rendered.push(format!("{}:{}:{line}", path.display(), index + 1));
            } else {
                rendered.push(format!("{}:{line}", path.display()));
            }
        }
    }

    rendered
}

fn apply_window(entries: &[String], offset: usize, limit: usize) -> Vec<String> {
    let iter = entries.iter().skip(offset);
    if limit == 0 {
        iter.cloned().collect()
    } else {
        iter.take(limit).cloned().collect()
    }
}

fn matches_file_type(path: &Path, file_type: Option<&str>) -> bool {
    let Some(file_type) = file_type else {
        return true;
    };
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let matches = match file_type {
        "rust" => &["rs"][..],
        "js" => &["js", "cjs", "mjs"][..],
        "ts" => &["ts"][..],
        "tsx" => &["tsx"][..],
        "py" => &["py"][..],
        "go" => &["go"][..],
        "java" => &["java"][..],
        "json" => &["json"][..],
        "md" => &["md"][..],
        "toml" => &["toml"][..],
        other => &[other],
    };

    matches.contains(&ext)
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

fn collect_lines(content: &str) -> Vec<String> {
    if content.is_empty() {
        Vec::new()
    } else {
        content.lines().map(ToString::to_string).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tempfile::tempdir;

    use crate::schema::{EditInput, GlobInput, GrepInput, GrepOutputMode, WriteInput};

    use super::FileService;

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

        assert_eq!(content, "     2\tbeta");
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
}
