use crossterm::{
    cursor, queue,
    style::{Color, Print, SetForegroundColor},
};
use rand::Rng;
use std::io::{self, BufWriter};
use crate::metrics::Metrics;
use crate::snake::draw_status;

// Depth is 1..=MAX_DEPTH. High depth = far away (small, slow).
// Depth decreases each tick as the star rushes toward the viewer.
const MAX_DEPTH: f64 = 32.0;
const MIN_STARS: usize = 40;
const MAX_STARS: usize = 300;

// Characters by closeness (depth bucket 0=close .. 4=far)
const STAR_CHARS: &[char] = &['█', '*', '+', '·', '·'];
const STAR_COLORS_NEAR: &[Color] = &[Color::White, Color::White, Color::Cyan, Color::DarkCyan, Color::DarkGrey];

// Speed tunables
const MIN_SPEED_MS: u64 = 20;
const MAX_SPEED_MS: u64 = 200;

struct Star {
    // Position in "space" coords centered on screen: -1.0..1.0
    sx: f64,
    sy: f64,
    depth: f64,       // 1.0 (close) .. MAX_DEPTH (far)
    // Previous screen position — needed to erase
    prev_col: Option<u16>,
    prev_row: Option<u16>,
}

impl Star {
    fn new_random(rng: &mut impl Rng) -> Self {
        Star {
            sx: rng.gen_range(-1.0..1.0f64),
            sy: rng.gen_range(-1.0..1.0f64),
            depth: rng.gen_range(1.0..MAX_DEPTH),
            prev_col: None,
            prev_row: None,
        }
    }

    // Project space coords to screen position given terminal size.
    // Returns None if off screen.
    fn project(&self, cols: u16, rows: u16) -> Option<(u16, u16)> {
        let cx = cols as f64 / 2.0;
        let cy = rows as f64 / 2.0;
        // Perspective divide: closer stars spread further from center
        let scale = MAX_DEPTH / self.depth;
        // Terminals are ~2x taller per cell than wide, compensate
        let col = cx + self.sx * cx * scale;
        let row = cy + self.sy * cy * scale * 0.5;
        if col >= 0.0 && col < cols as f64 && row >= 0.0 && row < rows as f64 {
            Some((col as u16, row as u16))
        } else {
            None
        }
    }

    fn depth_bucket(&self) -> usize {
        let frac = (self.depth / MAX_DEPTH).clamp(0.0, 1.0);
        // 0 = close (bright, big), 4 = far (dim, small)
        (frac * (STAR_CHARS.len() - 1) as f64) as usize
    }
}

pub struct Starfield {
    stars: Vec<Star>,
    pub target_count: usize,
}

impl Starfield {
    pub fn new(rng: &mut impl Rng) -> Self {
        let stars = (0..MIN_STARS).map(|_| Star::new_random(rng)).collect();
        Starfield { stars, target_count: MIN_STARS }
    }

    pub fn step(
        &mut self,
        stdout: &mut BufWriter<io::Stdout>,
        cols: u16,
        rows: u16,
        load: f64,
        rng: &mut impl Rng,
    ) -> io::Result<()> {
        // Speed at which depth decreases: faster under load
        let depth_step = 0.3 + load * 2.5;

        // Add/remove stars to match target count
        while self.stars.len() < self.target_count {
            // New stars spawn near center at max depth (far away)
            let mut s = Star::new_random(rng);
            s.sx = rng.gen_range(-0.15..0.15);
            s.sy = rng.gen_range(-0.15..0.15);
            s.depth = MAX_DEPTH;
            s.prev_col = None;
            s.prev_row = None;
            self.stars.push(s);
        }

        let mut to_respawn = Vec::new();

        for (i, star) in self.stars.iter_mut().enumerate() {
            // Erase previous position
            if let (Some(pc), Some(pr)) = (star.prev_col, star.prev_row) {
                queue!(stdout, cursor::MoveTo(pc, pr), SetForegroundColor(Color::Black), Print(' '))?;
            }

            star.depth -= depth_step;

            if star.depth < 1.0 || star.project(cols, rows).is_none() {
                to_respawn.push(i);
                continue;
            }

            // Draw at new position
            if let Some((col, row)) = star.project(cols, rows) {
                let bucket = star.depth_bucket();
                // Flip bucket: low depth (close) = bucket 0 = bright
                let b = (STAR_CHARS.len() - 1) - bucket;
                let ch = STAR_CHARS[b];
                // Streak effect: very close stars also leave a trail char one step back
                let color = STAR_COLORS_NEAR[b];
                queue!(stdout, SetForegroundColor(color), cursor::MoveTo(col, row), Print(ch))?;
                star.prev_col = Some(col);
                star.prev_row = Some(row);
            }
        }

        // Respawn stars that went off screen, replacing from back to keep indices valid
        for i in to_respawn.into_iter().rev() {
            let mut s = Star::new_random(rng);
            s.sx = rng.gen_range(-0.15..0.15);
            s.sy = rng.gen_range(-0.15..0.15);
            s.depth = MAX_DEPTH;
            // Only keep if under target count, otherwise drop
            if self.stars.len() > self.target_count {
                self.stars.swap_remove(i);
            } else {
                self.stars[i] = s;
            }
        }

        Ok(())
    }
}

pub fn tick_ms(metrics: &Metrics) -> u64 {
    metrics.tick_ms(MIN_SPEED_MS, MAX_SPEED_MS)
}

pub fn target_count(metrics: &Metrics) -> usize {
    let load = metrics.load();
    MIN_STARS + (load * (MAX_STARS - MIN_STARS) as f64) as usize
}

pub fn draw_status_bar(
    stdout: &mut BufWriter<io::Stdout>,
    metrics: &Metrics,
    field: &Starfield,
    cols: u16,
    status_row: u16,
    screensaver_mode: bool,
) -> io::Result<()> {
    draw_status(stdout, metrics, cols, status_row, screensaver_mode,
        &format!("Stars:{:3}  Spd:{:3}ms", field.stars.len(), tick_ms(metrics)))
}
