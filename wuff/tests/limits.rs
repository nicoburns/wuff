#![cfg(any(feature = "brotli", feature = "brotli-c"))]

use wuff::{decompress_woff2, decompress_woff2_with_limit};

const FONT: &[u8] = include_bytes!("../../conformance/fonts/wpt/valid-001.woff2");

#[test]
fn bounds_decoding_even_when_the_header_size_is_forged() {
    let decoded = decompress_woff2(FONT).expect("valid WOFF2 fixture");
    let mut forged = FONT.to_vec();
    forged[16..20].copy_from_slice(&1u32.to_be_bytes());

    assert!(decompress_woff2_with_limit(&forged, decoded.len() - 1).is_err());
    assert!(decompress_woff2_with_limit(&forged, decoded.len()).is_ok());
}

#[test]
fn bounds_table_and_collection_counts() {
    fn font(table_count: u16, collection_count: Option<u8>) -> Vec<u8> {
        let mut data = vec![0; 48];
        data[0..4].copy_from_slice(b"wOF2");
        data[4..8].copy_from_slice(if collection_count.is_some() {
            b"ttcf"
        } else {
            &[0, 1, 0, 0]
        });
        data[12..14].copy_from_slice(&table_count.to_be_bytes());
        data[16..20].copy_from_slice(&1u32.to_be_bytes());
        data[20..24].copy_from_slice(&1u32.to_be_bytes());
        data.extend(std::iter::repeat_n(0, table_count as usize * 2));
        if let Some(count) = collection_count {
            data.extend_from_slice(&[0, 1, 0, 0, count]);
            for _ in 0..count {
                data.extend_from_slice(&[1, 0, 1, 0, 0, 0]);
            }
        }
        data.push(0x3b);
        data.resize((data.len() + 3) & !3, 0);
        let len = data.len() as u32;
        data[8..12].copy_from_slice(&len.to_be_bytes());
        data
    }

    assert!(decompress_woff2(&font(256, None)).is_ok());
    assert!(decompress_woff2(&font(257, None)).is_err());
    assert!(decompress_woff2(&font(1, Some(128))).is_ok());
    assert!(decompress_woff2(&font(1, Some(129))).is_err());
}
