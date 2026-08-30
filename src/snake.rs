use crossterm::{
    cursor, queue,
    style::{Color, Print, SetForegroundColor},
};
use rand::Rng;
use std::collections::VecDeque;
use std::io::{self, Write, BufWriter};
use crate::metrics::Metrics;

const MIN_SPEED_MS: u64 = 30;
const MAX_SPEED_MS: u64 = 250;
const MIN_LENGTH: usize = 5;
const MAX_LENGTH_FRAC: f64 = 0.04;
const MIN_STRAIGHT_STEPS: u32 = 6;
const TURN_PROB: f64 = 0.04;
const WALL_LOOKAHEAD: i32 = 2;

const HEAD_CHAR: char = '█';
const BODY_CHARS: &[char] = &['█', '▓', '░'];
const TAIL_CHAR: char = '░';

#[derive(Clone, Copy, PartialEq)]
enum Dir { Up, Down, Left, Right }

impl Dir {
    fn delta(self) -> (i32, i32) {
        match self {
            Dir::Up    => (0, -1),
            Dir::Down  => (0,  1),
            Dir::Left  => (-1, 0),
            Dir::Right => (1,  0),
        }
    }
    fn opposite(self) -> Dir {
        match self {
            Dir::Up => Dir::Down, Dir::Down => Dir::Up,
            Dir::Left => Dir::Right, Dir::Right => Dir::Left,
        }
    }
    fn runway(self, x: u16, y: u16, cols: u16, rows: u16) -> i32 {
        match self {
            Dir::Right => cols as i32 - 1 - x as i32,
            Dir::Left  => x as i32,
            Dir::Down  => rows as i32 - 1 - y as i32,
            Dir::Up    => y as i32,
        }.max(0)
    }
}

pub struct Snake {
    body: VecDeque<(u16, u16)>,
    dir: Dir,
    pub target_len: usize,
    steps_straight: u32,
}

impl Snake {
    pub fn new(cols: u16, rows: u16) -> Self {
        let mut body = VecDeque::new();
        for i in 0..MIN_LENGTH {
            body.push_back((cols / 2 - i as u16, rows / 2));
        }
        Snake { body, dir: Dir::Right, target_len: MIN_LENGTH, steps_straight: 0 }
    }

    pub fn step(&mut self, cols: u16, rows: u16, rng: &mut impl Rng) -> Vec<(u16, u16)> {
        let (hx, hy) = self.body[0];
        let new_dir = self.pick_dir(hx, hy, cols, rows, rng);
        if new_dir == self.dir { self.steps_straight += 1; } else { self.steps_straight = 0; }
        self.dir = new_dir;

        let (dx, dy) = self.dir.delta();
        let nx = (hx as i32 + dx).clamp(0, cols as i32 - 1) as u16;
        let ny = (hy as i32 + dy).clamp(0, rows as i32 - 1) as u16;
        self.body.push_front((nx, ny));

        let over = self.body.len().saturating_sub(self.target_len);
        let drop = if over == 0 { 0 } else { ((over / 4) + 1).min(over) };
        let mut erased = Vec::with_capacity(drop);
        for _ in 0..drop {
            if let Some(pos) = self.body.pop_back() { erased.push(pos); }
        }
        erased
    }

    fn pick_dir(&self, hx: u16, hy: u16, cols: u16, rows: u16, rng: &mut impl Rng) -> Dir {
        let preferred = self.dir;
        let must_turn = preferred.runway(hx, hy, cols, rows) < WALL_LOOKAHEAD;

        if !must_turn && self.steps_straight < MIN_STRAIGHT_STEPS { return preferred; }
        if !must_turn && !rng.gen_bool(TURN_PROB) { return preferred; }

        let all_dirs = [Dir::Up, Dir::Down, Dir::Left, Dir::Right];
        let mut candidates: Vec<Dir> = all_dirs.iter().copied()
            .filter(|&d| d != preferred && d != preferred.opposite())
            .collect();

        if !must_turn && candidates.iter().all(|&d| d.runway(hx, hy, cols, rows) < 1) {
            return preferred;
        }

        candidates.sort_by_key(|&d| -(d.runway(hx, hy, cols, rows)));

        if !must_turn && candidates.len() == 2 && rng.gen_bool(0.5) {
            candidates[1]
        } else {
            candidates[0]
        }
    }
}

