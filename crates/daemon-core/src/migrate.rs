//! Whole-chain, fail-closed SQLite `PRAGMA user_version` migration runner (S3 phase 1, spec §3).
//! MOVED from `bpa-sessiond::persistence::Db::migrate` (byte-for-byte semantics preserved): ONE
//! `unchecked_transaction()` wraps every applicable step plus the final `user_version` bump, so
//! any failure anywhere in the chain rolls back the WHOLE chain — never a partially-migrated
//! database. `bpa-sessiond` re-seats onto this runner via a `&[Migration]` table of its existing
//! per-version `execute_batch` bodies, moved verbatim into `apply` fns.

use rusqlite::{Connection, Transaction};

/// One migration step: `apply` runs (inside the shared chain transaction) whenever the caller's
/// `from_version < upto`. Callers pass steps in ascending `upto` order; `run_migrations` applies
/// them in the slice order given, it does not sort.
pub struct Migration {
    pub upto: i64,
    pub apply: fn(&Transaction) -> rusqlite::Result<()>,
}

/// Typed migration-runner error. Callers typically wrap this into their own persistence error
/// type (e.g. `bpa-sessiond::persistence::PersistError::Migration`) to keep a stable wire code
/// and message text for existing consumers.
#[derive(Debug, thiserror::Error)]
pub enum MigrateError {
    #[error("db user_version {found} newer than supported {supported}")]
    VersionTooNew { found: i64, supported: i64 },
    #[error("{0}")]
    Sql(#[from] rusqlite::Error),
}

/// Run every step in `steps` whose `upto > from_version`, in slice order, inside ONE transaction,
/// then set `PRAGMA user_version` to `target` and commit — all atomically.
///
/// - `from_version == target` short-circuits to `Ok(())` without opening a transaction (no-op
///   reopen of an already-current database).
/// - `from_version > target` is refused as `MigrateError::VersionTooNew` (never runs migrations
///   backward or silently truncates a newer schema).
/// - Any step failing (or the final `user_version` pragma/commit failing) rolls back the WHOLE
///   chain via the transaction's `Drop` — including tables created by EARLIER steps in this same
///   call — leaving `user_version` exactly as it was before the call (fail-closed).
pub fn run_migrations(
    conn: &Connection,
    from_version: i64,
    target: i64,
    steps: &[Migration],
) -> Result<(), MigrateError> {
    if from_version == target {
        return Ok(());
    }
    if from_version > target {
        return Err(MigrateError::VersionTooNew {
            found: from_version,
            supported: target,
        });
    }
    let tx = conn.unchecked_transaction()?;
    for step in steps {
        if from_version < step.upto {
            (step.apply)(&tx)?;
        }
    }
    tx.pragma_update(None, "user_version", target)?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_t1(tx: &Transaction) -> rusqlite::Result<()> {
        tx.execute_batch("CREATE TABLE t1 (id INTEGER PRIMARY KEY);")
    }

    fn create_t2(tx: &Transaction) -> rusqlite::Result<()> {
        tx.execute_batch("CREATE TABLE t2 (id INTEGER PRIMARY KEY);")
    }

    fn create_t3(tx: &Transaction) -> rusqlite::Result<()> {
        tx.execute_batch("CREATE TABLE t3 (id INTEGER PRIMARY KEY);")
    }

    /// Mirrors the real production shape (`CREATE TABLE ...; INSERT ... SELECT ...;` where the
    /// INSERT can fail, per `persistence.rs`'s v2->v3 step): creates a table, THEN fails on a
    /// later statement in the same batch, so both "this step's own partial work" and "earlier
    /// steps' committed-looking work" must be proven rolled back together.
    fn create_t2_then_fail(tx: &Transaction) -> rusqlite::Result<()> {
        tx.execute_batch(
            "CREATE TABLE t2_bad (id INTEGER PRIMARY KEY);
             INSERT INTO this_table_does_not_exist (id) VALUES (1);",
        )
    }

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |_| Ok(()),
        )
        .is_ok()
    }

    fn user_version(conn: &Connection) -> i64 {
        conn.query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn whole_chain_success_applies_every_step_and_reaches_target() {
        let conn = Connection::open_in_memory().unwrap();
        let steps = [
            Migration {
                upto: 1,
                apply: create_t1,
            },
            Migration {
                upto: 2,
                apply: create_t2,
            },
            Migration {
                upto: 3,
                apply: create_t3,
            },
        ];
        run_migrations(&conn, 0, 3, &steps).unwrap();
        assert!(table_exists(&conn, "t1"));
        assert!(table_exists(&conn, "t2"));
        assert!(table_exists(&conn, "t3"));
        assert_eq!(user_version(&conn), 3);
    }

    #[test]
    fn steps_at_or_below_from_version_are_skipped() {
        let conn = Connection::open_in_memory().unwrap();
        let steps = [
            Migration {
                upto: 1,
                apply: create_t1,
            },
            Migration {
                upto: 2,
                apply: create_t2,
            },
            Migration {
                upto: 3,
                apply: create_t3,
            },
        ];
        // from_version = 1: only steps with upto > 1 (i.e. upto=2, upto=3) must run.
        run_migrations(&conn, 1, 3, &steps).unwrap();
        assert!(
            !table_exists(&conn, "t1"),
            "upto=1 step must be skipped when from_version=1"
        );
        assert!(table_exists(&conn, "t2"));
        assert!(table_exists(&conn, "t3"));
        assert_eq!(user_version(&conn), 3);
    }

    #[test]
    fn mid_chain_failure_rolls_back_the_whole_chain_not_just_the_failing_step() {
        let conn = Connection::open_in_memory().unwrap();
        let steps = [
            Migration {
                upto: 1,
                apply: create_t1,
            },
            Migration {
                upto: 2,
                apply: create_t2_then_fail,
            },
            Migration {
                upto: 3,
                apply: create_t3,
            },
        ];
        let err = run_migrations(&conn, 0, 3, &steps).unwrap_err();
        assert!(matches!(err, MigrateError::Sql(_)));
        assert!(
            !table_exists(&conn, "t1"),
            "an EARLIER step's table must be rolled back too (whole-chain, not per-step)"
        );
        assert!(
            !table_exists(&conn, "t2_bad"),
            "the failing step's own partial work must be rolled back"
        );
        assert!(
            !table_exists(&conn, "t3"),
            "a LATER step must never have run after an earlier failure"
        );
        assert_eq!(
            user_version(&conn),
            0,
            "user_version must be untouched on failure (fail-closed)"
        );
    }

    #[test]
    fn version_too_new_is_rejected_without_touching_the_db() {
        let conn = Connection::open_in_memory().unwrap();
        let err = run_migrations(&conn, 5, 3, &[]).unwrap_err();
        match err {
            MigrateError::VersionTooNew { found, supported } => {
                assert_eq!(found, 5);
                assert_eq!(supported, 3);
            }
            other => panic!("expected VersionTooNew, got {other:?}"),
        }
        assert_eq!(user_version(&conn), 0, "no pragma write on rejection");
    }

    #[test]
    fn empty_steps_with_from_equal_target_zero_is_ok() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn, 0, 0, &[]).unwrap();
        assert_eq!(user_version(&conn), 0);
    }

    #[test]
    fn version_too_new_message_matches_expected_wire_text() {
        let err = MigrateError::VersionTooNew {
            found: 4,
            supported: 3,
        };
        assert_eq!(err.to_string(), "db user_version 4 newer than supported 3");
    }
}
