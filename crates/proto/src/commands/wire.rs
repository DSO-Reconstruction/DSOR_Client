//! Field helpers shared by every command codec.
//!
//! The BitStream rules (dsor_raknet::bitstream): bits MSB-first, whole-byte integers
//! little-endian, strings a u16 length then the bytes. Every helper here is a thin
//! composition of those primitives, so a command body reads exactly as the server's
//! `w.write_uint(v, n)` / `w.write_bool(b)` / `w.write_string(s)` calls wrote it.

use dsor_raknet::{BitReader, BitWriter};

use super::DecodeError;

/// A command body: read from where its id ended, written the same way.
///
/// CONTRACT: `decode` consumes exactly the body (never the actor or the 0xFF tail), and
///   `encode` writes back the same bits, so decode -> encode reproduces the wire body.
pub trait Body: Sized {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError>;
    fn encode(&self, w: &mut BitWriter);
}

/// Integer, float and string readers past what BitReader offers.
pub trait ReadExt {
    fn u8(&mut self) -> Result<u8, DecodeError>;
    fn i8(&mut self) -> Result<i8, DecodeError>;
    fn u16(&mut self) -> Result<u16, DecodeError>;
    fn i16(&mut self) -> Result<i16, DecodeError>;
    fn u32(&mut self) -> Result<u32, DecodeError>;
    fn i32(&mut self) -> Result<i32, DecodeError>;
    fn u64(&mut self) -> Result<u64, DecodeError>;
    fn i64(&mut self) -> Result<i64, DecodeError>;
    fn f32(&mut self) -> Result<f32, DecodeError>;
    fn bit(&mut self) -> Result<bool, DecodeError>;
    fn bits(&mut self, count: u32) -> Result<u64, DecodeError>;
    fn string(&mut self) -> Result<String, DecodeError>;
    fn vec3(&mut self) -> Result<[f32; 3], DecodeError>;
    /// A u32 count, refused above `most` (the client's own bound where known, else a
    /// sanity bound) so a misread never allocates gigabytes.
    fn count(&mut self, most: u32, what: &'static str) -> Result<usize, DecodeError>;
    /// `bits` raw bits, kept verbatim.
    fn raw(&mut self, bits: usize) -> Result<Raw, DecodeError>;
}

impl ReadExt for BitReader<'_> {
    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.read_u8()?)
    }
    fn i8(&mut self) -> Result<i8, DecodeError> {
        Ok(self.read_u8()? as i8)
    }
    fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(self.read_u16()?)
    }
    fn i16(&mut self) -> Result<i16, DecodeError> {
        Ok(self.read_u16()? as i16)
    }
    fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(self.read_u32()?)
    }
    fn i32(&mut self) -> Result<i32, DecodeError> {
        Ok(self.read_u32()? as i32)
    }
    fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(self.read_uint(64)?)
    }
    fn i64(&mut self) -> Result<i64, DecodeError> {
        Ok(self.read_uint(64)? as i64)
    }
    fn f32(&mut self) -> Result<f32, DecodeError> {
        Ok(self.read_f32()?)
    }
    fn bit(&mut self) -> Result<bool, DecodeError> {
        Ok(self.read_bool()?)
    }
    fn bits(&mut self, count: u32) -> Result<u64, DecodeError> {
        Ok(self.read_bits(count)?)
    }
    fn string(&mut self) -> Result<String, DecodeError> {
        Ok(self.read_string()?)
    }
    fn vec3(&mut self) -> Result<[f32; 3], DecodeError> {
        Ok([self.read_f32()?, self.read_f32()?, self.read_f32()?])
    }
    fn count(&mut self, most: u32, what: &'static str) -> Result<usize, DecodeError> {
        let n = self.read_u32()?;
        if n > most {
            return Err(DecodeError::Invalid { what, value: n as i64 });
        }
        Ok(n as usize)
    }
    fn raw(&mut self, bits: usize) -> Result<Raw, DecodeError> {
        Raw::read(self, bits)
    }
}

