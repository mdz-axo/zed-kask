//! Splash screen — faithful terminal reproduction of the Kask SVG logo.
//!
//! Rasterizes the SVG geometry (`assets/kask-logo.svg`) into a pixel buffer
//! and renders it using Unicode half-block characters (`▀ ▄ █`). The four
//! compositional elements are preserved:
//!
//! 1. **Vintage galvanized milk can** — cylindrical body, neck, lid with knob, side handles, ribbing
//! 2. **Calligraphic stroke variation** — thick (2px) downstrokes, thin (1px) transitions
//! 3. **Curator's Eye** — almond eyelids, filled iris, pupil, white reflection, operator gap
//! 4. **Bitemporal shadow** — same geometry offset left at reduced opacity
//!
//! The pixel buffer is computed once from the SVG coordinates and cached.

use crossterm::event::{self, Event};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use std::time::{Duration, Instant};

/// Duration to display the splash screen before auto-transitioning.
const SPLASH_DURATION_MS: u64 = 2500;

// ── Pixel buffer constants ──────────────────────────────────────────
//
// The SVG viewBox is 400×600. We rasterize at scale 0.2 → 80×120 pixels.
// Each terminal row = 2 pixel rows (half-block rendering), so 60 terminal rows.
//
// Pixel values:
//   0 = background (transparent)
//   1 = black tracing (behind strokes)
//   2 = shadow stroke (15% opacity → dim gray)
//   3 = highlight (white — eye reflection, operator gap)
//   4 = steel blue highlight (#4682B4)
//   5 = dark steel shadow (#2C3E50)
//   6 = main stroke (#1A1A1A → near-black)
//   7 = steel blue highlight (#4682B4) — subtle accent

const CANVAS_W: usize = 80;
const CANVAS_H: usize = 120;
const SCALE: f64 = 0.2;

