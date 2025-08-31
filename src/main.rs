use wuff::decompress_woff2;

fn main() {
    let mut args = std::env::args();
    let infile = args.nth(1).unwrap();
    let outfile = args.next().unwrap();

    println!("Reading from {infile}");
    let woff = std::fs::read(infile).unwrap();

    println!("Decoding woff2");
    let otf = decompress_woff2(&woff).unwrap();

    println!("Writing to {outfile}");
    std::fs::write(outfile, otf).unwrap();
}
