use crossterm::{cursor, queue, style::{Color, Print, SetForegroundColor}};
use rand::{Rng, seq::SliceRandom};
use std::io::{self, BufWriter, Write};
use crate::metrics::Metrics;
use crate::snake::draw_status;

const MIN_SPEED_MS: u64 = 60;
const MAX_SPEED_MS: u64 = 300;
const MIN_BLOBS: usize = 3;
const MAX_BLOBS: usize = 12;

// Blob chars by radius: large → small
const BLOB_CHARS: &[&str] = &["█", "▓", "▒", "░", "·"];

struct Blob {
    // sub-cell position for smooth float movement
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    radius: f64,   // in terminal rows (1.0 = 1 row tall; cols ~2× narrower per cell)
    color: Color,
    // cells we drew last tick — must erase them this tick
    prev_cells: Vec<(u16, u16)>,
}

impl Blob {
    fn new(cols: u16, rows: u16, rng: &mut impl Rng) -> Self {
        let palette = [
            Color::Red, Color::DarkRed, Color::Yellow, Color::DarkYellow,
            Color::Magenta, Color::DarkMagenta,
        ];
        let color = *palette.choose(rng).unwrap_or(&Color::Red);
        Blob {
            x: rng.gen_range(4.0..(cols as f64 - 4.0)),
            y: rng.gen_range(2.0..(rows as f64 - 2.0)),
            vx: rng.gen_range(-0.4..0.4f64),
            vy: rng.gen_range(-0.3..0.3f64),
            radius: rng.gen_range(1.2..3.5),
            color,
            prev_cells: Vec::new(),
        }
    }

    // Cells covered by this blob (ellipse: 2:1 width:height ratio for terminal aspect)
    fn cells(&self) -> Vec<(u16, u16, char)> {
        let mut out = Vec::new();
        let rx = self.radius * 2.0; // horizontal radius (cols)
        let ry = self.radius;        // vertical radius (rows)
        let x0 = (self.x - rx - 1.0).floor() as i32;
        let x1 = (self.x + rx + 1.0).ceil()  as i32;
        let y0 = (self.y - ry - 1.0).floor() as i32;
        let y1 = (self.y + ry + 1.0).ceil()  as i32;
        for row in y0..=y1 {
            for col in x0..=x1 {
                if col < 0 || row < 0 { continue; }
                let dx = (col as f64 - self.x) / rx;
                let dy = (row as f64 - self.y) / ry;
                let dist = (dx * dx + dy * dy).sqrt(); // 0=center, 1=edge
                if dist <= 1.0 {
                    // Map distance to char: center solid, edge transparent
                    let bucket = (dist * (BLOB_CHARS.len() - 1) as f64) as usize;
                    let ch = BLOB_CHARS[bucket].chars().next().unwrap_or('·');
                    out.push((col as u16, row as u16, ch));
                }
            }
        }
        out
    }

    fn step(&mut self, cols: u16, rows: u16, load: f64, rng: &mut impl Rng) {
        // Speed scales with load; also add slight random drift for organic feel
        let speed = 0.4 + load * 1.2;
        self.vx += rng.gen_range(-0.05..0.05f64);
        self.vy += rng.gen_range(-0.05..0.05f64);
        // Clamp velocity
        let vmax = 0.5 + load * 0.8;
        self.vx = self.vx.clamp(-vmax, vmax);
        self.vy = self.vy.clamp(-vmax * 0.6, vmax * 0.6);
        self.x += self.vx * speed;
        self.y += self.vy * speed;

        // Bounce off walls
        let margin = self.radius * 2.0 + 1.0;
        if self.x < margin          { self.x = margin;                   self.vx =  self.vx.abs(); }
        if self.x > cols as f64 - margin { self.x = cols as f64 - margin; self.vx = -self.vx.abs(); }
        let vmargin = self.radius + 1.0;
        if self.y < vmargin          { self.y = vmargin;                   self.vy =  self.vy.abs(); }
        if self.y > rows as f64 - vmargin { self.y = rows as f64 - vmargin; self.vy = -self.vy.abs(); }

        // Radius breathes slowly
        self.radius += rng.gen_range(-0.05..0.05f64);
        self.radius = self.radius.clamp(1.0, 4.0);
    }
}

pub struct Lava {
    blobs: Vec<Blob>,
    pub target_blobs: usize,
}

impl Lava {
    pub fn new(cols: u16, rows: u16, rng: &mut impl Rng) -> Self {
        let blobs = (0..MIN_BLOBS).map(|_| Blob::new(cols, rows, rng)).collect();
        Lava { blobs, target_blobs: MIN_BLOBS }
    }

    pub fn step(
        &mut self,
        stdout: &mut BufWriter<io::Stdout>,
        cols: u16,
        rows: u16,
        load: f64,
        rng: &mut impl Rng,
    ) -> io::Result<()> {
        // Grow or shrink blob count toward target
        while self.blobs.len() < self.target_blobs {
            self.blobs.push(Blob::new(cols, rows, rng));
        }
        if self.blobs.len() > self.target_blobs {
            self.blobs.pop();
        }

        for blob in &mut self.blobs {
            // Erase previous cells
            for &(col, row) in &blob.prev_cells {
                queue!(stdout, cursor::MoveTo(col, row), SetForegroundColor(Color::Black), Print(' '))?;
            }

            blob.step(cols, rows, load, rng);

            // Draw new cells
            let cells = blob.cells();
            blob.prev_cells = cells.iter().map(|&(c, r, _)| (c, r)).collect();

            queue!(stdout, SetForegroundColor(blob.color))?;
            for (col, row, ch) in cells {
                if col < cols && row < rows {
                    queue!(stdout, cursor::MoveTo(col, row), Print(ch))?;
                }
            }
        }
        stdout.flush()
    }
}

pub fn tick_ms(metrics: &Metrics) -> u64 {
    metrics.tick_ms(MIN_SPEED_MS, MAX_SPEED_MS)
}

pub fn target_blobs(metrics: &Metrics) -> usize {
    let load = metrics.load();
    MIN_BLOBS + (load * (MAX_BLOBS - MIN_BLOBS) as f64) as usize
}

pub fn draw_status_bar(
    stdout: &mut BufWriter<io::Stdout>,
    metrics: &Metrics,
    lava: &Lava,
    cols: u16,
    status_row: u16,
    screensaver_mode: bool,
) -> io::Result<()> {
    draw_status(stdout, metrics, cols, status_row, screensaver_mode,
        &format!("Blobs:{:2}  Spd:{:3}ms", lava.blobs.len(), tick_ms(metrics)))
}