/// Pre-computed pixel buffer for the Kask logo.
/// Computed once via `build_logo_buffer()`.
fn build_logo_buffer() -> Vec<u8> {
    let mut buf = vec![0u8; CANVAS_W * CANVAS_H];

    // ── Helper: scale SVG coordinate to canvas coordinate ──
    let sx = |x: f64| -> f64 { x * SCALE };
    let sy = |y: f64| -> f64 { y * SCALE };

    // ── BITEMPORAL SHADOW (drawn first, so main strokes overlay) ──
    // Shadow offset
    let so = -25.0;

    // Shadow body rect: x=140 y=180 w=120 h=280
    draw_rect_stroke(
        &mut buf,
        sx(140.0 + so),
        sy(180.0),
        sx(120.0),
        sy(280.0),
        1,
        2,
    );
    // Shadow neck rect: x=160 y=140 w=80 h=45
    draw_rect_stroke(
        &mut buf,
        sx(160.0 + so),
        sy(140.0),
        sx(80.0),
        sy(45.0),
        1,
        2,
    );
    // Shadow lid ellipse (approximate with rect)
    draw_rect_stroke(
        &mut buf,
        sx(155.0 + so),
        sy(125.0),
        sx(90.0),
        sy(20.0),
        1,
        2,
    );
    // Shadow left handle
    draw_cubic_bezier(
        &mut buf,
        sx(140.0 + so),
        sy(220.0),
        sx(115.0 + so),
        sy(220.0),
        sx(115.0 + so),
        sy(280.0),
        sx(140.0 + so),
        sy(280.0),
        1,
        2,
    );
    // Shadow right handle
    draw_cubic_bezier(
        &mut buf,
        sx(260.0 + so),
        sy(220.0),
        sx(285.0 + so),
        sy(220.0),
        sx(285.0 + so),
        sy(280.0),
        sx(260.0 + so),
        sy(280.0),
        1,
        2,
    );

    // ── MAIN MILK CAN BODY ──

    // Left side — thick downstroke: M 140 180 L 140 460, width 9
    draw_thick_line(&mut buf, sx(140.0), sy(180.0), sx(140.0), sy(460.0), 2, 6);
    // Left side black tracing (behind)
    draw_thick_line(&mut buf, sx(139.0), sy(180.0), sx(139.0), sy(460.0), 1, 1);
    // Left side steel blue highlight
    draw_thick_line(&mut buf, sx(143.0), sy(185.0), sx(143.0), sy(455.0), 1, 4);
    // Right side — thick downstroke: M 260 180 L 260 460, width 9
    draw_thick_line(&mut buf, sx(260.0), sy(180.0), sx(260.0), sy(460.0), 2, 6);
    // Right side black tracing (behind)
    draw_thick_line(&mut buf, sx(261.0), sy(180.0), sx(261.0), sy(460.0), 1, 1);
    // Right side dark steel shadow
    draw_thick_line(&mut buf, sx(257.0), sy(185.0), sx(257.0), sy(455.0), 1, 5);
    // Bottom — thick curved base: M 140 460 Q 140 470 200 470 Q 260 470 260 460, width 9
    draw_quad_bezier(
        &mut buf,
        sx(140.0),
        sy(460.0),
        sx(140.0),
        sy(470.0),
        sx(200.0),
        sy(470.0),
        2,
        6,
    );
    draw_quad_bezier(
        &mut buf,
        sx(200.0),
        sy(470.0),
        sx(260.0),
        sy(470.0),
        sx(260.0),
        sy(460.0),
        2,
        6,
    );
    // Bottom black tracing (behind)
    draw_quad_bezier(
        &mut buf,
        sx(140.0),
        sy(461.0),
        sx(140.0),
        sy(471.0),
        sx(200.0),
        sy(471.0),
        1,
        1,
    );
    draw_quad_bezier(
        &mut buf,
        sx(200.0),
        sy(471.0),
        sx(260.0),
        sy(471.0),
        sx(260.0),
        sy(461.0),
        1,
        1,
    );
    // Bottom dark steel shadow
    draw_quad_bezier(
        &mut buf,
        sx(145.0),
        sy(462.0),
        sx(145.0),
        sy(468.0),
        sx(200.0),
        sy(468.0),
        1,
        5,
    );
    draw_quad_bezier(
        &mut buf,
        sx(200.0),
        sy(468.0),
        sx(255.0),
        sy(468.0),
        sx(255.0),
        sy(462.0),
        1,
        5,
    );
    // Top shoulder — thinner transition: M 140 180 Q 200 175 260 180, width 7
    draw_quad_bezier(
        &mut buf,
        sx(140.0),
        sy(180.0),
        sx(200.0),
        sy(175.0),
        sx(260.0),
        sy(180.0),
        1,
        6,
    );
    // Top shoulder black tracing (behind)
    draw_quad_bezier(
        &mut buf,
        sx(140.0),
        sy(181.0),
        sx(200.0),
        sy(176.0),
        sx(260.0),
        sy(181.0),
        1,
        1,
    );
    // Top shoulder steel blue highlight
    draw_quad_bezier(
        &mut buf,
        sx(145.0),
        sy(178.0),
        sx(200.0),
        sy(173.0),
        sx(255.0),
        sy(178.0),
        1,
        4,
    );

    // ── GALVANIZED RIBBING ──
    // Upper rib: M 140 220 Q 200 218 260 220, width 5
    draw_quad_bezier(
        &mut buf,
        sx(140.0),
        sy(220.0),
        sx(200.0),
        sy(218.0),
        sx(260.0),
        sy(220.0),
        1,
        6,
    );
    // Upper rib black tracing (behind)
    draw_quad_bezier(
        &mut buf,
        sx(140.0),
        sy(221.0),
        sx(200.0),
        sy(219.0),
        sx(260.0),
        sy(221.0),
        1,
        1,
    );
    // Upper rib steel blue highlight
    draw_quad_bezier(
        &mut buf,
        sx(145.0),
        sy(219.0),
        sx(200.0),
        sy(217.0),
        sx(255.0),
        sy(219.0),
        1,
        4,
    );
    // Middle rib: M 140 280 Q 200 278 260 280, width 5
    draw_quad_bezier(
        &mut buf,
        sx(140.0),
        sy(280.0),
        sx(200.0),
        sy(278.0),
        sx(260.0),
        sy(280.0),
        1,
        6,
    );
    // Middle rib black tracing (behind)
    draw_quad_bezier(
        &mut buf,
        sx(140.0),
        sy(281.0),
        sx(200.0),
        sy(279.0),
        sx(260.0),
        sy(281.0),
        1,
        1,
    );
    // Middle rib steel blue highlight
    draw_quad_bezier(
        &mut buf,
        sx(145.0),
        sy(279.0),
        sx(200.0),
        sy(277.0),
        sx(255.0),
        sy(279.0),
        1,
        4,
    );
    // Lower rib: M 140 400 Q 200 398 260 400, width 5
    draw_quad_bezier(
        &mut buf,
        sx(140.0),
        sy(400.0),
        sx(200.0),
        sy(398.0),
        sx(260.0),
        sy(400.0),
        1,
        6,
    );
    // Lower rib black tracing (behind)
    draw_quad_bezier(
        &mut buf,
        sx(140.0),
        sy(401.0),
        sx(200.0),
        sy(399.0),
        sx(260.0),
        sy(401.0),
        1,
        1,
    );
    // Lower rib steel blue highlight
    draw_quad_bezier(
        &mut buf,
        sx(145.0),
        sy(399.0),
        sx(200.0),
        sy(397.0),
        sx(255.0),
        sy(399.0),
        1,
        4,
    );
    // Bottom rib: M 140 440 Q 200 438 260 440, width 5
    draw_quad_bezier(
        &mut buf,
        sx(140.0),
        sy(440.0),
        sx(200.0),
        sy(438.0),
        sx(260.0),
        sy(440.0),
        1,
        6,
    );
    // Bottom rib black tracing (behind)
    draw_quad_bezier(
        &mut buf,
        sx(140.0),
        sy(441.0),
        sx(200.0),
        sy(439.0),
        sx(260.0),
        sy(441.0),
        1,
        1,
    );
    // Bottom rib steel blue highlight
    draw_quad_bezier(
        &mut buf,
        sx(145.0),
        sy(439.0),
        sx(200.0),
        sy(437.0),
        sx(255.0),
        sy(439.0),
        1,
        4,
    );

    // ── SHOULDER ──
    // Left shoulder curve: M 140 180 C 140 165 160 155 160 145, width 8
    draw_cubic_bezier(
        &mut buf,
        sx(140.0),
        sy(180.0),
        sx(140.0),
        sy(165.0),
        sx(160.0),
        sy(155.0),
        sx(160.0),
        sy(145.0),
        2,
        6,
    );
    // Left shoulder black tracing (behind)
    draw_cubic_bezier(
        &mut buf,
        sx(139.0),
        sy(180.0),
        sx(139.0),
        sy(165.0),
        sx(159.0),
        sy(155.0),
        sx(159.0),
        sy(145.0),
        1,
        1,
    );
    // Left shoulder steel blue highlight
    draw_cubic_bezier(
        &mut buf,
        sx(142.0),
        sy(178.0),
        sx(142.0),
        sy(164.0),
        sx(161.0),
        sy(154.0),
        sx(161.0),
        sy(145.0),
        1,
        4,
    );
    // Right shoulder curve: M 260 180 C 260 165 240 155 240 145, width 8
    draw_cubic_bezier(
        &mut buf,
        sx(260.0),
        sy(180.0),
        sx(260.0),
        sy(165.0),
        sx(240.0),
        sy(155.0),
        sx(240.0),
        sy(145.0),
        2,
        6,
    );
    // Right shoulder black tracing (behind)
    draw_cubic_bezier(
        &mut buf,
        sx(261.0),
        sy(180.0),
        sx(261.0),
        sy(165.0),
        sx(241.0),
        sy(155.0),
        sx(241.0),
        sy(145.0),
        1,
        1,
    );
    // Right shoulder dark steel shadow
    draw_cubic_bezier(
        &mut buf,
        sx(258.0),
        sy(178.0),
        sx(258.0),
        sy(164.0),
        sx(239.0),
        sy(154.0),
        sx(239.0),
        sy(145.0),
        1,
        5,
    );

    // ── NECK ──
    // Left neck: M 160 140 L 160 145, width 8
    draw_thick_line(&mut buf, sx(160.0), sy(140.0), sx(160.0), sy(145.0), 2, 6);
    // Left neck black tracing
    draw_thick_line(&mut buf, sx(159.0), sy(140.0), sx(159.0), sy(145.0), 1, 1);
    // Right neck: M 240 140 L 240 145, width 8
    draw_thick_line(&mut buf, sx(240.0), sy(140.0), sx(240.0), sy(145.0), 2, 6);
    // Right neck black tracing
    draw_thick_line(&mut buf, sx(241.0), sy(140.0), sx(241.0), sy(145.0), 1, 1);

    // ── RIM ──
    // M 155 140 C 155 133 200 128 245 140 C 245 147 200 152 155 140, width 6
    draw_cubic_bezier(
        &mut buf,
        sx(155.0),
        sy(140.0),
        sx(155.0),
        sy(133.0),
        sx(200.0),
        sy(128.0),
        sx(245.0),
        sy(140.0),
        1,
        6,
    );
    draw_cubic_bezier(
        &mut buf,
        sx(245.0),
        sy(140.0),
        sx(245.0),
        sy(147.0),
        sx(200.0),
        sy(152.0),
        sx(155.0),
        sy(140.0),
        1,
        6,
    );
    // Rim black tracing (behind)
    draw_cubic_bezier(
        &mut buf,
        sx(155.0),
        sy(141.0),
        sx(155.0),
        sy(134.0),
        sx(200.0),
        sy(129.0),
        sx(245.0),
        sy(141.0),
        1,
        1,
    );
    draw_cubic_bezier(
        &mut buf,
        sx(245.0),
        sy(141.0),
        sx(245.0),
        sy(148.0),
        sx(200.0),
        sy(153.0),
        sx(155.0),
        sy(141.0),
        1,
        1,
    );

    // ── LID ──
    // Ellipse cx=200 cy=135 rx=45 ry=10 (approximate with rect)
    draw_rect_stroke(&mut buf, sx(155.0), sy(125.0), sx(90.0), sy(20.0), 1, 6);
    // Lid black tracing
    draw_rect_stroke(&mut buf, sx(155.0), sy(126.0), sx(90.0), sy(20.0), 1, 1);

    // ── LID KNOB ──
    // M 192 125 Q 200 118 208 125, width 7
    draw_quad_bezier(
        &mut buf,
        sx(192.0),
        sy(125.0),
        sx(200.0),
        sy(118.0),
        sx(208.0),
        sy(125.0),
        2,
        6,
    );
    // Lid knob black tracing
    draw_quad_bezier(
        &mut buf,
        sx(192.0),
        sy(126.0),
        sx(200.0),
        sy(119.0),
        sx(208.0),
        sy(126.0),
        1,
        1,
    );

    // ── SIDE HANDLES ──
    // Left handle: M 140 220 C 115 220 110 250 115 270 C 118 280 128 285 140 280, width 7
    draw_cubic_bezier(
        &mut buf,
        sx(140.0),
        sy(220.0),
        sx(115.0),
        sy(220.0),
        sx(110.0),
        sy(250.0),
        sx(115.0),
        sy(270.0),
        1,
        6,
    );
    draw_cubic_bezier(
        &mut buf,
        sx(115.0),
        sy(270.0),
        sx(118.0),
        sy(280.0),
        sx(128.0),
        sy(285.0),
        sx(140.0),
        sy(280.0),
        1,
        6,
    );
    // Left handle black tracing
    draw_cubic_bezier(
        &mut buf,
        sx(140.0),
        sy(221.0),
        sx(115.0),
        sy(221.0),
        sx(110.0),
        sy(251.0),
        sx(115.0),
        sy(271.0),
        1,
        1,
    );
    draw_cubic_bezier(
        &mut buf,
        sx(115.0),
        sy(271.0),
        sx(118.0),
        sy(281.0),
        sx(128.0),
        sy(286.0),
        sx(140.0),
        sy(281.0),
        1,
        1,
    );
    // Right handle: M 260 220 C 285 220 290 250 285 270 C 282 280 272 285 260 280, width 7
    draw_cubic_bezier(
        &mut buf,
        sx(260.0),
        sy(220.0),
        sx(285.0),
        sy(220.0),
        sx(290.0),
        sy(250.0),
        sx(285.0),
        sy(270.0),
        1,
        6,
    );
    draw_cubic_bezier(
        &mut buf,
        sx(285.0),
        sy(270.0),
        sx(282.0),
        sy(280.0),
        sx(272.0),
        sy(285.0),
        sx(260.0),
        sy(280.0),
        1,
        6,
    );
    // Right handle black tracing
    draw_cubic_bezier(
        &mut buf,
        sx(260.0),
        sy(221.0),
        sx(285.0),
        sy(221.0),
        sx(290.0),
        sy(251.0),
        sx(285.0),
        sy(271.0),
        1,
        1,
    );
    draw_cubic_bezier(
        &mut buf,
        sx(285.0),
        sy(271.0),
        sx(282.0),
        sy(281.0),
        sx(272.0),
        sy(286.0),
        sx(260.0),
        sy(281.0),
        1,
        1,
    );

    // ── CURATOR'S EYE ──
    // Upper eyelid — thick: M 165 330 Q 200 312 235 330, width 8
    draw_quad_bezier(
        &mut buf,
        sx(165.0),
        sy(330.0),
        sx(200.0),
        sy(312.0),
        sx(235.0),
        sy(330.0),
        2,
        6,
    );
    // Upper eyelid black tracing
    draw_quad_bezier(
        &mut buf,
        sx(165.0),
        sy(331.0),
        sx(200.0),
        sy(313.0),
        sx(235.0),
        sy(331.0),
        1,
        1,
    );
    // Upper eyelid steel blue highlight
    draw_quad_bezier(
        &mut buf,
        sx(170.0),
        sy(328.0),
        sx(200.0),
        sy(314.0),
        sx(230.0),
        sy(328.0),
        1,
        4,
    );
    // Lower eyelid — thinner: M 168 352 Q 200 372 232 352, width 5
    draw_quad_bezier(
        &mut buf,
        sx(168.0),
        sy(352.0),
        sx(200.0),
        sy(372.0),
        sx(232.0),
        sy(352.0),
        1,
        6,
    );
    // Lower eyelid black tracing
    draw_quad_bezier(
        &mut buf,
        sx(168.0),
        sy(353.0),
        sx(200.0),
        sy(373.0),
        sx(232.0),
        sy(353.0),
        1,
        1,
    );
    // Lower eyelid dark steel shadow
    draw_quad_bezier(
        &mut buf,
        sx(172.0),
        sy(353.0),
        sx(200.0),
        sy(370.0),
        sx(228.0),
        sy(353.0),
        1,
        5,
    );

    // Iris — solid fill with Eye of the Tiger: cx=200 cy=342 r=20
    draw_filled_circle(&mut buf, sx(200.0), sy(342.0), sx(20.0), 6);
    // Iris black tracing (outer edge)
    draw_filled_circle(&mut buf, sx(200.0), sy(342.0), sx(21.0), 1);
    // Iris steel blue ring (outer)
    draw_filled_circle(&mut buf, sx(200.0), sy(342.0), sx(18.0), 4);
    // Iris dark steel ring (inner)
    draw_filled_circle(&mut buf, sx(200.0), sy(342.0), sx(15.0), 5);
    // Pupil — darker with Eye of the Tiger: cx=200 cy=342 r=11
    draw_filled_circle(&mut buf, sx(200.0), sy(342.0), sx(11.0), 6);
    // Pupil black tracing
    draw_filled_circle(&mut buf, sx(200.0), sy(342.0), sx(12.0), 1);
    // Pupil steel blue inner ring
    draw_filled_circle(&mut buf, sx(200.0), sy(342.0), sx(9.0), 4);
    // Light reflection — white with steel blue glow: cx=206 cy=336 r=5
    draw_filled_circle(&mut buf, sx(206.0), sy(336.0), sx(6.0), 4);
    draw_filled_circle(&mut buf, sx(206.0), sy(336.0), sx(5.0), 3);

    // Eyelashes — subtle flicks with Eye of the Tiger
    draw_thick_line(&mut buf, sx(178.0), sy(328.0), sx(175.0), sy(321.0), 1, 6);
    draw_thick_line(&mut buf, sx(190.0), sy(324.0), sx(188.0), sy(316.0), 1, 6);
    draw_thick_line(&mut buf, sx(210.0), sy(324.0), sx(212.0), sy(316.0), 1, 6);
    draw_thick_line(&mut buf, sx(222.0), sy(328.0), sx(225.0), sy(321.0), 1, 6);
    // Eyelashes black tracing
    draw_thick_line(&mut buf, sx(178.0), sy(329.0), sx(175.0), sy(322.0), 1, 1);
    draw_thick_line(&mut buf, sx(190.0), sy(325.0), sx(188.0), sy(317.0), 1, 1);
    draw_thick_line(&mut buf, sx(210.0), sy(325.0), sx(212.0), sy(317.0), 1, 1);
    draw_thick_line(&mut buf, sx(222.0), sy(329.0), sx(225.0), sy(322.0), 1, 1);

    // Operator Authority Gap — white arc: M 209 333 A 20 20 0 0 1 211 351, width 3
    draw_thick_line(&mut buf, sx(209.0), sy(333.0), sx(211.0), sy(351.0), 1, 3);
    // Operator gap steel blue accent
    draw_thick_line(&mut buf, sx(210.0), sy(334.0), sx(212.0), sy(350.0), 1, 4);

    buf
}

