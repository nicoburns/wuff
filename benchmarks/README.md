# Benchmark

## Running the benchmark

Run a standalone measurement with:

```sh
cargo bench -p wuff-benchmarks --bench decompress_woff2 -- solo -t 5
```

For an A/B comparison, install `cargo-export` once:

```sh
cargo install cargo-export --locked
```

Before changing the code, build and export the baseline under a stable name:

```sh
cargo export target/benchmarks -- \
  bench -p wuff-benchmarks --bench decompress_woff2
```

After changing the code, run the comparison from the workspace root:

```sh
cargo bench -p wuff-benchmarks --bench decompress_woff2 -- \
  compare target/benchmarks/decompress_woff2 \
  -t 5 -v
```

## Fonts

Both fonts are licensed under the Apache License 2.0. Their license texts are
included alongside the generated files.

### Arimo Variable

- Source: `parley_dev/assets/fonts/arimo_fonts/Arimo-VariableFont_wght.ttf`
- Source SHA-256: `3e361011862a83ebae8768a325fb69a747ba888c843fff78dfe0c17ff73ec946`
- WOFF2 SHA-256: `4a81951175bec926c21e3917f269504c677b0630d163fe51bd7293c394cb1507`
- License: `LICENSE-Arimo.txt`

### Roboto Regular

- Source: `parley_dev/assets/fonts/roboto_fonts/Roboto-Regular.ttf`
- Source SHA-256: `319cff6e7a31f0f2a41c475dca42890aa5d19fe16017e2290f8c1d4e14f76481`
- WOFF2 SHA-256: `07797815fdd612d152d18e01df5ca89cc4634df3a69d96ca6c10e1284a4dbcbd`
- License: `LICENSE-Roboto-Regular.txt`

### Reproducing the fixtures

The files were generated with FontTools 4.63.0:

```sh
fonttools ttLib.woff2 compress -q --hmtx-transform \
  ./Arimo-VariableFont_wght.ttf \
  -o benchmarks/benches/assets/Arimo-VariableFont_wght.woff2

fonttools ttLib.woff2 compress -q --hmtx-transform \
  ./Roboto-Regular.ttf \
  -o benchmarks/benches/assets/Roboto-Regular.woff2
```
