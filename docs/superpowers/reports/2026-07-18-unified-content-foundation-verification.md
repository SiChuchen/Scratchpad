# Unified Content Foundation Verification

## Upgrade fixture

The upgrade regression starts from a real pre-content database produced by the
current Dock and Vault schema initializers:

- starting main schema version: 2
- starting Vault schema version: 4
- no content catalog, state, pending-delete, or content FTS table

The fixture records every column in each legacy payload table before the
upgrade, runs ensure_content_schema with 30 cleanup days, and compares every
value again.

| Legacy table | Before | After | Verified data |
| --- | ---: | ---: | --- |
| entries | 3 | 3 | text/image/file payload, paths, names, metadata, timestamps |
| home_entries | 2 | 2 | Home-only plus dual membership, positions 3.0 and 1.5 |
| note_entries | 2 | 2 | Note-only plus dual membership, positions 0.5 and 2.5 |
| vault_entries | 4 | 4 | credential/bookmark/note payload and timestamps |
| vault_fields | 4 | 4 | sort order, normal fields, password and token sensitive fields |
| vault_tags | 2 | 2 | manual and AI tag source |
| vault_ai_metadata | 3 | 3 | summary, aliases, hash, and ready/pending status |
| vault_fts | 4 | 4 | exactly one safe row per Vault entry |

The resulting catalog has seven unique rows. Mapping and order are:

- dock:home-only: temporary, inbox 3.0, cleanup after 30 days.
- dock:note-file: saved, saved position 0.5.
- dock:dual-member: saved, inbox 1.5, saved position 2.5.
- every Vault row: saved; stable append positions 3.5 through 6.5, ordered by
  source timestamps and IDs without changing payload order.

Every catalog item has exactly one content_fts row and every Vault payload has
exactly one vault_fts row. A second ensure call preserves the complete catalog
and projection snapshots, proving idempotence.

The two requested test filters initially selected zero tests at
efcbefbd3d9a7391068389d52e35dcf81306c71a. After the upgrade test was added,
its only RED was an incorrect expected Vault append position in the test; no
production migration gap was found. The corrected regression passes.

## Atomicity

failed_projection_write_rolls_back_payload_and_revision replaces the FTS5 table
in a test connection with an equivalent controlled table whose body constraint
rejects the new Dock text. SQLite does not permit triggers directly on a
virtual FTS5 table. It then invokes the real create_text_entry_with_revision
Home mutation.

The forced projection write fails and the transaction leaves all of these
unchanged: entries, home_entries, note_entries, content_catalog, content_fts,
vault_fts, and content_state.revision. No production atomicity change was
necessary. Existing full-suite Vault create/capture tests also verify that
unified projection failures leave no payload, capture request, catalog, FTS, or
revision residue.

## Sensitive boundary

The exact test-only literals are NeverIndexMePassword and NeverIndexMeToken;
they are not real credentials. Tests use the case/whitespace variants
 PaSsWoRd  and  ToKeN  with sensitive flags.

- both exact values are absent from every column of vault_fts and content_fts;
- content-changed JSON is restricted to revision and changes, with each change
  restricted to id and operation;
- undo JSON is restricted to token and expiresAt;
- failure/event log-shaped JSON is restricted to token, id, and code;
- none of those JSON payloads contains either exact value or either key;
- pending-delete storage has only token, unified_id, created_at, expires_at,
  and status.

The AI boundary remains covered by existing prompt tests: capture sends masked
text marked as untrusted data and does not send draft fields; query-plan prompts
contain the masked query and timestamp but no catalog, entries, tags, fields,
entry IDs, or payload. Pending/error AI metadata is excluded from projections.

## Deferred failure, restart, and concurrency

The full Rust suite covers the acceptance paths without duplicating fixtures:

- temporary Dock content and a Vault item after unsave are locally searchable;
- Dock and Vault share save/unsave/save retention and cleanup behavior;
- revision polling observes mutations even if an event is missed;
- pending undo survives a file-backed reopen and commits only at expiry;
- failed commits roll back payload/catalog/projection/revision, persist failed
  status, and recover without deleting the content;
- concurrent prepare calls produce one stable token, and restore versus due
  commit has exactly one winner;
- busy writers retry without incorrectly marking a token failed; worker wake,
  panic restart, cooldown, and deferred-token ordering regressions pass.

## Generated databases

Reproducible command, run from src-tauri:

    cargo run --example create_validation_fixtures -- ..\test-data\unified-validation

The example checks both target names and sidecars before creating anything,
rejects symlink targets/output directories, never opens the application data
directory, uses fixed IDs/timestamps/positions, cleans all newly created
database/sidecar files after any failure, and validates the completed databases
through read-only Rust connections. SQLite file-byte determinism is not
promised; logical fixture content is deterministic.

Read-only validation output:

- legacy: main v2, Vault v4, 7 payloads, 3 Dock kinds, 3 Vault kinds
  (4 Vault rows), Home/Note memberships 2/2, 4 vault_fts rows, content schema
  absent.
- fresh: main v4, Vault v4, 7 unique catalog rows, 2 temporary and 5 saved,
  7 content_fts, 4 vault_fts, revision 9, and one pending delete with only fixed
  token/id/timestamps/status.
- both FTS indexes contain zero occurrences of the exact sensitive literals.

Generated file evidence:

| File | Size | SHA-256 |
| --- | ---: | --- |
| legacy-validation.sqlite3 | 151,552 bytes | B2EE1552DC746CFBBCEA905F6A7B65E6E39CB0A533AE5CE91EFA7E739A0BC836 |
| fresh-validation.sqlite3 | 217,088 bytes | F6A3B9AEB15BF57FD25E29B657C28E0E57B4DEFC9A249F4F6CE7E6E144E071A6 |

A second invocation exited 1; both existing database hashes and mtimes were
unchanged (CHANGED=0). The databases were generated and then removed; the
command above reproduces them. The available Android SDK sqlite3 binary could
not load FTS5, so FTS/count/privacy validation used the example's bundled
rusqlite read-only verifier instead.

## Commands

| Command | Result |
| --- | --- |
| requested test filters before implementation | exit 0, 0 tests selected for each |
| cargo test upgrade_preserves_all_legacy_payload_counts_and_membership | exit 0, 1 passed |
| cargo test failed_projection_write_rolls_back_payload_and_revision | exit 0, 1 passed |
| privacy boundary regression filter | exit 0, 1 passed |
| fixture generation | exit 0; counts shown above |
| fixture generation into the same directory | inner exit 1; wrapper exit 0 after unchanged hash/mtime check |
| pnpm test:unit | exit 0; 19 files, 138 tests passed |
| pnpm check | exit 0; 0 errors, 7 pre-existing UI accessibility warnings |
| pnpm build | exit 0; 186 modules transformed |
| cargo test | exit 0; 400 tests passed, 0 failed |
| cargo clippy --all-targets --all-features -- -D warnings | exit 0 |
| cargo build | exit 0 |
| cargo fmt -- --check | exit 1; baseline diffs only in scratchpad/assets.rs, scratchpad/storage.rs, and system/window.rs |
| separate rustfmt check over Task 1-9 changed Rust files, excluding the three baseline files | exit 0 |
| git diff --check | exit 0 |

This foundation plan intentionally does not change UI behavior. UI integration
belongs to the later main-window and quick-access plans.
