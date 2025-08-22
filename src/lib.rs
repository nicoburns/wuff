//! Pure Rust WOFF2 decoder

#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(clippy::needless_range_loop)]

pub mod table_tags;
pub mod woff2_common;

#[inline(always)]
pub fn PREDICT_FALSE(cond: bool) -> bool {
    cond
}

#[inline(always)]
pub fn PREDICT_TRUE(cond: bool) -> bool {
    cond
}
