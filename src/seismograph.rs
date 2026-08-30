use crossterm::{cursor, queue, style::{Color, Print, SetForegroundColor}};
use rand::Rng;
use std::io::{self, BufWriter};
use crate::metrics::Metrics;
use crate::snake::draw_status;

const MIN_SPEED_MS: u64 = 30;
const MAX_SPEED_MS: u64 = 120;

// Each metric gets its own wave lane rendered in a distinct color.
// The screen is divided into 3 horizontal bands.
const LANE_COLORS: &[Color] = &[Color::Green, Color::Cyan, Color::Red];
const LANE_LABELS: &[&str]  = &["CPU", "MEM", "NET"];

// Characters for the waveform column (top half vs bottom half of excursion)
const WAVE_FULL:  char = '█';
const WAVE_TOP:   char = '▀';
const WAVE_BOT:   char = '▄';
const WAVE_MID:   char = '─';
const AXIS_CHAR:  char = '·';

pub struct Seismograph {
    // Ring buffer of amplitude samples per lane (one entry per column drawn)
    history: Vec<Vec<f64>>,  // [lane][col_index]
    cols: u16,
    noise: Vec<f64>,         // per-lane noise offset for organic feel
}

impl Seismograph {
    pub fn new(cols: u16) -> Self {
        let n = cols as usize;
        Seismograph {
            history: vec![vec![0.0; n]; LANE_LABELS.len()],
            cols,
            noise: vec![0.0; LANE_LABELS.len()],
        }
    }

    pub fn resize(&mut self, cols: u16) {
        self.cols = cols;
        let n = cols as usize;
        for lane in &mut self.history {
            lane.resize(n, 0.0);
        }
    }

    pub fn step(
        &mut self,
        stdout: &mut BufWriter<io::Stdout>,
        metrics: &Metrics,
        rows: u16,
        screensaver_mode: bool,
        rng: &mut impl Rng,
    ) -> io::Result<()> {
        let cols = self.cols;
        let n_lanes = LANE_LABELS.len();
        // Each lane gets an equal slice of rows (minus status bar)
        let lane_rows = (rows as usize / n_lanes).max(2);

        // Raw amplitudes for each lane, 0.0..1.0
        let amplitudes = [metrics.cpu_pct, metrics.mem_pct, metrics.net_pct];

        // Shift history left and push new sample on the right
        for (li, &amp) in amplitudes.iter().enumerate() {
            // Add a little noise for organic feel — more noise at higher load
            self.noise[li] += rng.gen_range(-0.08..0.08f64) * (0.2 + amp);
            self.noise[li] = self.noise[li].clamp(-0.12, 0.12);
            let sample = (amp + self.noise[li]).clamp(0.0, 1.0);

            let lane = &mut self.history[li];
            lane.rotate_left(1);
            *lane.last_mut().unwrap() = sample;
        }

        // Redraw all lanes from scratch each tick (cheap — terminal is fast)
        for (li, &color) in LANE_COLORS.iter().enumerate() {
            let row_base = (li * lane_rows) as u16;
            let lane_h   = lane_rows as u16;
            let mid_row  = row_base + lane_h / 2;

            // Clear the lane
            for r in row_base..row_base + lane_h {
                queue!(stdout,
                    cursor::MoveTo(0, r),
                    SetForegroundColor(Color::Black),
                    Print(" ".repeat(cols as usize))
                )?;
            }

            // Draw axis label on the left
            queue!(stdout,
                SetForegroundColor(color),
                cursor::MoveTo(0, mid_row),
                Print(LANE_LABELS[li])
            )?;

            // Draw waveform — each column maps to one history sample
            for col in 3..cols {
                let sample = self.history[li][col as usize];
                // Amplitude in rows above/below midpoint
                let half = (lane_h / 2) as f64;
                let excursion = (sample * half * 0.9) as u16; // rows of deflection

                if excursion == 0 {
                    // Flat line
                    queue!(stdout,
                        SetForegroundColor(Color::DarkGrey),
                        cursor::MoveTo(col, mid_row),
                        Print(AXIS_CHAR)
                    )?;
                } else {
                    // Fill from mid_row upward
                    let top = mid_row.saturating_sub(excursion);
                    queue!(stdout, SetForegroundColor(color))?;
                    for r in top..mid_row {
                        let ch = if r == top { WAVE_TOP }
                                 else if r == mid_row.saturating_sub(1) { WAVE_BOT }
                                 else { WAVE_FULL };
                        if r < rows {
                            queue!(stdout, cursor::MoveTo(col, r), Print(ch))?;
                        }
                    }
                    // Midpoint marker
                    queue!(stdout, cursor::MoveTo(col, mid_row), Print(WAVE_MID))?;
                }
            }

            // Draw lane separator
            if li + 1 < n_lanes {
                let sep_row = row_base + lane_h;
                if sep_row < rows {
                    queue!(stdout,
                        SetForegroundColor(Color::DarkGrey),
                        cursor::MoveTo(0, sep_row),
                        Print("─".repeat(cols as usize))
                    )?;
                }
            }
        }

        draw_status(stdout, metrics, cols, rows, screensaver_mode,
            &format!("Spd:{:3}ms", tick_ms(metrics)))
    }
}

pub fn tick_ms(metrics: &Metrics) -> u64 {
    metrics.tick_ms(MIN_SPEED_MS, MAX_SPEED_MS)
}
