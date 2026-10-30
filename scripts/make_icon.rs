use image::{ImageBuffer, Rgba, RgbaImage};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Loading original icon.png...");
    let src_img = image::open("icon.png")?.to_rgba8();
    let (src_w, src_h) = (src_img.width() as f64, src_img.height() as f64);

    let canvas_size = 1024;
    let mut out_img: RgbaImage = ImageBuffer::new(canvas_size, canvas_size);

    // Apple Big Sur squircle geometry
    // Target squircle size: 824x824, centered at (512, 502) (slightly offset up for shadow)
    let sq_w = 824.0;
    let sq_h = 824.0;
    let sq_cx = 512.0;
    let sq_cy = 502.0;
    let sq_radius = 185.0; // Corner radius

    // Distance function to a rounded rectangle with corner radius
    // Returns signed distance (negative inside, positive outside)
    let sd_rounded_rect = |px: f64, py: f64, cx: f64, cy: f64, w: f64, h: f64, r: f64| -> f64 {
        let dx = (px - cx).abs() - (w / 2.0 - r);
        let dy = (py - cy).abs() - (h / 2.0 - r);
        let outside_x = dx.max(0.0);
        let outside_y = dy.max(0.0);
        let outside_dist = (outside_x * outside_x + outside_y * outside_y).sqrt();
        let inside_dist = dx.max(dy).min(0.0);
        outside_dist + inside_dist - r
    };

    // 1. Render Drop Shadow
    // Shadow is offset down by 22px, with blur
    let shadow_cy = sq_cy + 22.0;
    let shadow_blur = 28.0;

    for y in 0..canvas_size {
        for x in 0..canvas_size {
            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;

            let dist = sd_rounded_rect(px, py, sq_cx, shadow_cy, sq_w, sq_h, sq_radius);

            if dist < shadow_blur * 2.0 {
                // Smooth falloff from shadow boundary
                let factor = if dist <= 0.0 {
                    1.0
                } else {
                    let t = dist / shadow_blur;
                    (-0.5 * t * t).exp()
                };
                let alpha = (factor * 0.38 * 255.0).clamp(0.0, 255.0) as u8;
                if alpha > 0 {
                    out_img.put_pixel(x, y, Rgba([0, 0, 0, alpha]));
                }
            }
        }
    }

    // 2. Render Squircle Body with Original Icon Content & Anti-Aliasing
    for y in 0..canvas_size {
        for x in 0..canvas_size {
            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;

            let dist = sd_rounded_rect(px, py, sq_cx, sq_cy, sq_w, sq_h, sq_radius);

            if dist <= 1.0 {
                // Anti-aliased coverage: 1.0 inside, smooth transition at border
                let coverage = (-dist + 0.5).clamp(0.0, 1.0);

                // Map (px, py) within the squircle to src_img coordinates
                // We map [sq_cx - sq_w/2, sq_cx + sq_w/2] -> [0, src_w]
                let u = ((px - (sq_cx - sq_w / 2.0)) / sq_w).clamp(0.0, 1.0);
                let v = ((py - (sq_cy - sq_h / 2.0)) / sq_h).clamp(0.0, 1.0);

                let src_x = ((u * (src_w - 1.0)).round() as u32).min(src_img.width() - 1);
                let src_y = ((v * (src_h - 1.0)).round() as u32).min(src_img.height() - 1);

                let src_pixel = src_img.get_pixel(src_x, src_y);

                // Subtle inner top border highlight (macOS style)
                let border_dist = dist.abs();
                let is_top = py < sq_cy;
                let highlight = if border_dist < 2.0 && is_top {
                    (1.0 - border_dist / 2.0) * 0.25
                } else {
                    0.0
                };

                let r = ((src_pixel[0] as f64 * (1.0 - highlight) + 255.0 * highlight).clamp(0.0, 255.0)) as u8;
                let g = ((src_pixel[1] as f64 * (1.0 - highlight) + 255.0 * highlight).clamp(0.0, 255.0)) as u8;
                let b = ((src_pixel[2] as f64 * (1.0 - highlight) + 255.0 * highlight).clamp(0.0, 255.0)) as u8;

                let final_alpha = (coverage * src_pixel[3] as f64) as u8;

                // Alpha blend over existing shadow
                let existing = out_img.get_pixel(x, y);
                let bg_a = existing[3] as f64 / 255.0;
                let fg_a = final_alpha as f64 / 255.0;
                let out_a = fg_a + bg_a * (1.0 - fg_a);

                if out_a > 0.0 {
                    let out_r = ((r as f64 * fg_a + existing[0] as f64 * bg_a * (1.0 - fg_a)) / out_a) as u8;
                    let out_g = ((g as f64 * fg_a + existing[1] as f64 * bg_a * (1.0 - fg_a)) / out_a) as u8;
                    let out_b = ((b as f64 * fg_a + existing[2] as f64 * bg_a * (1.0 - fg_a)) / out_a) as u8;
                    out_img.put_pixel(x, y, Rgba([out_r, out_g, out_b, (out_a * 255.0) as u8]));
                }
            }
        }
    }

    // Save as output
    let out_path = "icon_squircle.png";
    out_img.save(out_path)?;
    println!("Successfully generated native macOS squircle icon: {}", out_path);

    Ok(())
}

