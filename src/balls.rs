use crossterm::{cursor, queue, style::{Color, Print, SetForegroundColor}};
use rand::{Rng, seq::SliceRandom};
use std::collections::HashMap;
use std::io::{self, BufWriter, Write};
use crate::metrics::Metrics;
use crate::snake::draw_status;

const MIN_SPEED_MS: u64 = 30;
const MAX_SPEED_MS: u64 = 120;
const MIN_BALLS: usize = 2;
const MAX_BALLS: usize = 16;
const GRAVITY: f64 = 0.18;
const BOUNCE_DAMPEN: f64 = 0.72;
const WALL_DAMPEN:   f64 = 0.88;
const MIN_RADIUS: f64 = 1.5;
const MAX_RADIUS: f64 = 5.0;

const COLORS: &[Color] = &[
    Color::Red, Color::Green, Color::Cyan, Color::Yellow,
    Color::Magenta, Color::White, Color::DarkYellow,
];

// ---------------------------------------------------------------------------
// Braille canvas
//
// A braille character occupies one terminal cell but encodes a 2×4 dot grid:
//
//   dot col:  0   1
//   dot row:  0 → ⠁  ⠈
//             1 → ⠂  ⠐
//             2 → ⠄  ⠠
//             3 → ⡀  ⢀
//
// The Unicode codepoint is U+2800 + bitmask where bits map to dots:
//   bit 0 = (col0,row0)   bit 1 = (col0,row1)   bit 2 = (col0,row2)
//   bit 3 = (col1,row0)   bit 4 = (col1,row1)   bit 5 = (col1,row2)
//   bit 6 = (col0,row3)   bit 7 = (col1,row3)
//
// We accumulate dots in dot-space (2× cols, 4× rows resolution) then
// encode each terminal cell on flush.
// ---------------------------------------------------------------------------

fn dot_bit(dcol: u8, drow: u8) -> u8 {
    // dcol: 0 or 1, drow: 0..3
    match (dcol, drow) {
        (0, 0) => 0x01, (0, 1) => 0x02, (0, 2) => 0x04, (0, 3) => 0x40,
        (1, 0) => 0x08, (1, 1) => 0x10, (1, 2) => 0x20, (1, 3) => 0x80,
        _      => 0x00,
    }
}

// Dot coordinates → (terminal cell col, terminal cell row, bit)
fn dot_to_cell(dx: i32, dy: i32) -> (i32, i32, u8) {
    let col  = dx.div_euclid(2);
    let row  = dy.div_euclid(4);
    let dcol = (dx.rem_euclid(2)) as u8;
    let drow = (dy.rem_euclid(4)) as u8;
    (col, row, dot_bit(dcol, drow))
}

// Ellipse in dot-space: samples the perimeter and returns dot coords.
// Terminal cells are ~2:1 wide:tall so we scale: 1 cell-row = 4 dot-rows,
// 1 cell-col = 2 dot-cols. With terminal aspect ~2:1 the dot grid is square.
fn ellipse_dots(cx: f64, cy: f64, r_cols: f64, r_rows: f64) -> Vec<(i32, i32)> {
    // Convert centre and radii to dot-space
    let dcx = cx  * 2.0;
    let dcy = cy  * 4.0;
    let drx = r_cols * 2.0;
    let dry = r_rows * 4.0;

    // Sample the perimeter at enough points for a solid-looking outline
    let n = ((drx + dry) * std::f64::consts::PI * 2.0) as usize + 16;
    let mut dots = Vec::with_capacity(n);
    for i in 0..n {
        let t  = i as f64 / n as f64 * std::f64::consts::TAU;
        let dx = (dcx + drx * t.cos()).round() as i32;
        let dy = (dcy + dry * t.sin()).round() as i32;
        dots.push((dx, dy));
    }
    dots.sort_unstable();
    dots.dedup();
    dots
}

// ---------------------------------------------------------------------------
// Ball
// ---------------------------------------------------------------------------

struct Ball {
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    radius: f64,  // cell-rows
    color: Color,
    // Terminal cells painted last tick: (col, row) → must be erased
    prev_cells: Vec<(i32, i32)>,
    resting: bool,
}

impl Ball {
    fn new(cols: u16, load: f64, rng: &mut impl Rng) -> Self {
        let radius = MIN_RADIUS + rng.gen_range(0.0..1.0)
                     * (MAX_RADIUS - MIN_RADIUS) * (0.3 + load * 0.7);
        let from_left = rng.gen_bool(0.5);
        let rx = radius * 2.1;
        let x  = if from_left { rx + 1.0 } else { cols as f64 - rx - 1.0 };
        let speed = 1.5 + load * 3.5;
        let vx = if from_left {  rng.gen_range(0.4..1.0) * speed }
                 else          { -rng.gen_range(0.4..1.0) * speed };
        let vy = rng.gen_range(-2.0..0.0) * (0.5 + load);

        Ball {
            x, y: 2.0, vx, vy, radius,
            color: *COLORS.choose(rng).unwrap_or(&Color::White),
            prev_cells: Vec::new(),
            resting: false,
        }
    }