/// Writers mirroring ReadExt.
pub trait WriteExt {
    fn u8(&mut self, v: u8);
    fn i8(&mut self, v: i8);
    fn u16(&mut self, v: u16);
    fn i16(&mut self, v: i16);
    fn u32(&mut self, v: u32);
    fn i32(&mut self, v: i32);
    fn u64(&mut self, v: u64);
    fn i64(&mut self, v: i64);
    fn f32(&mut self, v: f32);
    fn bit(&mut self, v: bool);
    fn bits(&mut self, v: u64, count: u32);
    fn string(&mut self, v: &str);
    fn vec3(&mut self, v: [f32; 3]);
    fn count(&mut self, n: usize);
}

impl WriteExt for BitWriter {
    fn u8(&mut self, v: u8) {
        self.write_u8(v)
    }
    fn i8(&mut self, v: i8) {
        self.write_u8(v as u8)
    }
    fn u16(&mut self, v: u16) {
        self.write_u16(v)
    }
    fn i16(&mut self, v: i16) {
        self.write_u16(v as u16)
    }
    fn u32(&mut self, v: u32) {
        self.write_u32(v)
    }
    fn i32(&mut self, v: i32) {
        self.write_u32(v as u32)
    }
    fn u64(&mut self, v: u64) {
        self.write_uint(v, 64)
    }
    fn i64(&mut self, v: i64) {
        self.write_uint(v as u64, 64)
    }
    fn f32(&mut self, v: f32) {
        self.write_f32(v)
    }
    fn bit(&mut self, v: bool) {
        self.write_bool(v)
    }
    fn bits(&mut self, v: u64, count: u32) {
        self.write_bits(v, count)
    }
    fn string(&mut self, v: &str) {
        self.write_string(v)
    }
    fn vec3(&mut self, v: [f32; 3]) {
        for f in v {
            self.write_f32(f);
        }
    }
    fn count(&mut self, n: usize) {
        self.write_u32(n as u32)
    }
}

/// Bits kept verbatim: an unknown command's body, or a span whose layout is not
/// established. `data` is MSB-first, the last byte zero-padded.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Raw {
    pub data: Vec<u8>,
    pub bits: usize,
}

impl Raw {
    pub fn read(r: &mut BitReader<'_>, bits: usize) -> Result<Self, DecodeError> {
        if bits > r.remaining() {
            return Err(dsor_raknet::Overrun { wanted: bits, at: r.position(), length: r.position() + r.remaining() }.into());
        }
        let mut data = Vec::with_capacity(bits.div_ceil(8));
        let mut left = bits;
        while left >= 8 {
            data.push(r.read_bits(8)? as u8);
            left -= 8;
        }
        if left > 0 {
            data.push((r.read_bits(left as u32)? as u8) << (8 - left));
        }
        Ok(Self { data, bits })
    }

    pub fn write(&self, w: &mut BitWriter) {
        let mut left = self.bits;
        for &b in &self.data {
            if left >= 8 {
                w.write_bits(b as u64, 8);
                left -= 8;
            } else {
                w.write_bits((b >> (8 - left)) as u64, left as u32);
                left = 0;
            }
        }
    }
}

/// Read `n` items with `f`.
pub fn list<T>(
    r: &mut BitReader<'_>,
    n: usize,
    mut f: impl FnMut(&mut BitReader<'_>) -> Result<T, DecodeError>,
) -> Result<Vec<T>, DecodeError> {
    let mut out = Vec::with_capacity(n.min(4096));
    for _ in 0..n {
        out.push(f(r)?);
    }
    Ok(out)
}

/// A u32 count (refused above `most`), then that many items.
pub fn counted<T>(
    r: &mut BitReader<'_>,
    most: u32,
    what: &'static str,
    f: impl FnMut(&mut BitReader<'_>) -> Result<T, DecodeError>,
) -> Result<Vec<T>, DecodeError> {
    let n = r.count(most, what)?;
    list(r, n, f)
}

/// The client's usual bound on a counted list (sub_8FD904 and its siblings:
/// 0 <= count <= 1000000).
pub const MOST: u32 = 1_000_000;
