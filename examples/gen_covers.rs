//! Writes N placeholder 600x900 cover JPGs for the M0 spike.
//! Usage: cargo run --release --example gen_covers -- [dir=covers] [count=2000]

use image::{Rgb, RgbImage};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let dir = args.next().unwrap_or_else(|| "covers".into());
    let count: u32 = args.next().map(|s| s.parse()).transpose()?.unwrap_or(2000);
    std::fs::create_dir_all(&dir)?;

    for i in 0..count {
        // Gradient plus per-pixel noise so JPEG size and decode cost resemble real art.
        let (r0, g0, b0) = ((i * 37 % 200) as u8, (i * 91 % 200) as u8, (i * 53 % 200) as u8);
        let mut seed = i.wrapping_mul(2_654_435_761) | 1;
        let img = RgbImage::from_fn(600, 900, |x, y| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let n = (seed & 31) as u8;
            Rgb([
                r0.wrapping_add((x / 12) as u8).wrapping_add(n),
                g0.wrapping_add((y / 18) as u8).wrapping_add(n),
                b0.wrapping_add(((x + y) / 30) as u8),
            ])
        });
        img.save(format!("{dir}/{i:05}.jpg"))?;
    }
    println!("wrote {count} covers to {dir}");
    Ok(())
}
