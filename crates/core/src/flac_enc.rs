//! A streaming FLAC encoder (RFC 9639) for [`crate::save`]: our own code, so the document can be
//! written block by block (the audio-io encoder takes a whole buffer, which for a three-hour
//! file is over 10 GB). Fixed 4096-frame blocks, independent channels, and per subframe the
//! cheapest of CONSTANT, FIXED (orders 0 to 4, partitioned Rice residual) or VERBATIM. The total
//! frame count is known up front, so STREAMINFO is written first and the file is never seeked.
//! The MD5 field is left zero ("not computed"), which every decoder accepts.

use std::io::{self, Write};

/// Frames in every block but the last.
pub const BLOCK_SIZE: usize = 4096;
const MAX_PARTITION_ORDER: u32 = 5;

struct BitWriter {
    out: Vec<u8>,
    acc: u64,
    nbits: u32,
}

impl BitWriter {
    fn new() -> Self {
        Self { out: Vec::new(), acc: 0, nbits: 0 }
    }

    /// The low `n` (at most 32) bits of `v`, most significant first.
    fn write(&mut self, v: u64, n: u32) {
        let n = n.min(32);
        if n == 0 {
            return;
        }
        self.acc = (self.acc << n) | (v & ((1u64 << n) - 1));
        self.nbits += n;
        while self.nbits >= 8 {
            self.nbits -= 8;
            self.out.push((self.acc >> self.nbits) as u8);
        }
        self.acc &= (1u64 << self.nbits) - 1;
    }

    fn write_signed(&mut self, v: i64, n: u32) {
        self.write(v as u64, n);
    }

    /// `q` zero bits and a one.
    fn write_unary(&mut self, mut q: u64) {
        while q >= 31 {
            self.write(0, 31);
            q -= 31;
        }
        self.write(1, q as u32 + 1);
    }

    fn align(&mut self) {
        if self.nbits > 0 {
            self.write(0, 8 - self.nbits);
        }
    }

    fn bytes(&self) -> &[u8] {
        &self.out
    }
}

fn crc8(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 { (crc << 1) ^ 0x07 } else { crc << 1 };
        }
    }
    crc
}

fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &b in data {
        crc ^= u16::from(b) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x8005 } else { crc << 1 };
        }
    }
    crc
}

/// FLAC's UTF-8-like coding of the frame number (up to 36 bits).
fn write_utf8_number(w: &mut BitWriter, v: u64) {
    if v < 0x80 {
        w.write(v, 8);
        return;
    }
    let n: u32 = (2..7).find(|&n| v < (1u64 << (5 * n + 1))).unwrap_or(7);
    let cont_bits = 6 * (n - 1);
    let lead_payload = if n == 7 { 0 } else { 7 - n };
    let prefix = (0xFFu64 << (8 - n)) & 0xFF;
    w.write(prefix | ((v >> cont_bits) & ((1u64 << lead_payload) - 1)), 8);
    for i in (0..n - 1).rev() {
        w.write(0x80 | ((v >> (6 * i)) & 0x3F), 8);
    }
}

fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

/// The residual of fixed predictor `order` over `x` (from index `order` on).
fn fixed_residual(x: &[i64], order: usize) -> Vec<i64> {
    (order..x.len())
        .map(|i| match order {
            0 => x[i],
            1 => x[i] - x[i - 1],
            2 => x[i] - 2 * x[i - 1] + x[i - 2],
            3 => x[i] - 3 * x[i - 1] + 3 * x[i - 2] - x[i - 3],
            _ => x[i] - 4 * x[i - 1] + 6 * x[i - 2] - 4 * x[i - 3] + x[i - 4],
        })
        .collect()
}

/// The Rice parameter that suits `count` values summing to `sum`.
fn rice_param(sum: u64, count: u64) -> u32 {
    if count == 0 {
        return 0;
    }
    let mean = sum / count;
    (if mean == 0 { 0 } else { 63 - mean.leading_zeros() }).min(30)
}

/// A partition split of a residual: its order and the estimated size in bits.
fn plan(u: &[u64], n: usize, order: usize) -> (u32, u64) {
    let mut best = (0u32, u64::MAX);
    for p in 0..=MAX_PARTITION_ORDER {
        let parts = 1usize << p;
        if !n.is_multiple_of(parts) || n / parts <= order {
            break;
        }
        let size = n / parts;
        let mut bits = 4 + 5 * parts as u64;
        let mut at = 0;
        for part in 0..parts {
            let len = if part == 0 { size - order } else { size };
            let s: u64 = u[at..at + len].iter().sum();
            let k = rice_param(s, len as u64);
            bits += len as u64 * (u64::from(k) + 1) + (s >> k);
            at += len;
        }
        if bits < best.1 {
            best = (p, bits);
        }
    }
    best
}

