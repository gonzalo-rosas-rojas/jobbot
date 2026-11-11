//! The use cases of `jobbot-app` against the real adapters the test kit wires: the SQLite
//! stores (in memory or in a temp folder), the fake Claude and the shipped profile. One test
//! binary, one module per use case.

mod actions;
mod answers;
mod claude;
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
mod spans;
mod status;
mod today;
mod window;