    fn rx(&self) -> f64 { self.radius * 2.1 }
    fn ry(&self) -> f64 { self.radius }

    fn erase(&self, stdout: &mut BufWriter<io::Stdout>, cols: u16, rows: u16) -> io::Result<()> {
        queue!(stdout, SetForegroundColor(Color::Black))?;
        for &(col, row) in &self.prev_cells {
            if col >= 0 && row >= 0 && (col as u16) < cols && (row as u16) < rows {
                queue!(stdout, cursor::MoveTo(col as u16, row as u16), Print(' '))?;
            }
        }
        Ok(())
    }

    fn draw(&mut self, stdout: &mut BufWriter<io::Stdout>, cols: u16, rows: u16) -> io::Result<()> {
        let dots = ellipse_dots(self.x, self.y, self.rx(), self.ry());

        // Accumulate dot bits per terminal cell
        let mut cells: HashMap<(i32, i32), u8> = HashMap::new();
        for (dx, dy) in dots {
            let (col, row, bit) = dot_to_cell(dx, dy);
            *cells.entry((col, row)).or_insert(0) |= bit;
        }

        queue!(stdout, SetForegroundColor(self.color))?;
        let mut painted = Vec::with_capacity(cells.len());
        for (&(col, row), &bits) in &cells {
            if col >= 0 && row >= 0 && (col as u16) < cols && (row as u16) < rows {
                let ch = char::from_u32(0x2800 | bits as u32).unwrap_or('·');
                queue!(stdout, cursor::MoveTo(col as u16, row as u16), Print(ch))?;
                painted.push((col, row));
            }
        }

        self.prev_cells = painted;
        Ok(())
    }

    fn step(&mut self, cols: u16, rows: u16) {
        if self.resting { return; }

        self.vy += GRAVITY;
        self.x  += self.vx;
        self.y  += self.vy;

        let floor = rows as f64 - 1.0 - self.ry();
        let ceil  = self.ry();
        let left  = self.rx() + 0.5;
        let right = cols as f64 - self.rx() - 0.5;

        if self.y >= floor {
            self.y  = floor;
            self.vy = -self.vy.abs() * BOUNCE_DAMPEN;
            self.vx *= 0.96;
            if self.vy.abs() < 0.4 { self.vy = 0.0; self.resting = true; }
        }
        if self.y < ceil  { self.y = ceil;  self.vy =  self.vy.abs() * WALL_DAMPEN; }
        if self.x < left  { self.x = left;  self.vx =  self.vx.abs() * WALL_DAMPEN; }
        if self.x > right { self.x = right; self.vx = -self.vx.abs() * WALL_DAMPEN; }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub struct Balls {
    balls: Vec<Ball>,
    pub target_count: usize,
    spawn_timer: u32,
}

impl Balls {
    pub fn new() -> Self {
        Balls { balls: Vec::new(), target_count: MIN_BALLS, spawn_timer: 0 }
    }

    pub fn step(
        &mut self,
        stdout: &mut BufWriter<io::Stdout>,
        cols: u16,
        rows: u16,
        load: f64,
        rng: &mut impl Rng,
    ) -> io::Result<()> {
        // Spawn gradually
        self.spawn_timer += 1;
        let spawn_every = (8u32).saturating_sub((load * 6.0) as u32).max(2);
        if self.balls.len() < self.target_count && self.spawn_timer >= spawn_every {
            self.balls.push(Ball::new(cols, load, rng));
            self.spawn_timer = 0;
        }

        // Remove excess — resting ones first
        while self.balls.len() > self.target_count {
            let i = self.balls.iter().position(|b| b.resting).unwrap_or(0);
            self.balls[i].erase(stdout, cols, rows)?;
            self.balls.remove(i);
        }

        for ball in &mut self.balls {
            ball.erase(stdout, cols, rows)?;
            ball.step(cols, rows);
            ball.draw(stdout, cols, rows)?;
        }

        stdout.flush()
    }
}

pub fn tick_ms(metrics: &Metrics) -> u64 {
    metrics.tick_ms(MIN_SPEED_MS, MAX_SPEED_MS)
}

pub fn target_count(metrics: &Metrics) -> usize {
    let load = metrics.load();
    MIN_BALLS + (load * (MAX_BALLS - MIN_BALLS) as f64) as usize
}

pub fn draw_status_bar(
    stdout: &mut BufWriter<io::Stdout>,
    metrics: &Metrics,
    balls: &Balls,
    cols: u16,
    status_row: u16,
    screensaver_mode: bool,
) -> io::Result<()> {
    draw_status(stdout, metrics, cols, status_row, screensaver_mode,
        &format!("Balls:{:2}  Spd:{:3}ms", balls.balls.len(), tick_ms(metrics)))
}
