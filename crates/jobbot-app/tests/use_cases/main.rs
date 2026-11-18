//! The use cases of `jobbot-app` against the real adapters the test kit wires: the SQLite
//! stores (in memory or in a temp folder), the fake Claude and the made-up rules of
//! `jobbot_testkit::profile`. One test binary, one module per use case; `shipped_profile` is the
//! one that reads the files the repository ships.

mod actions;
mod answers;
mod claude;
mod dates;
mod digest;
mod discovery;
mod dossier;
mod freelance;
mod gate;
mod hostile;
mod inbox;
mod inbox_properties;
mod pipeline;
mod rules;
mod shipped_profile;
mod spans;
mod status;
mod today;
