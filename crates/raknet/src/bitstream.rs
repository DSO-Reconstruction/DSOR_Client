//! RakNet's BitStream, as the 2018 client reads and writes it.
//!
//! Bits are most significant first within each byte; whole-byte integers are
//! little-endian (bytes reversed, bits within each byte still MSB-first). A field can
//! start at any bit, because booleans take one bit.
//!
//! EVIDENCE: the experimental server's raknet/bitstream.py, which the real client
//! accepts, and rust-18's check against RakNet's own C++ BitStream.

/// Builds a buffer field by field.
#[derive(Default, Clone, Debug)]
pub struct BitWriter {
    bytes: Vec<u8>,
    bits: usize,
}

impl BitWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(bytes: usize) -> Self {
        Self { bytes: Vec::with_capacity(bytes), bits: 0 }
    }

    /// How many bits have been written.
    pub fn bit_len(&self) -> usize {
        self.bits
    }

    /// The low `count` bits of `value`, most significant first.
    pub fn write_bits(&mut self, value: u64, count: u32) {
        debug_assert!(count <= 64);
        for shift in (0..count).rev() {
            let bit = (value >> shift) & 1;
            let at = self.bits;
            if at % 8 == 0 {
                self.bytes.push(0);
            }
            if bit != 0 {
                self.bytes[at / 8] |= 1 << (7 - (at % 8));
            }
            self.bits += 1;
        }
    }

    pub fn write_bool(&mut self, value: bool) {
        self.write_bits(value as u64, 1);
    }

    /// A `bits`-wide unsigned integer; whole bytes go little-endian.
    pub fn write_uint(&mut self, value: u64, bits: u32) {
        if bits % 8 != 0 {
            self.write_bits(value, bits);
            return;
        }
        for i in 0..bits / 8 {
            self.write_bits((value >> (8 * i)) & 0xFF, 8);
        }
    }

    pub fn write_u8(&mut self, value: u8) {
        self.write_uint(value as u64, 8);
    }

    pub fn write_u16(&mut self, value: u16) {
        self.write_uint(value as u64, 16);
    }

    pub fn write_u32(&mut self, value: u32) {
        self.write_uint(value as u64, 32);
    }

    pub fn write_f32(&mut self, value: f32) {
        self.write_u32(value.to_bits());
    }

    pub fn write_bytes(&mut self, data: &[u8]) {
        if self.bits % 8 == 0 {
            self.bytes.extend_from_slice(data);
            self.bits += data.len() * 8;
            return;
        }
        for &b in data {
            self.write_bits(b as u64, 8);
        }
    }

    /// A u16 length then the UTF-8 bytes, the game's usual string.
    pub fn write_string(&mut self, text: &str) {
        self.write_u16(text.len() as u16);
        self.write_bytes(text.as_bytes());
    }

    /// The buffer, the last byte zero-padded, and its true length in bits.
    pub fn finish(self) -> (Vec<u8>, usize) {
        (self.bytes, self.bits)
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

/// Error reading past the end of a buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overrun {
    pub wanted: usize,
    pub at: usize,
    pub length: usize,
}

impl std::fmt::Display for Overrun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "need {} bits at bit {}, buffer is {} bits", self.wanted, self.at, self.length)
    }
}

impl std::error::Error for Overrun {}

/// Reads fields of any width from a buffer.
#[derive(Clone, Debug)]
pub struct BitReader<'a> {
    data: &'a [u8],
    position: usize,
    length: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0, length: data.len() * 8 }
    }

    /// A reader that stops at `bits`, the frame's own declared length.
    pub fn with_bit_len(data: &'a [u8], bits: usize) -> Self {
        Self { data, position: 0, length: bits.min(data.len() * 8) }
    }

    pub fn position(&self) -> usize {
        self.position
    }

    pub fn seek(&mut self, bit: usize) {
        self.position = bit.min(self.length);
    }

    pub fn remaining(&self) -> usize {
        self.length - self.position
    }

    pub fn read_bits(&mut self, count: u32) -> Result<u64, Overrun> {
        let count = count as usize;
        if count > self.remaining() {
            return Err(Overrun { wanted: count, at: self.position, length: self.length });
        }
        let mut value = 0u64;
        for _ in 0..count {
            let byte = self.data[self.position >> 3];
            let bit = (byte >> (7 - (self.position & 7))) & 1;
            value = (value << 1) | bit as u64;
            self.position += 1;
        }
        Ok(value)
    }

    pub fn read_bool(&mut self) -> Result<bool, Overrun> {
        Ok(self.read_bits(1)? == 1)
    }

    pub fn read_uint(&mut self, bits: u32) -> Result<u64, Overrun> {
        if bits % 8 != 0 {
            return self.read_bits(bits);
        }
        let mut value = 0u64;
        for i in 0..bits / 8 {
            value |= self.read_bits(8)? << (8 * i);
        }
        Ok(value)
    }

    pub fn read_u8(&mut self) -> Result<u8, Overrun> {
        Ok(self.read_uint(8)? as u8)
    }

    pub fn read_u16(&mut self) -> Result<u16, Overrun> {
        Ok(self.read_uint(16)? as u16)
    }

    pub fn read_u32(&mut self) -> Result<u32, Overrun> {
        Ok(self.read_uint(32)? as u32)
    }

    pub fn read_i32(&mut self) -> Result<i32, Overrun> {
        Ok(self.read_u32()? as i32)
    }

    pub fn read_f32(&mut self) -> Result<f32, Overrun> {
        Ok(f32::from_bits(self.read_u32()?))
    }

    pub fn read_bytes(&mut self, count: usize) -> Result<Vec<u8>, Overrun> {
        if self.position % 8 == 0 && count * 8 <= self.remaining() {
            let start = self.position / 8;
            self.position += count * 8;
            return Ok(self.data[start..start + count].to_vec());
        }
        (0..count).map(|_| self.read_u8()).collect()
    }

    pub fn read_string(&mut self) -> Result<String, Overrun> {
        let n = self.read_u16()? as usize;
        let raw = self.read_bytes(n)?;
        Ok(String::from_utf8_lossy(&raw).into_owned())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn round_trip_unaligned_fields() {
        let mut w = BitWriter::new();
        w.write_bool(true);
        w.write_u32(0xDEADBEEF);
        w.write_string("Kingshill");
        w.write_bits(5, 3);
        w.write_f32(1.5);
        let (bytes, bits) = w.finish();
        assert_eq!(bits, 1 + 32 + 16 + 9 * 8 + 3 + 32);
        let mut r = BitReader::with_bit_len(&bytes, bits);
        assert!(r.read_bool().unwrap());
        assert_eq!(r.read_u32().unwrap(), 0xDEADBEEF);
        assert_eq!(r.read_string().unwrap(), "Kingshill");
        assert_eq!(r.read_bits(3).unwrap(), 5);
        assert_eq!(r.read_f32().unwrap(), 1.5);
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn matches_the_servers_layout() {
        // 0x84/121 hand-off as the experimental server sends it:
        // 84 7900 0e00 "127.0.0.1:2192" 80
        let mut w = BitWriter::new();
        w.write_u8(0x84);
        w.write_u16(121);
        w.write_string("127.0.0.1:2192");
        w.write_bool(true);
        assert_eq!(
            w.into_bytes(),
            hex("8479000e003132372e302e302e313a3231393280")
        );
    }

    pub(crate) fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }
}
