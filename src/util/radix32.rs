#[cfg(any(feature = "tracing", feature = "http-client"))]
use std::fmt::Display;
#[cfg(any(feature = "tracing", feature = "http-client"))]
use std::fmt::Formatter;
#[cfg(any(feature = "tracing", feature = "http-client"))]
use std::str::from_utf8_unchecked;

#[cfg(any(feature = "tracing", feature = "http-client"))]
pub struct Radix32(u128);

#[cfg(any(feature = "tracing", feature = "http-client"))]
#[inline]
pub fn radix_32(n: u128) -> Radix32 {
    Radix32(n)
}

#[cfg(any(feature = "tracing", feature = "http-client"))]
impl Display for Radix32 {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        const MASK: u128 = (1 << 5) - 1;
        let mut n = self.0;
        let mut buf = [0_u8; 26];
        let mut index = 0;
        for i in (0..26).rev() {
            match (n & MASK) as u8 {
                d @ 0..10 => {
                    buf[i] = b'0' + d;
                }
                d @ 10..32 => {
                    buf[i] = b'a' + d - 10;
                }
                _ => unreachable!("unreachable")
            }
            n = n >> 5;
            if n == 0 {
                index = i;
                break;
            }
        }
        let s = unsafe { from_utf8_unchecked(&buf[index..]) };
        f.write_str(s)
    }
}

#[cfg(feature = "http-server-tracer")]
pub fn from_radix_32(s: &str) -> Option<u128> {
    if !matches!(s.len(), 1..=26) {
        return None;
    }
    let mut n: u128 = 0;
    for &c in s.as_bytes().iter() {
        n = n << 5;
        match c {
            b'0'..=b'9' => {
                n |= (c - b'0') as u128
            }
            b'a'..=b'z' => {
                n |= (c - b'a' + 10) as u128
            }
            _ => return None
        }
    }
    Some(n)
}
