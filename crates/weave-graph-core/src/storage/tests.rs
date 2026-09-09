use super::*;

// Compile-time check: the trait must stay dyn-compatible so callers can
// hold `Box<dyn Storage>` without committing to a backend at compile
// time.
#[allow(dead_code)]
fn assert_object_safe(_: &dyn Storage) {}
