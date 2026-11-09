// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! A minimal, dependency-free PNG encoder for the VM's `javax.imageio.ImageIO`
//! support. Emits 8-bit truecolor (RGB, color type 2) PNGs with exact pixels.
//!
//! Compression uses DEFLATE *stored* (uncompressed) blocks: the resulting file
//! is larger than a real encoder's, but it is a fully valid PNG and the pixels
//! are byte-exact. We only need visual fidelity, so this is sufficient and keeps
//! the encoder small and obviously correct (no Huffman/LZ77 machinery).

/// Encode a width×height image of packed `0xAARRGGBB` pixels (row-major) as a
/// truecolor PNG. The alpha byte is ignored (color type 2).
pub fn encode_rgb(width: u32, height: u32, pixels: &[i32]) -> Vec<u8> {
    // Raw image data: each scanline is a filter byte (0 = None) followed by RGB
    // triples. This is what the zlib stream wraps.
    let mut raw = Vec::with_capacity((height as usize) * (1 + 3 * width as usize));
    for y in 0..height {
        raw.push(0u8); // filter: None
        for x in 0..width {
            let p = pixels[(y as usize) * (width as usize) + x as usize] as u32;
            raw.push((p >> 16) as u8); // R
            raw.push((p >> 8) as u8); // G
            raw.push(p as u8); // B
        }
    }

    let zlib = zlib_store(&raw);

    let mut out = Vec::new();
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]); // signature

    // IHDR: width, height, bit depth 8, color type 2 (truecolor), no
    // compression/filter/interlace variants.
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    write_chunk(&mut out, b"IHDR", &ihdr);
    write_chunk(&mut out, b"IDAT", &zlib);
    write_chunk(&mut out, b"IEND", &[]);
    out
}

/// Wrap `data` in a zlib stream made of DEFLATE stored blocks.
fn zlib_store(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[0x78, 0x01]); // zlib header: CM=8, CINFO=7, FLEVEL=0
    let chunks: Vec<&[u8]> = if data.is_empty() {
        vec![&[]]
    } else {
        data.chunks(0xFFFF).collect()
    };
    for (i, chunk) in chunks.iter().enumerate() {
        let last = i == chunks.len() - 1;
        out.push(if last { 1 } else { 0 }); // BFINAL bit, BTYPE=00 (stored)
        let len = chunk.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes()); // NLEN = ones-complement
        out.extend_from_slice(chunk);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

/// Write a PNG chunk: length, type, data, CRC32 over type+data.
fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

/// CRC-32 (IEEE 802.3 / zlib polynomial), computed directly (no static table).
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Adler-32 checksum (the trailer of a zlib stream).
fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % MOD;
        b = (b + a) % MOD;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_known_vector() {
        // CRC-32 of "IEND" is a fixed PNG constant.
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
    }

    #[test]
    fn encodes_valid_png_header_and_size() {
        // 2x1 image: one red, one blue pixel.
        let png = encode_rgb(2, 1, &[0x00FF_0000u32 as i32, 0x0000_00FFu32 as i32]);
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        // IHDR length (13) + "IHDR".
        assert_eq!(&png[8..12], &[0, 0, 0, 13]);
        assert_eq!(&png[12..16], b"IHDR");
        // Ends with an IEND chunk.
        assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
    }
}