// ── Rasterization primitives ────────────────────────────────────────

fn set_pixel(buf: &mut [u8], x: f64, y: f64, value: u8) {
    let ix = x.round() as i32;
    let iy = y.round() as i32;
    if ix >= 0 && (ix as usize) < CANVAS_W && iy >= 0 && (iy as usize) < CANVAS_H {
        let idx = iy as usize * CANVAS_W + ix as usize;
        // Main stroke (1) and highlight (3) override shadow (2); shadow doesn't override main
        if value == 1 || value == 3 || buf[idx] == 0 {
            buf[idx] = value;
        }
    }
}

/// Bresenham line with thickness (draws parallel offset lines for width > 1).
fn draw_thick_line(buf: &mut [u8], x0: f64, y0: f64, x1: f64, y1: f64, width: i32, value: u8) {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.001 {
        set_pixel(buf, x0, y0, value);
        return;
    }
    // Normal vector perpendicular to line
    let nx = -dy / len;
    let ny = dx / len;
    let half_w = (width as f64) / 2.0;

    for w in 0..width {
        let offset = (w as f64) - half_w + 0.5;
        let ox = nx * offset;
        let oy = ny * offset;
        draw_line_bresenham(buf, x0 + ox, y0 + oy, x1 + ox, y1 + oy, value);
    }
}

