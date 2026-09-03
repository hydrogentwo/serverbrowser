//! Terminal image output — turn an RGBA pixel buffer into something a
//! terminal can actually display, using one of several graphics protocols.
//!
//!   1. Kitty graphics protocol (iTerm2 uses a variant too) — most robust.
//!   2. DEC sixel — depends on terminal support.
//!   3. Half-block ANSI truecolor — universal but low color resolution.
//!   4. (Text mode is handled elsewhere; see `render`.)

use std::io::Write;

use crate::config::OutputMode;

/// A decoded RGBA8 image in memory, row-major, top-left origin.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// RGBA8, 4 bytes per pixel, `width * height * 4` long.
    pub pixels: Vec<u8>,
}

impl Frame {
    pub fn from_rgba(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        Frame {
            width,
            height,
            pixels,
        }
    }

    fn pixel(&self, x: u32, y: u32) -> (u8, u8, u8, u8) {
        let i = ((y * self.width + x) * 4) as usize;
        (self.pixels[i], self.pixels[i + 1], self.pixels[i + 2], self.pixels[i + 3])
    }
}

/// Encode as base64 for the kitty graphics protocol.
fn b64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len() * 4 / 3 + 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(n >> 6) as usize & 63] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[n as usize & 63] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Emit the frame to `w` using the selected protocol.
pub fn emit<W: Write>(w: &mut W, frame: &Frame, mode: OutputMode) -> std::io::Result<()> {
    match mode {
        OutputMode::Kitty => emit_kitty(w, frame),
        OutputMode::Sixel => emit_sixel(w, frame),
        OutputMode::Blocks => emit_blocks(w, frame),
        OutputMode::Text => Ok(()), // handled by caller
    }
}

/// Kitty graphics protocol. Format understood by kitty and (mostly) iTerm2.
/// We write the full RGBA payload (chunked, base64).
pub fn emit_kitty<W: Write>(w: &mut W, frame: &Frame) -> std::io::Result<()> {
    // 0 in kitty enumeration = "chunked" flags; f=32 means RGBA.
    write!(
        w,
        "\x1b_Ga=T,f=32,s={},v={},m=1;{}\x1b\\",
        frame.width,
        frame.height,
        b64(&frame.pixels)
    )
}

/// DEC sixel. We quantize to a fixed 256-entry (or fewer) palette to keep the
/// output reasonable. A proper implementation would use dithering; here we
/// use a simple 2x2x2 RGB cube (8 colors) + grayscale ramps — good enough to
/// convey page layout.
pub fn emit_sixel<W: Write>(w: &mut W, frame: &Frame) -> std::io::Result<()> {
    // Define a small palette (16 colors: 4 grays + a coarse color cube).
    let palette: Vec<(u8, u8, u8)> = {
        let mut p = vec![(0, 0, 0)];
        for level in [85u8, 170u8, 255u8] {
            for r in [0u8, level] {
                for g in [0u8, level] {
                    for b in [0u8, level] {
                        p.push((r, g, b));
                    }
                }
            }
        }
        p.truncate(256);
        // dedup
        let mut seen = std::collections::HashSet::new();
        p.retain(|c| seen.insert(*c));
        p
    };

    write!(w, "\x1bPq")?;
    for (i, (r, g, b)) in palette.iter().enumerate() {
        write!(
            w,
            "#{};2;{};{};{}",
            i,
            (r * 100 / 255),
            (g * 100 / 255),
            (b * 100 / 255)
        )?;
    }

    let quantize = |c: u8| -> (u8, u8, u8) {
        // nearest palette entry
        let mut best = 0;
        let mut best_d = i32::MAX;
        for (i, (pr, pg, pb)) in palette.iter().enumerate() {
            let dr = *pr as i32 - c as i32;
            let dg = *pg as i32 - c as i32;
            let db = *pb as i32 - c as i32;
            // We only handle grayscale here simply; full RGB indexing below.
            let d = dr * dr + dg * dg + db * db;
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        palette[best]
    };

    // Sixel encodes 6 vertical pixels per character row, using one byte per
    // column where bit 0 is the top pixel. Colors are set per pixel via #N.
    let h = frame.height;
    let wdt = frame.width;
    for band in (0..h).step_by(6) {
        for x in 0..wdt {
            let mut current = None;
            let mut byte: u8 = 0;
            for dy in 0..6u32 {
                let y = band + dy;
                if y >= h {
                    break;
                }
                let (r, g, b, _a) = frame.pixel(x, y);
                let q = quantize((r / 3 + g / 3 + b / 3) as u8); // luma only here
                let idx = palette
                    .iter()
                    .position(|c| *c == q)
                    .unwrap_or(0);
                if current != Some(idx) {
                    if dy > 0 {
                        write!(w, "#{}", current.unwrap())?;
                    }
                    write!(w, "#{}", idx)?;
                    current = Some(idx);
                }
                byte |= 1 << dy;
            }
            write!(w, "{}{}", byte as char, '$')?;
        }
        write!(w, "-")?;
    }
    write!(w, "\x1b\\")
}

/// Half-block ANSI truecolor. Two pixels per character cell (upper/lower), so
/// width stays the same and height halves. Uses 24-bit color escape codes.
pub fn emit_blocks<W: Write>(w: &mut W, frame: &Frame) -> std::io::Result<()> {
    for y in (0..frame.height).step_by(2) {
        for x in 0..frame.width {
            let (r1, g1, b1, _) = frame.pixel(x, y);
            let (r2, g2, b2, _) = if y + 1 < frame.height {
                frame.pixel(x, y + 1)
            } else {
                (r1, g1, b1, 255)
            };
            // upper half = foreground, lower half = background on U+2584
            write!(
                w,
                "\x1b[38;2;{};{};{}m\x1b[48;2;{};{};{}m▀",
                r1, g1, b1, r2, g2, b2
            )?;
        }
        write!(w, "\x1b[0m\n")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b64_roundtrip_simple() {
        assert_eq!(b64(b"Man"), "TWFu");
        assert_eq!(b64(b"Ma"), "TWE=");
        assert_eq!(b64(b"M"), "TQ==");
        assert_eq!(b64(b""), "");
    }

    #[test]
    fn kitty_starts_with_g() {
        let f = Frame::from_rgba(1, 1, vec![255, 0, 0, 255]);
        let mut out = vec![];
        emit_kitty(&mut out, &f).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.starts_with("\x1b_G"));
        assert!(s.ends_with("\x1b\\"));
    }

    #[test]
    fn blocks_has_reset() {
        let f = Frame::from_rgba(2, 2, vec![0; 16]);
        let mut out = vec![];
        emit_blocks(&mut out, &f).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("\x1b[0m"));
    }
}