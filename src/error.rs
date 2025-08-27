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
