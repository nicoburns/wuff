// Tags of popular tables.
pub const kGlyfTableTag: u32 = 0x676c7966;
pub const kHeadTableTag: u32 = 0x68656164;
pub const kLocaTableTag: u32 = 0x6c6f6361;
pub const kDsigTableTag: u32 = 0x44534947;
pub const kCffTableTag: u32 = 0x43464620;
pub const kHmtxTableTag: u32 = 0x686d7478;
pub const kHheaTableTag: u32 = 0x68686561;
pub const kMaxpTableTag: u32 = 0x6d617870;

#[inline(always)]
pub fn PREDICT_FALSE(cond: bool) -> bool {
    cond
}

#[inline(always)]
pub fn PREDICT_TRUE(cond: bool) -> bool {
    cond
}

#[inline(always)]
pub fn FONT_COMPRESSION_FAILURE() -> bool {
    false
}

/// Output interface for the woff2 decoding.
///
/// Writes to arbitrary offsets are supported to facilitate updating offset
/// table and checksums after tables are ready. Reading the current size is
/// supported so a 'loca' table can be built up while writing glyphs.
///
/// By default limits size to kDefaultMaxSize.
///
trait WOFF2Out {
    /// Append n bytes of data from buf.
    /// Return true if all written, false otherwise.
    fn Write(&mut self, src: &[u8]) -> bool;

    /// Write n bytes of data from buf at offset.
    /// Return true if all written, false otherwise.
    fn WriteAtOffset(&mut self, src: &[u8], offset: usize) -> bool;

    fn Size(&self) -> usize;
}

struct Woff2MemoryOut;
impl Woff2MemoryOut {
    fn new() -> Self {
        Woff2MemoryOut
    }
}

#[inline]
fn StoreU32(dst: &mut [u8], offset: usize, x: u32) -> usize {
    dst[offset] = (x >> 24) as u8;
    dst[offset + 1] = (x >> 16) as u8;
    dst[offset + 2] = (x >> 8) as u8;
    dst[offset + 3] = x as u8;

    offset + 4
}

#[inline]
fn Store16(dst: &mut [u8], offset: usize, x: i32) -> usize {
    dst[offset] = (x >> 8) as u8;
    dst[offset + 1] = x as u8;

    offset + 2
}

#[inline]
fn StoreU32_mut(dst: &mut [u8], offset: &mut usize, x: u32) {
    dst[*offset] = (x >> 24) as u8;
    dst[*offset + 1] = (x >> 16) as u8;
    dst[*offset + 2] = (x >> 8) as u8;
    dst[*offset + 3] = x as u8;

    *offset += 4
}

#[inline]
fn Store16_mut(dst: &mut [u8], offset: &mut usize, x: i32) {
    dst[*offset] = (x >> 8) as u8;
    dst[*offset + 1] = x as u8;

    *offset += 2
}

// #[inline]
// fn StoreBytes(data: &mut[u8], offset: usize, uint8_t* dst) {
//   memcpy(&dst[*offset], data, len);
//   *offset += len;
// }

pub enum WuffErr {
    GenericError,
}

impl From<bytes::TryGetError> for WuffErr {
    fn from(_value: bytes::TryGetError) -> Self {
        Self::GenericError
    }
}

pub(crate) fn usize_will_overflow(a: usize, b: usize) -> bool {
    a.checked_add(b).is_none()
}

pub(crate) fn u32_will_overflow(a: u32, b: u32) -> bool {
    a.checked_add(b).is_none()
}


macro_rules! bail {
    () => {
        return Err(WuffErr::GenericError)
    };
}
pub(crate) use bail;

macro_rules! bail_if {
    ($cond: expr) => {
        if $cond {
            return Err(WuffErr::GenericError);
        }
    };
}
pub(crate) use bail_if;

macro_rules! bail_with_msg_if {
        ($cond: expr, $($msg:tt),*) => {
            if $cond {
                #[cfg(feature = "font_compression_bin")]
                eprintln!($($msg),*);
                return Err(WuffErr::GenericError);
            }
        };
    }
pub(crate) use bail_with_msg_if;

macro_rules! unwrap_or_bail {
    ($result: expr) => {
        match $result {
            Ok(val) => val,
            Err(_) => return false,
        }
    };
}
pub(crate) use unwrap_or_bail;
