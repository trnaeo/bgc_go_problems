use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};
use image::{Rgb, RgbImage};

use crate::config::RenderConfig;
use crate::sgf::{Branch, Color, Point};

const FONT_BYTES: &[u8] = include_bytes!("../assets/DejaVuSans-Bold.ttf");
const COLUMN_LETTERS: &str = "ABCDEFGHJKLMNOPQRST";

type Rgbf = [f32; 3];

const WOOD: Rgbf = [226.0, 178.0, 100.0];
const LINE: Rgbf = [45.0, 32.0, 18.0];

/// Sichtbarer Ausschnitt des Bretts (Gitterindizes, inklusiv).
#[derive(Debug, Clone, Copy)]
pub struct Crop {
    pub x0: usize,
    pub x1: usize,
    pub y0: usize,
    pub y1: usize,
}

pub struct StoneMark {
    pub at: Point,
    pub color: Color,
    pub label: Option<String>,
    /// Stein wurde geschlagen: es wird nur die Nummer auf dem leeren Punkt gezeichnet.
    pub removed: bool,
}

/// Rechteck um alle Punkte plus `margin` Linien. Liegt der Rand nahe am Brettrand,
/// wird bis zum Brettrand erweitert.
pub fn compute_crop(size: usize, points: &[Point], margin: usize) -> Crop {
    let last = size - 1;
    if points.is_empty() {
        return Crop {
            x0: 0,
            x1: last,
            y0: 0,
            y1: last,
        };
    }
    let min_x = points.iter().map(|p| p.x).min().unwrap();
    let max_x = points.iter().map(|p| p.x).max().unwrap();
    let min_y = points.iter().map(|p| p.y).min().unwrap();
    let max_y = points.iter().map(|p| p.y).max().unwrap();

    let lo = |v: usize| {
        let v = v.saturating_sub(margin);
        if v <= 1 {
            0
        } else {
            v
        }
    };
    let hi = |v: usize| {
        let v = (v + margin).min(last);
        if v + 2 >= size {
            last
        } else {
            v
        }
    };
    Crop {
        x0: lo(min_x),
        x1: hi(max_x),
        y0: lo(min_y),
        y1: hi(max_y),
    }
}

fn hoshi_points(size: usize) -> Vec<Point> {
    let p = |x, y| Point { x, y };
    match size {
        // D16 K16 Q16 / D10 K10 Q10 / D4 K4 Q4
        19 => {
            let l = [3, 9, 15];
            l.iter()
                .flat_map(|&x| l.iter().map(move |&y| p(x, y)))
                .collect()
        }
        // D10 K10 / G7 / D4 K4
        13 => vec![p(3, 3), p(9, 3), p(6, 6), p(3, 9), p(9, 9)],
        9 => vec![p(2, 2), p(6, 2), p(2, 6), p(6, 6), p(4, 4)],
        _ => Vec::new(),
    }
}

fn blend(img: &mut RgbImage, x: i32, y: i32, c: Rgbf, a: f32) {
    if a <= 0.0 || x < 0 || y < 0 || x >= img.width() as i32 || y >= img.height() as i32 {
        return;
    }
    let a = a.min(1.0);
    let px = img.get_pixel_mut(x as u32, y as u32);
    for i in 0..3 {
        px.0[i] = (px.0[i] as f32 * (1.0 - a) + c[i] * a)
            .round()
            .clamp(0.0, 255.0) as u8;
    }
}

fn fill_rect(img: &mut RgbImage, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgbf) {
    for y in y0.round() as i32..y1.round() as i32 {
        for x in x0.round() as i32..x1.round() as i32 {
            blend(img, x, y, c, 1.0);
        }
    }
}

/// Anti-aliaster Kreis; `shade` liefert die Farbe pro Pixel (x, y, Abstand zum Mittelpunkt).
fn disc(
    img: &mut RgbImage,
    cx: f32,
    cy: f32,
    r: f32,
    alpha: f32,
    mut shade: impl FnMut(f32, f32, f32) -> Rgbf,
) {
    for y in (cy - r - 2.0).floor() as i32..=(cy + r + 2.0).ceil() as i32 {
        for x in (cx - r - 2.0).floor() as i32..=(cx + r + 2.0).ceil() as i32 {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
            let cov = (r - d + 0.5).clamp(0.0, 1.0);
            if cov > 0.0 {
                blend(img, x, y, shade(px, py, d), cov * alpha);
            }
        }
    }
}

