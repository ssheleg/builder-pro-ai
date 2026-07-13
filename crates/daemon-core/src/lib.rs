//! `bpa-daemon-core` — shared daemon primitives extracted from `bpa-sessiond` (S3 phase 1, spec
//! §3) so a second daemon (`bpa-orchd`) can reuse them without depending on the sessiond crate
//! itself. This phase moves `dirs`, `singleton`, and `logging` verbatim, parameterizing only the
//! hardcoded socket/lock/log file names; every other behavior (runtime-dir resolution,
//! permissions, locking, peer-cred, tracing setup) is byte-for-byte unchanged from the
//! pre-extraction `bpa-sessiond` code. `bpa-sessiond` re-seats onto this crate via thin wrappers
//! that pin its own on-disk names (`d.sock`/`d.lock`/`sessiond.tracing.log`).

pub mod broadcast;
pub mod dirs;
pub mod handshake;
pub mod logging;
pub mod migrate;
pub mod singleton;