fn draw_line_bresenham(buf: &mut [u8], x0: f64, y0: f64, x1: f64, y1: f64, value: u8) {
    let ix0 = x0.round() as i32;
    let iy0 = y0.round() as i32;
    let ix1 = x1.round() as i32;
    let iy1 = y1.round() as i32;

    let dx = (ix1 - ix0).abs();
    let dy = -(iy1 - iy0).abs();
    let sx: i32 = if ix0 < ix1 { 1 } else { -1 };
    let sy: i32 = if iy0 < iy1 { 1 } else { -1 };
    let mut err = dx + dy;
    let mut x = ix0;
    let mut y = iy0;

    loop {
        if x >= 0 && (x as usize) < CANVAS_W && y >= 0 && (y as usize) < CANVAS_H {
            let idx = y as usize * CANVAS_W + x as usize;
            if value == 1 || value == 3 || buf[idx] == 0 {
                buf[idx] = value;
            }
        }
        if x == ix1 && y == iy1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// Quadratic Bezier — subdivide into line segments.
fn draw_quad_bezier(
    buf: &mut [u8],
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    width: i32,
    value: u8,
) {
    let steps = 40;
    let mut prev_x = x0;
    let mut prev_y = y0;
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let mt = 1.0 - t;
        let px = mt * mt * x0 + 2.0 * mt * t * x1 + t * t * x2;
        let py = mt * mt * y0 + 2.0 * mt * t * y1 + t * t * y2;
        draw_thick_line(buf, prev_x, prev_y, px, py, width, value);
        prev_x = px;
        prev_y = py;
    }
}

/// Cubic Bezier — subdivide into line segments.
fn draw_cubic_bezier(
    buf: &mut [u8],
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    x3: f64,
    y3: f64,
    width: i32,
    value: u8,
) {
    let steps = 50;
    let mut prev_x = x0;
    let mut prev_y = y0;
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let mt = 1.0 - t;
        let px =
            mt * mt * mt * x0 + 3.0 * mt * mt * t * x1 + 3.0 * mt * t * t * x2 + t * t * t * x3;
        let py =
            mt * mt * mt * y0 + 3.0 * mt * mt * t * y1 + 3.0 * mt * t * t * y2 + t * t * t * y3;
        draw_thick_line(buf, prev_x, prev_y, px, py, width, value);
        prev_x = px;
        prev_y = py;
    }
}

