use wuff::decompress_woff1;
use wuff::decompress_woff2;

fn main() {
    let mut args = std::env::args();
    let infile = args.nth(1).unwrap();
    let outfile = args.next().unwrap();

    println!("Reading from {infile}");
    let woff = std::fs::read(&infile).unwrap();

    let otf = if infile.ends_with("woff") {
        println!("Decoding woff1");
        decompress_woff1(&woff).unwrap()
    } else {
        println!("Decoding woff2");
        decompress_woff2(&woff).unwrap()
    };

    println!("Writing to {outfile}");
    std::fs::write(outfile, otf).unwrap();
}
