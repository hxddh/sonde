//! What a recorded run amounts to. A run's file ([`run`]) is read once, as a
//! stream, into operations ([`episode`]), findings and what fixing them
//! saves ([`fixes`], [`fixloop`]), signals ([`signals`]), its profile and
//! cost ([`profile`]) and the page the app shows of it ([`view`]); a program
//! is followed over its runs ([`program`]), tried under faults ([`trial`]),
//! replayed on other stores ([`replay`]) and shared ([`share`],
//! [`report`]). Traffic sonde did not record is read as a run ([`import`]).
//! What the runs folder keeps beside them is its [`index`] and [`summary`].
//!
//! The recording itself, the endpoint, its policy, faults and TLS, are
//! `sonde-proxy`'s.

pub mod code;
pub mod engines;
pub mod episode;
pub mod fixes;
pub mod fixloop;
pub mod import;
pub mod index;
pub mod logs;
pub mod month;
pub mod phrase;
pub mod profile;
pub mod program;
pub mod project;
pub mod replay;
pub mod report;
pub mod run;
pub mod share;
pub mod signals;
pub mod split;
pub mod stored;
pub mod summary;
pub mod trial;
pub mod trial_plan;
pub mod view;
