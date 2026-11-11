//! Fuzzer bytes as an apply link, through the browser's address check (`fuzz::address`).
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| jobbot_testkit::fuzz::address(data));
