## Stage 1: Read Image Output
**Goal**: Make `Read` return image-aware output for common image files instead of lossy UTF-8 text.
**Success Criteria**: Reading supported image files returns image content blocks; text files and notebooks keep existing readable behavior.
**Tests**: Unit tests cover image detection, image content generation, and unchanged text-file reads.
**Status**: Complete

## Stage 2: Read PDF Pages
**Goal**: Implement PDF reading with page-range validation and extracted page content.
**Success Criteria**: `Read` accepts valid PDF page selections, rejects invalid or oversized requests, and returns extracted text for requested pages.
**Tests**: Unit tests cover page-range parsing, page-limit enforcement, and PDF text extraction for selected pages.
**Status**: In Progress

## Stage 3: Ripgrep-backed Grep
**Goal**: Align `Grep` behavior with the documented ripgrep contract, including file-path searches.
**Success Criteria**: `Grep` shells out to `rg`, supports both directory and single-file `path` values, and preserves documented flags.
**Tests**: Unit tests cover file-path search, multiline search, output modes, and ripgrep-backed counts.
**Status**: Not Started

## Stage 4: Notebook Cell Number Alignment
**Goal**: Support notebook editing by 0-indexed `cell_number` as described in the tool contract.
**Success Criteria**: Replace, insert, and delete operations accept `cell_number`; existing `cell_id` behavior remains compatible where practical.
**Tests**: Unit tests cover replace/insert/delete by `cell_number` and compatibility with current ID-based flows.
**Status**: Not Started
