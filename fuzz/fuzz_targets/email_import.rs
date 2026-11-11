//! Fuzzer bytes as an e-mail import file, imported into an empty store (`fuzz::email_import`).
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| jobbot_testkit::fuzz::email_import(data));