/// Filled circle using midpoint algorithm.
fn draw_filled_circle(buf: &mut [u8], cx: f64, cy: f64, r: f64, value: u8) {
    let r_int = r.round() as i32;
    let cx_int = cx.round() as i32;
    let cy_int = cy.round() as i32;
    for dy in -r_int..=r_int {
        for dx in -r_int..=r_int {
            if dx * dx + dy * dy <= r_int * r_int {
                let px = cx_int + dx;
                let py = cy_int + dy;
                if px >= 0 && (px as usize) < CANVAS_W && py >= 0 && (py as usize) < CANVAS_H {
                    let idx = py as usize * CANVAS_W + px as usize;
                    if value == 1 || value == 3 || buf[idx] == 0 {
                        buf[idx] = value;
                    }
                }
            }
        }
    }
}

/// Stroked rectangle (outline only).
fn draw_rect_stroke(buf: &mut [u8], x: f64, y: f64, w: f64, h: f64, width: i32, value: u8) {
    // Top
    draw_thick_line(buf, x, y, x + w, y, width, value);
    // Bottom
    draw_thick_line(buf, x, y + h, x + w, y + h, width, value);
    // Left
    draw_thick_line(buf, x, y, x, y + h, width, value);
    // Right
    draw_thick_line(buf, x + w, y, x + w, y + h, width, value);
}

