mod metrics;
mod snake;
mod starfield;
mod lava;
mod seismograph;
mod balls;

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    style::{Color, SetForegroundColor},
    terminal::{self, ClearType},
};
use std::env;
use std::io::{self, BufWriter};
use std::time::{Duration, Instant};

use metrics::Metrics;

const METRICS_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Clone, Copy)]
enum Mode { Snake, Starfield, Lava, Seismograph, Balls }

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    let screensaver_mode = args.iter().any(|a| a == "--screensaver" || a == "-s");
    let demo_mode        = args.iter().any(|a| a == "--demo"        || a == "-d");
    let mode = if args.iter().any(|a| a == "--starfield" || a == "--stars") {
        Mode::Starfield
    } else if args.iter().any(|a| a == "--lava") {
        Mode::Lava
    } else if args.iter().any(|a| a == "--seismograph" || a == "--seismo") {
        Mode::Seismograph
    } else if args.iter().any(|a| a == "--balls") {
        Mode::Balls
    } else {
        Mode::Snake
    };

    let mut stdout = BufWriter::new(io::stdout());
    terminal::enable_raw_mode()?;
    execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide, terminal::Clear(ClearType::All))?;

    let result = match mode {
        Mode::Snake       => run_snake(&mut stdout, screensaver_mode, demo_mode),
        Mode::Starfield   => run_starfield(&mut stdout, screensaver_mode, demo_mode),
        Mode::Lava        => run_lava(&mut stdout, screensaver_mode, demo_mode),
        Mode::Seismograph => run_seismograph(&mut stdout, screensaver_mode, demo_mode),
        Mode::Balls       => run_balls(&mut stdout, screensaver_mode, demo_mode),
    };

    execute!(stdout, terminal::LeaveAlternateScreen, cursor::Show, SetForegroundColor(Color::Reset))?;
    terminal::disable_raw_mode()?;
    result
}

fn handle_events(
    stdout: &mut BufWriter<io::Stdout>,
    screensaver_mode: bool,
    on_resize: &mut dyn FnMut(&mut BufWriter<io::Stdout>, u16, u16) -> io::Result<()>,
) -> io::Result<bool> {
    if event::poll(Duration::from_millis(5))? {
        match event::read()? {
            Event::Key(KeyEvent { code: KeyCode::Char('q'), .. })
            | Event::Key(KeyEvent { code: KeyCode::Esc, .. })
            | Event::Key(KeyEvent { code: KeyCode::Char('c'), modifiers: KeyModifiers::CONTROL, .. })
            => return Ok(true),
            Event::Key(_) if screensaver_mode => return Ok(true),
            Event::Resize(c, r) => on_resize(stdout, c, r)?,
            _ => {}
        }
    }
    Ok(false)
}

fn run_snake(stdout: &mut BufWriter<io::Stdout>, screensaver_mode: bool, demo_mode: bool) -> io::Result<()> {
    let mut rng = rand::thread_rng();
    let mut metrics = Metrics::new(demo_mode);

    let (cols, rows) = terminal::size()?;
    let game_rows = rows.saturating_sub(1);
    let mut s = snake::Snake::new(cols, game_rows);
    s.target_len = snake::target_len(&metrics, cols, game_rows);

    let mut last_step            = Instant::now();
    let mut last_metrics_refresh = Instant::now();

    loop {
        let quit = handle_events(stdout, screensaver_mode, &mut |stdout, c, r| {
            execute!(stdout, terminal::Clear(ClearType::All))?;
            s = snake::Snake::new(c, r.saturating_sub(1));
            Ok(())
        })?;
        if quit { break; }

        let refresh_interval = if demo_mode { Duration::from_millis(50) } else { METRICS_INTERVAL };
        if last_metrics_refresh.elapsed() >= refresh_interval {
            metrics.refresh();
            let (cols, rows) = terminal::size()?;
            s.target_len = snake::target_len(&metrics, cols, rows.saturating_sub(1));
            last_metrics_refresh = Instant::now();
        }

        if last_step.elapsed() >= Duration::from_millis(snake::tick_ms(&metrics)) {
            let (cols, rows) = terminal::size()?;
            let game_rows = rows.saturating_sub(1);

            for pos in s.step(cols, game_rows, &mut rng) {
                snake::erase_pos(stdout, pos)?;
            }

            snake::draw(stdout, &s, &metrics, cols, rows.saturating_sub(1), screensaver_mode)?;
            last_step = Instant::now();
        }
    }
    Ok(())
}