pub fn tick_ms(metrics: &Metrics) -> u64 {
    metrics.tick_ms(MIN_SPEED_MS, MAX_SPEED_MS)
}

pub fn target_len(metrics: &Metrics, cols: u16, rows: u16) -> usize {
    let max_len = ((cols as f64 * rows as f64 * MAX_LENGTH_FRAC) as usize).max(MIN_LENGTH + 1);
    let load = metrics.load();
    const GROWTH_THRESHOLD: f64 = 0.20;
    if load < GROWTH_THRESHOLD {
        MIN_LENGTH
    } else {
        let t = (load - GROWTH_THRESHOLD) / (1.0 - GROWTH_THRESHOLD);
        MIN_LENGTH + (t * (max_len - MIN_LENGTH) as f64) as usize
    }
}

fn palette(load: f64) -> (Color, Color) {
    if load < 0.5 {
        if load < 0.25 { (Color::Green,  Color::DarkGreen)  }
        else           { (Color::Yellow, Color::DarkYellow) }
    } else {
        if load < 0.75 { (Color::Yellow, Color::DarkYellow) }
        else           { (Color::Red,    Color::DarkRed)    }
    }
}

pub fn draw(
    stdout: &mut BufWriter<io::Stdout>,
    snake: &Snake,
    metrics: &Metrics,
    cols: u16,
    status_row: u16,
    screensaver_mode: bool,
) -> io::Result<()> {
    let (bright, dim) = palette(metrics.load());
    let len = snake.body.len();

    for (i, &(col, row)) in snake.body.iter().enumerate() {
        let (ch, color) = if i == 0 {
            (HEAD_CHAR, bright)
        } else if i == len - 1 {
            (TAIL_CHAR, dim)
        } else {
            let body_len = (len - 2).max(1);
            let ch = BODY_CHARS[((i - 1) * BODY_CHARS.len() / body_len).min(BODY_CHARS.len() - 1)];
            let color = if i < (len * 3 / 4) { bright } else { dim };
            (ch, color)
        };
        queue!(stdout, SetForegroundColor(color), cursor::MoveTo(col, row), Print(ch))?;
    }

    draw_status(stdout, metrics, cols, status_row, screensaver_mode,
        &format!("Len:{:3}  Spd:{:3}ms", len, tick_ms(metrics)))
}

pub fn erase_pos(stdout: &mut BufWriter<io::Stdout>, pos: (u16, u16)) -> io::Result<()> {
    queue!(stdout, cursor::MoveTo(pos.0, pos.1), SetForegroundColor(Color::Black), Print(' '))
}

pub fn draw_status(
    stdout: &mut BufWriter<io::Stdout>,
    metrics: &Metrics,
    cols: u16,
    status_row: u16,
    screensaver_mode: bool,
    extra: &str,
) -> io::Result<()> {
    let hint = if screensaver_mode { "[any key=wake]" } else { "[q/ESC=quit]" };
    let demo = if metrics.demo { format!("  {}", metrics.demo_phase()) } else { String::new() };
    let s = format!(
        " Load:{:3}%  CPU:{:3}%  MEM:{:3}%  NET:{:3}%  {}{}  {} ",
        (metrics.load()    * 100.0) as u32,
        (metrics.cpu_pct   * 100.0) as u32,
        (metrics.mem_pct   * 100.0) as u32,
        (metrics.net_pct   * 100.0) as u32,
        extra, demo, hint,
    );
    let s = &s[..s.len().min(cols as usize)];
    queue!(stdout, SetForegroundColor(Color::DarkGrey), cursor::MoveTo(0, status_row), Print(s))?;
    stdout.flush()
}