// ── Splash screen widget ────────────────────────────────────────────

/// Kask logo splash screen — faithful reproduction of `kask-logo.svg`.
pub struct SplashScreen {
    start_time: Instant,
    duration: Duration,
    buffer: Vec<u8>,
}

impl SplashScreen {
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            duration: Duration::from_millis(SPLASH_DURATION_MS),
            buffer: build_logo_buffer(),
        }
    }

    /// Check if the splash screen should auto-dismiss.
    pub fn should_dismiss(&self) -> bool {
        self.start_time.elapsed() >= self.duration
    }

    /// Check for early dismissal via key press.
    pub fn check_early_dismiss(&mut self) -> bool {
        if event::poll(Duration::from_millis(16)).unwrap_or(false) {
            if let Ok(Event::Key(_)) = event::read() {
                return true;
            }
        }
        false
    }

    /// Render the splash screen into the given frame.
    pub fn render(&self, f: &mut Frame) {
        let area = f.area();

        // Clear to dark background
        let bg = Block::default().style(Style::default().bg(Color::Rgb(11, 12, 21)));
        f.render_widget(bg, area);

        // The pixel buffer is CANVAS_W × CANVAS_H.
        // With half-block rendering, terminal rows = CANVAS_H / 2.
        let term_rows = CANVAS_H / 2;
        let term_cols = CANVAS_W;

        // Center the logo in the terminal
        let x_off = area.x + area.width.saturating_sub(term_cols as u16) / 2;
        // The logo + wordmark + prompt is (term_rows + 3) rows tall.
        // Clamp to fit within the terminal — on very small terminals the
        // splash is truncated rather than panicking.
        let splash_h = (term_rows as u16 + 3).min(area.height);
        let y_off = area.y + (area.height.saturating_sub(splash_h)) / 2;

        // Render pixel buffer using half-block characters
        let mut lines: Vec<Line> = Vec::new();

        for row in 0..term_rows {
            let mut spans: Vec<Span> = Vec::new();
            let top_y = row * 2;
            let bot_y = row * 2 + 1;

            for col in 0..term_cols {
                let top = self.buffer[top_y * CANVAS_W + col];
                let bot = self.buffer[bot_y * CANVAS_W + col];

                let (ch, fg, bg_color) = half_block_pixel(top, bot);
                spans.push(Span::styled(ch, Style::default().fg(fg).bg(bg_color)));
            }
            lines.push(Line::from(spans));
        }

        // Add "KASK" wordmark below the logo (monospace, letter-spaced)
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("{:>width$}", "K  A  S  K", width = term_cols / 2 + 8),
            Style::default()
                .fg(Color::Rgb(224, 224, 224))
                .bg(Color::Rgb(11, 12, 21)),
        )));

        // Prompt
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(
                "{:>width$}",
                "Press any key to continue",
                width = term_cols / 2 + 14
            ),
            Style::default()
                .fg(Color::DarkGray)
                .bg(Color::Rgb(11, 12, 21)),
        )));

        let render_area = Rect::new(x_off, y_off, term_cols as u16, splash_h);
        let paragraph = Paragraph::new(lines);
        f.render_widget(paragraph, render_area);
    }
}

