use std::hint::black_box;

use tango_bench::{IntoBenchmarks, benchmark_fn, tango_benchmarks, tango_main};

const ARIMO_VARIABLE: &[u8] = include_bytes!("assets/Arimo-VariableFont_wght.woff2");
const ROBOTO_REGULAR: &[u8] = include_bytes!("assets/Roboto-Regular.woff2");

fn benchmarks() -> impl IntoBenchmarks {
    [
        benchmark_fn("decompress_woff2/arimo_variable", |b| {
            b.iter(|| {
                wuff::decompress_woff2(black_box(ARIMO_VARIABLE))
                    .expect("Arimo benchmark font should decompress")
            })
        }),
        benchmark_fn("decompress_woff2/roboto_regular", |b| {
            b.iter(|| {
                wuff::decompress_woff2(black_box(ROBOTO_REGULAR))
                    .expect("Roboto benchmark font should decompress")
            })
        }),
    ]
}

tango_benchmarks!(benchmarks());
tango_main!();