fn lerp(a: Rgbf, b: Rgbf, t: f32) -> Rgbf {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn draw_stone(img: &mut RgbImage, cx: f32, cy: f32, r: f32, color: Color) {
    // Schatten
    disc(
        img,
        cx + r * 0.10,
        cy + r * 0.14,
        r * 1.06,
        0.18,
        |_, _, _| [0.0; 3],
    );
    disc(
        img,
        cx + r * 0.06,
        cy + r * 0.08,
        r * 1.01,
        0.22,
        |_, _, _| [0.0; 3],
    );

    let (hx, hy) = (cx - r * 0.35, cy - r * 0.35);
    disc(img, cx, cy, r, 1.0, |px, py, d| {
        let t = ((px - hx).powi(2) + (py - hy).powi(2)).sqrt() / (r * 1.6);
        match color {
            Color::Black => lerp([95.0; 3], [8.0; 3], t),
            Color::White => {
                let base = lerp([255.0; 3], [200.0; 3], t);
                let edge = ((d - (r - 1.6)) / 1.6).clamp(0.0, 1.0) * 0.6;
                lerp(base, [70.0; 3], edge)
            }
        }
    });
}

fn draw_text(
    img: &mut RgbImage,
    font: &FontRef,
    text: &str,
    cx: f32,
    cy: f32,
    px: f32,
    color: Rgbf,
) {
    let scale = PxScale::from(px);
    let scaled = font.as_scaled(scale);
    let mut x = 0.0;
    let mut glyphs = Vec::new();
    for ch in text.chars() {
        let id = font.glyph_id(ch);
        glyphs.push(id.with_scale_and_position(scale, point(x, 0.0)));
        x += scaled.h_advance(id);
    }

    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for g in &glyphs {
        if let Some(o) = font.outline_glyph(g.clone()) {
            let b = o.px_bounds();
            min_x = min_x.min(b.min.x);
            min_y = min_y.min(b.min.y);
            max_x = max_x.max(b.max.x);
            max_y = max_y.max(b.max.y);
        }
    }
    if min_x > max_x {
        return;
    }
    let dx = cx - (min_x + max_x) / 2.0;
    let dy = cy - (min_y + max_y) / 2.0;

    for mut g in glyphs {
        g.position = point(g.position.x + dx, g.position.y + dy);
        if let Some(o) = font.outline_glyph(g) {
            let b = o.px_bounds();
            let (ox, oy) = (b.min.x.floor() as i32, b.min.y.floor() as i32);
            o.draw(|gx, gy, cov| blend(img, ox + gx as i32, oy + gy as i32, color, cov));
        }
    }
}

pub fn render(
    size: usize,
    stones: &[StoneMark],
    branch: Option<Branch>,
    crop: Crop,
    cfg: &RenderConfig,
) -> RgbImage {
    let font = FontRef::try_from_slice(FONT_BYTES).expect("eingebettete Schrift ist gültig");
    let cell = cfg.cell_size as f32;
    let half = (cell * 0.5).round();
    let band = if cfg.coordinates {
        (cell * 0.8).round()
    } else {
        0.0
    };

    let cols = (crop.x1 - crop.x0) as f32;
    let rows = (crop.y1 - crop.y0) as f32;
    let width = half + cols * cell + half + band;
    let height = band + half + rows * cell + half;

    // Brettfläche: ohne Koordinatenband oben und rechts
    let (board_left, board_top, board_right, board_bottom) = (0.0, band, width - band, height);
    let gx = |c: usize| half + (c - crop.x0) as f32 * cell;
    let gy = |r: usize| band + half + (r - crop.y0) as f32 * cell;

    let mut img = RgbImage::new(width as u32, height as u32);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let grain = ((y as f32 * 0.9 + (x as f32 * 0.02).sin() * 6.0).sin()) * 4.0;
        *px = Rgb([
            (WOOD[0] + grain).clamp(0.0, 255.0) as u8,
            (WOOD[1] + grain).clamp(0.0, 255.0) as u8,
            (WOOD[2] + grain).clamp(0.0, 255.0) as u8,
        ]);
    }
    // Unbeschriftetes Holz, um Gitterlinien hinter Nummern geschlagener Steine zu entfernen
    let wood = img.clone();

    let thin = (cell / 28.0).round().max(1.0);
    let thick = thin * 2.0;
    let last = size - 1;
    let top_is_edge = crop.y0 == 0;
    let bottom_is_edge = crop.y1 == last;
    let left_is_edge = crop.x0 == 0;
    let right_is_edge = crop.x1 == last;

    // Senkrechte Linien
    for c in crop.x0..=crop.x1 {
        let t = if c == 0 || c == last { thick } else { thin };
        let y_from = if top_is_edge {
            gy(0) - thick / 2.0
        } else {
            board_top
        };
        let y_to = if bottom_is_edge {
            gy(crop.y1) + thick / 2.0
        } else {
            board_bottom
        };
        fill_rect(
            &mut img,
            gx(c) - t / 2.0,
            y_from,
            gx(c) + t / 2.0,
            y_to,
            LINE,
        );
    }
    // Waagrechte Linien
    for r in crop.y0..=crop.y1 {
        let t = if r == 0 || r == last { thick } else { thin };
        let x_from = if left_is_edge {
            gx(0) - thick / 2.0
        } else {
            board_left
        };
        let x_to = if right_is_edge {
            gx(crop.x1) + thick / 2.0
        } else {
            board_right
        };
        fill_rect(
            &mut img,
            x_from,
            gy(r) - t / 2.0,
            x_to,
            gy(r) + t / 2.0,
            LINE,
        );
    }

    // Hoshi
    for p in hoshi_points(size) {
        if (crop.x0..=crop.x1).contains(&p.x) && (crop.y0..=crop.y1).contains(&p.y) {
            disc(&mut img, gx(p.x), gy(p.y), cell * 0.1, 1.0, |_, _, _| LINE);
        }
    }

    // Koordinaten: Buchstaben oben, Zahlen rechts
    if cfg.coordinates {
        let letters: Vec<char> = COLUMN_LETTERS.chars().collect();
        for c in crop.x0..=crop.x1 {
            if let Some(l) = letters.get(c) {
                draw_text(
                    &mut img,
                    &font,
                    &l.to_string(),
                    gx(c),
                    band / 2.0,
                    cell * 0.42,
                    LINE,
                );
            }
        }
        for r in crop.y0..=crop.y1 {
            draw_text(
                &mut img,
                &font,
                &(size - r).to_string(),
                width - band / 2.0,
                gy(r),
                cell * 0.42,
                LINE,
            );
        }
    }

    // Steine und Beschriftungen
    let radius = cell * 0.47;
    for s in stones {
        let (cx, cy) = (gx(s.at.x), gy(s.at.y));
        if s.removed {
            // Gitterlinien hinter der Nummer entfernen
            disc(&mut img, cx, cy, radius * 0.8, 1.0, |px, py, _| {
                let x = (px as u32).min(wood.width() - 1);
                let y = (py as u32).min(wood.height() - 1);
                let p = wood.get_pixel(x, y).0;
                [p[0] as f32, p[1] as f32, p[2] as f32]
            });
        } else {
            draw_stone(&mut img, cx, cy, radius, s.color);
        }
    }
    for s in stones {
        if let Some(label) = &s.label {
            // Auf dem Abzweigpunkt wird die Nummer kleiner und nach unten versetzt,
            // damit oben Platz für das Dreieck mit der Abzweig-Nummer bleibt.
            let on_branch = branch.is_some_and(|b| b.at == s.at);
            let (scale, dy) = if on_branch {
                (0.74, radius * 0.27)
            } else {
                (1.0, 0.0)
            };
            let px = scale
                * if label.chars().count() > 1 {
                    cell * 0.42
                } else {
                    cell * 0.55
                };
            let color = match (s.removed, s.color) {
                (true, _) => LINE,
                (false, Color::Black) => [255.0; 3],
                (false, Color::White) => [20.0; 3],
            };
            draw_text(
                &mut img,
                &font,
                label,
                gx(s.at.x),
                gy(s.at.y) + dy,
                px,
                color,
            );
        }
    }

    // Abzweigung der 2. Lösung: Dreieck mit Zugnummer ("▲ 2").
    // Schwarz; auf schwarzem Stein weiss. Mit Nummer auf dem Punkt sitzt es oben im Stein,
    // sonst in der Mitte.
    if let Some(b) = branch {
        let (cx, cy) = (gx(b.at.x), gy(b.at.y));
        let at_point = stones.iter().find(|s| s.at == b.at);
        let on_black = at_point.is_some_and(|s| s.color == Color::Black && !s.removed);
        let color = if on_black { [255.0; 3] } else { [0.0; 3] };
        let (tri_r, text_px, cy) = if at_point.is_some_and(|s| s.label.is_some()) {
            (cell * 0.12, cell * 0.34, cy - radius * 0.58)
        } else {
            (cell * 0.17, cell * 0.40, cy)
        };
        draw_branch_mark(
            &mut img,
            &font,
            cx,
            cy,
            tri_r,
            text_px,
            &b.number.to_string(),
            color,
        );
    }

    img
}