/// Map a pair of vertical pixels (top, bottom) to a half-block character + colors.
///
/// Pixel values: 0=bg, 1=main stroke, 2=shadow, 3=highlight
fn half_block_pixel(top: u8, bot: u8) -> (&'static str, Color, Color) {
    let bg = Color::Rgb(11, 12, 21); // #0B0C15 deep space
    let main = Color::Rgb(224, 224, 224); // #E0E0E0 light gray
    let shadow = Color::Rgb(60, 60, 70); // dim shadow
    let highlight = Color::White; // eye reflection
    let steel_blue = Color::Rgb(70, 130, 180); // #4682B4 steel blue
    let dark_steel = Color::Rgb(44, 62, 80); // #2C3E50 dark steel

    let top_color = pixel_color(top, main, shadow, highlight, steel_blue, dark_steel, bg);
    let bot_color = pixel_color(bot, main, shadow, highlight, steel_blue, dark_steel, bg);

    match (top, bot) {
        (0, 0) => (" ", bg, bg),
        (0, _) => ("▄", bot_color, bg),
        (_, 0) => ("▀", top_color, bg),
        _ if top == bot => ("█", top_color, bg),
        _ => ("▀", top_color, bot_color),
    }
}

fn pixel_color(
    value: u8,
    main: Color,
    shadow: Color,
    highlight: Color,
    steel_blue: Color,
    dark_steel: Color,
    bg: Color,
) -> Color {
    match value {
        1 => main, // black tracing
        2 => shadow,
        3 => highlight,
        4 => steel_blue,
        5 => dark_steel,
        6 => main,       // main stroke (near-black)
        7 => steel_blue, // steel blue highlight
        _ => bg,
    }
}

impl Default for SplashScreen {
    fn default() -> Self {
        Self::new()
    }
}
