//! A page scan made of fuzzer bytes, planned with a made-up answer bank (`fuzz::form_plan`).
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| jobbot_testkit::fuzz::form_plan(data));