fn write_subframe(w: &mut BitWriter, x: &[i64], bps: u32) {
    let n = x.len();
    let first = x.first().copied().unwrap_or(0);
    if x.iter().all(|&s| s == first) {
        w.write(0, 8); // pad, CONSTANT, no wasted bits
        w.write_signed(first, bps);
        return;
    }
    let verbatim = n as u64 * u64::from(bps);
    let mut best: Option<(usize, Vec<u64>, u32, u64)> = None;
    for order in 0..=4usize.min(n.saturating_sub(1)) {
        let u: Vec<u64> = fixed_residual(x, order).into_iter().map(zigzag).collect();
        let (p, bits) = plan(&u, n, order);
        if bits == u64::MAX {
            continue;
        }
        let bits = bits + order as u64 * u64::from(bps);
        if bits < verbatim && best.as_ref().is_none_or(|b| bits < b.3) {
            best = Some((order, u, p, bits));
        }
    }
    match best {
        Some((order, u, p, _)) => {
            w.write(0, 1);
            w.write(8 + order as u64, 6);
            w.write(0, 1);
            for &s in &x[..order] {
                w.write_signed(s, bps);
            }
            w.write(1, 2); // Rice coding, 5-bit parameters
            w.write(u64::from(p), 4);
            let (parts, size) = (1usize << p, n >> p);
            let mut at = 0;
            for part in 0..parts {
                let len = if part == 0 { size - order } else { size };
                let s: u64 = u[at..at + len].iter().sum();
                let k = rice_param(s, len as u64);
                w.write(u64::from(k), 5);
                for &v in &u[at..at + len] {
                    w.write_unary(v >> k);
                    w.write(v & ((1u64 << k) - 1), k);
                }
                at += len;
            }
        }
        None => {
            w.write(0, 1);
            w.write(1, 6);
            w.write(0, 1);
            for &s in x {
                w.write_signed(s, bps);
            }
        }
    }
}

/// Writes a FLAC stream of `total` frames one block at a time.
pub struct Encoder {
    channels: usize,
    bps: u32,
    frame_no: u64,
}

impl Encoder {
    /// Writes the stream header. `bps` is 16 or 24; at most 8 channels; the rate fits 20 bits.
    pub fn new(out: &mut impl Write, channels: usize, sample_rate: u32, bps: u32, total: u64) -> io::Result<Self> {
        if channels == 0 || channels > 8 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("FLAC supports 1 to 8 channels, not {channels}")));
        }
        if sample_rate == 0 || sample_rate > 655_350 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("FLAC cannot store a sample rate of {sample_rate} Hz")));
        }
        if !matches!(bps, 16 | 24) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "FLAC is written as 16 or 24 bit"));
        }
        if total >= 1u64 << 36 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "too many frames for FLAC"));
        }
        let block = BLOCK_SIZE as u64; // the last block may be shorter, whatever STREAMINFO says
        let mut si = BitWriter::new();
        si.write(1, 1); // last metadata block
        si.write(0, 7); // STREAMINFO
        si.write(34, 24);
        si.write(block, 16);
        si.write(block, 16);
        si.write(0, 24); // frame sizes unknown
        si.write(0, 24);
        si.write(u64::from(sample_rate), 20);
        si.write(channels as u64 - 1, 3);
        si.write(u64::from(bps - 1), 5);
        si.write(total >> 32, 4);
        si.write(total & 0xFFFF_FFFF, 32);
        for _ in 0..4 {
            si.write(0, 32); // MD5 not computed
        }
        out.write_all(b"fLaC")?;
        out.write_all(si.bytes())?;
        Ok(Self { channels, bps, frame_no: 0 })
    }

    /// Encode one block (at most [`BLOCK_SIZE`] frames): `block[ch]` holds that channel's samples.
    pub fn write_block(&mut self, out: &mut impl Write, block: &[Vec<i64>]) -> io::Result<()> {
        let len = block.first().map_or(0, Vec::len);
        if block.len() != self.channels || len == 0 || len > BLOCK_SIZE || block.iter().any(|c| c.len() != len) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "bad FLAC block"));
        }
        let mut h = BitWriter::new();
        h.write(0x3FFE, 14);
        h.write(0, 2); // reserved, fixed block size
        h.write(7, 4); // block size - 1 follows in 16 bits
        h.write(0, 4); // sample rate from STREAMINFO
        h.write(self.channels as u64 - 1, 4); // independent channels
        h.write(if self.bps == 16 { 4 } else { 6 }, 3);
        h.write(0, 1);
        write_utf8_number(&mut h, self.frame_no);
        h.write(len as u64 - 1, 16);
        let crc = crc8(h.bytes());
        h.write(u64::from(crc), 8);
        for ch in block {
            write_subframe(&mut h, ch, self.bps);
        }
        h.align();
        let crc = crc16(h.bytes());
        out.write_all(h.bytes())?;
        out.write_all(&crc.to_be_bytes())?;
        self.frame_no += 1;
        Ok(())
    }
}
