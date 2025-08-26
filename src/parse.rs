use bytes::Buf;

use crate::error::WuffErr;

pub trait Parse: Sized {
    fn parse(input: &mut impl Buf) -> Result<Self, WuffErr>;
}
