## Stage 1: Slim Execution Tool Outputs
**Goal**: Remove redundant metadata from Bash, TaskOutput, TaskStop, and Glob responses.
**Success Criteria**: Execution-oriented tools no longer return timing, command echoes, or unused metadata; docs stay in sync.
**Tests**: `cargo test server::tests::tool_metadata_stays_in_sync_with_docs services::process::tests`
**Status**: Complete

## Stage 2: Slim File And Notebook Mutation Outputs
**Goal**: Reduce Write, Edit, and NotebookEdit responses to the latest content plus minimal status/context.
**Success Criteria**: Mutation tools stop returning file paths, old content, and diff payloads while still exposing the updated result.
**Tests**: `cargo test services::file_service::tests`
**Status**: In Progress

## Stage 3: Final Cleanup And Verification
**Goal**: Remove any leftover redundant output fields, verify the full suite, and clean up the implementation plan.
**Success Criteria**: Tool docs and runtime behavior match the slimmed outputs; `IMPLEMENTATION_PLAN.md` is removed once all stages are complete.
**Tests**: `cargo test`
**Status**: Not Started
