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

#[cfg(not(feature = "debug"))]
mod regular {
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
}
#[cfg(not(feature = "debug"))]
pub(crate) use regular::*;

#[cfg(feature = "debug")]
mod debug {
    macro_rules! bail {
        () => {
            panic!()
        };
    }
    pub(crate) use bail;

    macro_rules! bail_if {
        ($cond: expr) => {
            if $cond {
                panic!("{}", stringify!($cond))
            }
        };
    }
    pub(crate) use bail_if;

    macro_rules! bail_with_msg_if {
        ($cond: expr, $($msg:tt),*) => {
            if $cond {
                panic!($($msg),*);
            }
        };
    }
    pub(crate) use bail_with_msg_if;
}
#[cfg(feature = "debug")]
pub(crate) use debug::*;