fn text_width(font: &FontRef, text: &str, px: f32) -> f32 {
    let scaled = font.as_scaled(PxScale::from(px));
    text.chars()
        .map(|c| scaled.h_advance(font.glyph_id(c)))
        .sum()
}

/// Dreieck gefolgt von einer Nummer, als Einheit um (cx, cy) zentriert.
#[allow(clippy::too_many_arguments)]
fn draw_branch_mark(
    img: &mut RgbImage,
    font: &FontRef,
    cx: f32,
    cy: f32,
    tri_r: f32,
    text_px: f32,
    text: &str,
    color: Rgbf,
) {
    let tri_w = tri_r * 3f32.sqrt();
    let gap = text_px * 0.2;
    let text_w = text_width(font, text, text_px);
    let left = cx - (tri_w + gap + text_w) / 2.0;
    // fill_triangle erwartet den Umkreismittelpunkt; optischer Schwerpunkt liegt r/4 darüber
    fill_triangle(img, left + tri_w / 2.0, cy + tri_r / 4.0, tri_r, color);
    draw_text(
        img,
        font,
        text,
        left + tri_w + gap + text_w / 2.0,
        cy,
        text_px,
        color,
    );
}

/// Anti-aliasing per 4x4-Supersampling; gleichseitiges Dreieck mit Spitze nach oben.
fn fill_triangle(img: &mut RgbImage, cx: f32, cy: f32, r: f32, color: Rgbf) {
    // Umkreisradius r: Spitze bei cy - r, Basis bei cy + r/2
    let half_base = r * 3f32.sqrt() / 2.0;
    let (top, bottom) = (cy - r, cy + r / 2.0);
    let inside = |x: f32, y: f32| {
        if y < top || y > bottom {
            return false;
        }
        let w = half_base * (y - top) / (bottom - top);
        (x - cx).abs() <= w
    };
    const N: i32 = 4;
    for y in (top.floor() as i32 - 1)..=(bottom.ceil() as i32 + 1) {
        for x in ((cx - half_base).floor() as i32 - 1)..=((cx + half_base).ceil() as i32 + 1) {
            let mut hits = 0;
            for sy in 0..N {
                for sx in 0..N {
                    let fx = x as f32 + (sx as f32 + 0.5) / N as f32;
                    let fy = y as f32 + (sy as f32 + 0.5) / N as f32;
                    if inside(fx, fy) {
                        hits += 1;
                    }
                }
            }
            blend(img, x, y, color, hits as f32 / (N * N) as f32);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(size: usize) -> Vec<String> {
        let letters: Vec<char> = COLUMN_LETTERS.chars().collect();
        let mut v: Vec<String> = hoshi_points(size)
            .iter()
            .map(|p| format!("{}{}", letters[p.x], size - p.y))
            .collect();
        v.sort();
        v
    }

    fn expected(list: &[&str]) -> Vec<String> {
        let mut v: Vec<String> = list.iter().map(|s| s.to_string()).collect();
        v.sort();
        v
    }

    #[test]
    fn hoshi_19() {
        assert_eq!(
            names(19),
            expected(&["D16", "K16", "Q16", "D10", "K10", "Q10", "D4", "K4", "Q4"])
        );
    }

    #[test]
    fn hoshi_13() {
        assert_eq!(names(13), expected(&["D10", "K10", "G7", "D4", "K4"]));
    }

    #[test]
    fn hoshi_9() {
        assert_eq!(names(9), expected(&["C7", "G7", "E5", "C3", "G3"]));
    }
}