fn run_starfield(stdout: &mut BufWriter<io::Stdout>, screensaver_mode: bool, demo_mode: bool) -> io::Result<()> {
    let mut rng = rand::thread_rng();
    let mut metrics = Metrics::new(demo_mode);

    let mut field = starfield::Starfield::new(&mut rng);
    field.target_count = starfield::target_count(&metrics);

    let mut last_step            = Instant::now();
    let mut last_metrics_refresh = Instant::now();

    loop {
        let quit = handle_events(stdout, screensaver_mode, &mut |stdout, _, _| {
            execute!(stdout, terminal::Clear(ClearType::All))
        })?;
        if quit { break; }

        let refresh_interval = if demo_mode { Duration::from_millis(50) } else { METRICS_INTERVAL };
        if last_metrics_refresh.elapsed() >= refresh_interval {
            metrics.refresh();
            field.target_count = starfield::target_count(&metrics);
            last_metrics_refresh = Instant::now();
        }

        if last_step.elapsed() >= Duration::from_millis(starfield::tick_ms(&metrics)) {
            let (cols, rows) = terminal::size()?;
            let game_rows = rows.saturating_sub(1);
            field.step(stdout, cols, game_rows, metrics.load(), &mut rng)?;
            starfield::draw_status_bar(stdout, &metrics, &field, cols, rows.saturating_sub(1), screensaver_mode)?;
            last_step = Instant::now();
        }
    }
    Ok(())
}

fn run_lava(stdout: &mut BufWriter<io::Stdout>, screensaver_mode: bool, demo_mode: bool) -> io::Result<()> {
    let mut rng = rand::thread_rng();
    let mut metrics = Metrics::new(demo_mode);

    let (cols, rows) = terminal::size()?;
    let game_rows = rows.saturating_sub(1);
    let mut lamp = lava::Lava::new(cols, game_rows, &mut rng);
    lamp.target_blobs = lava::target_blobs(&metrics);

    let mut last_step            = Instant::now();
    let mut last_metrics_refresh = Instant::now();

    loop {
        let quit = handle_events(stdout, screensaver_mode, &mut |stdout, c, r| {
            execute!(stdout, terminal::Clear(ClearType::All))?;
            lamp = lava::Lava::new(c, r.saturating_sub(1), &mut rng);
            Ok(())
        })?;
        if quit { break; }

        let refresh_interval = if demo_mode { Duration::from_millis(50) } else { METRICS_INTERVAL };
        if last_metrics_refresh.elapsed() >= refresh_interval {
            metrics.refresh();
            lamp.target_blobs = lava::target_blobs(&metrics);
            last_metrics_refresh = Instant::now();
        }

        if last_step.elapsed() >= Duration::from_millis(lava::tick_ms(&metrics)) {
            let (cols, rows) = terminal::size()?;
            let game_rows = rows.saturating_sub(1);
            lamp.step(stdout, cols, game_rows, metrics.load(), &mut rng)?;
            lava::draw_status_bar(stdout, &metrics, &lamp, cols, rows.saturating_sub(1), screensaver_mode)?;
            last_step = Instant::now();
        }
    }
    Ok(())
}

fn run_balls(stdout: &mut BufWriter<io::Stdout>, screensaver_mode: bool, demo_mode: bool) -> io::Result<()> {
    let mut rng = rand::thread_rng();
    let mut metrics = Metrics::new(demo_mode);

    let mut b = balls::Balls::new();
    b.target_count = balls::target_count(&metrics);

    let mut last_step            = Instant::now();
    let mut last_metrics_refresh = Instant::now();

    loop {
        let quit = handle_events(stdout, screensaver_mode, &mut |stdout, _, _| {
            execute!(stdout, terminal::Clear(ClearType::All))
        })?;
        if quit { break; }

        let refresh_interval = if demo_mode { Duration::from_millis(50) } else { METRICS_INTERVAL };
        if last_metrics_refresh.elapsed() >= refresh_interval {
            metrics.refresh();
            b.target_count = balls::target_count(&metrics);
            last_metrics_refresh = Instant::now();
        }

        if last_step.elapsed() >= Duration::from_millis(balls::tick_ms(&metrics)) {
            let (cols, rows) = terminal::size()?;
            let game_rows = rows.saturating_sub(1);
            b.step(stdout, cols, game_rows, metrics.load(), &mut rng)?;
            balls::draw_status_bar(stdout, &metrics, &b, cols, rows.saturating_sub(1), screensaver_mode)?;
            last_step = Instant::now();
        }
    }
    Ok(())
}

fn run_seismograph(stdout: &mut BufWriter<io::Stdout>, screensaver_mode: bool, demo_mode: bool) -> io::Result<()> {
    let mut rng = rand::thread_rng();
    let mut metrics = Metrics::new(demo_mode);

    let (cols, _rows) = terminal::size()?;
    let mut seismo = seismograph::Seismograph::new(cols);

    let mut last_step            = Instant::now();
    let mut last_metrics_refresh = Instant::now();

    loop {
        let quit = handle_events(stdout, screensaver_mode, &mut |stdout, c, _r| {
            execute!(stdout, terminal::Clear(ClearType::All))?;
            seismo.resize(c);
            Ok(())
        })?;
        if quit { break; }

        let refresh_interval = if demo_mode { Duration::from_millis(50) } else { METRICS_INTERVAL };
        if last_metrics_refresh.elapsed() >= refresh_interval {
            metrics.refresh();
            last_metrics_refresh = Instant::now();
        }

        if last_step.elapsed() >= Duration::from_millis(seismograph::tick_ms(&metrics)) {
            let (cols, rows) = terminal::size()?;
            seismo.resize(cols);
            seismo.step(stdout, &metrics, rows.saturating_sub(1), screensaver_mode, &mut rng)?;
            last_step = Instant::now();
        }
    }
    Ok(())
}
