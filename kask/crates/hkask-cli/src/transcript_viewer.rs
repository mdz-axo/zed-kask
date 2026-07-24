//! TUI Transcript Viewer — interactive synchronized audio + transcript playback.
//!
//! Renders a TranscriptBundle in the terminal with:
//! - Word-level highlighting synced to audio playback
//! - Keyboard navigation (arrow keys, vim-style j/k)
//! - Play/pause, seek, segment jump
//!
//! Uses ratatui for rendering, ffplay for audio, crossterm for input.

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use hkask_types::TranscriptBundle;
use ratatui::prelude::CrosstermBackend;
use ratatui::{
    Frame, Terminal,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
};
use std::io::{Stdout, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Interactive TUI transcript viewer using ffplay for audio.
pub struct TranscriptViewer {
    bundle: TranscriptBundle,
    /// ffplay child process (None if audio file not available)
    ffplay: Option<Child>,
    /// Path to the audio file
    audio_path: Option<PathBuf>,
    /// Current highlighted word index
    current_word_idx: usize,
    /// Whether audio is currently playing
    is_playing: bool,
    /// Playback start time (for position calculation)
    play_start: Option<Instant>,
    /// Seek offset when paused
    paused_position: Duration,
    /// Scroll offset for transcript display
    scroll_offset: u16,
    /// ffplay input pipe for sending seek commands
    ffplay_stdin: Option<std::process::ChildStdin>,
}

impl TranscriptViewer {
    /// Load a TranscriptBundle from a JSON file.
    pub fn from_file(path: &Path) -> anyhow::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        let bundle: TranscriptBundle = serde_json::from_str(&json)?;

        let audio_path = if Path::new(&bundle.audio_path).exists() {
            Some(PathBuf::from(&bundle.audio_path))
        } else {
            None
        };

        Ok(Self {
            bundle,
            ffplay: None,
            audio_path,
            current_word_idx: 0,
            is_playing: false,
            play_start: None,
            paused_position: Duration::ZERO,
            scroll_offset: 0,
            ffplay_stdin: None,
        })
    }

    /// Run the interactive TUI loop.
    pub fn run(&mut self) -> anyhow::Result<()> {
        let mut terminal = ratatui::init();
        let result = self.event_loop(&mut terminal);
        self.stop_audio();
        ratatui::restore();
        result
    }

    fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    ) -> anyhow::Result<()> {
        let tick_rate = Duration::from_millis(50);

        loop {
            self.sync_position();
            terminal.draw(|f| self.render(f))?;

            if event::poll(tick_rate)? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        if !self.handle_key(key.code) {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn sync_position(&mut self) {
        if !self.is_playing {
            return;
        }

        let elapsed = if let Some(start) = self.play_start {
            start.elapsed() + self.paused_position
        } else {
            self.paused_position
        };

        let pos_ms = elapsed.as_millis() as u64;

        if let Some(idx) = self
            .bundle
            .words
            .iter()
            .position(|w| w.start_ms <= pos_ms && pos_ms < w.end_ms)
        {
            self.current_word_idx = idx;
        }
    }

    fn handle_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => return false,
            KeyCode::Char(' ') => self.toggle_play_pause(),
            KeyCode::Right => self.seek_relative(5.0),
            KeyCode::Left => self.seek_relative(-5.0),
            KeyCode::Up | KeyCode::Char('k') => {
                if !self.is_playing && self.current_word_idx > 0 {
                    self.current_word_idx -= 1;
                    let ms = self.bundle.words[self.current_word_idx].start_ms;
                    self.seek_to_ms(ms);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.is_playing && self.current_word_idx + 1 < self.bundle.words.len() {
                    self.current_word_idx += 1;
                    let ms = self.bundle.words[self.current_word_idx].start_ms;
                    self.seek_to_ms(ms);
                }
            }
            KeyCode::Char('[') => self.jump_segment(-1),
            KeyCode::Char(']') => self.jump_segment(1),
            KeyCode::Home => {
                self.seek_to_ms(0);
                self.current_word_idx = 0;
            }
            KeyCode::End => {
                if let Some(w) = self.bundle.words.last() {
                    self.seek_to_ms(w.start_ms);
                }
            }
            KeyCode::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(5);
            }
            KeyCode::PageDown => {
                self.scroll_offset += 5;
            }
            _ => {}
        }
        true
    }

    fn toggle_play_pause(&mut self) {
        if self.is_playing {
            // Pause
            if let Some(start) = self.play_start {
                self.paused_position += start.elapsed();
            }
            self.stop_audio();
            self.is_playing = false;
            self.play_start = None;
        } else {
            // Play from current position
            self.start_audio_at(self.paused_position);
            self.is_playing = true;
            self.play_start = Some(Instant::now());
        }
    }

    fn seek_relative(&mut self, delta_secs: f32) {
        let was_playing = self.is_playing;
        if was_playing {
            self.stop_audio();
        }

        let current_ms = if let Some(start) = self.play_start {
            (start.elapsed() + self.paused_position).as_millis() as i64
        } else {
            self.paused_position.as_millis() as i64
        };
        let new_ms = (current_ms + (delta_secs * 1000.0) as i64).max(0) as u64;

        self.paused_position = Duration::from_millis(new_ms);

        if was_playing {
            self.start_audio_at(self.paused_position);
            self.play_start = Some(Instant::now());
        }
    }

    fn seek_to_ms(&mut self, ms: u64) {
        let was_playing = self.is_playing;
        if was_playing {
            self.stop_audio();
        }
        self.paused_position = Duration::from_millis(ms);
        if was_playing {
            self.start_audio_at(self.paused_position);
            self.play_start = Some(Instant::now());
        }
    }

    fn jump_segment(&mut self, delta: i32) {
        if self.bundle.segments.is_empty() {
            return;
        }
        let pos_ms = if let Some(start) = self.play_start {
            (start.elapsed() + self.paused_position).as_millis() as u64
        } else {
            self.paused_position.as_millis() as u64
        };

        let current_seg = self
            .bundle
            .segments
            .iter()
            .position(|s| s.start_ms <= pos_ms && pos_ms < s.end_ms);

        let target_idx = match current_seg {
            Some(i) => (i as i32 + delta)
                .max(0)
                .min(self.bundle.segments.len() as i32 - 1) as usize,
            None => 0,
        };

        let ms = self.bundle.segments[target_idx].start_ms;
        self.seek_to_ms(ms);
    }

    // ── ffplay audio control ───────────────────────────────────────────────

    fn start_audio_at(&mut self, position: Duration) {
        self.stop_audio();

        let audio_path = match &self.audio_path {
            Some(p) => p,
            None => return,
        };

        let seek_secs = position.as_secs_f64();

        match Command::new("ffplay")
            .arg("-nodisp") // no video window
            .arg("-autoexit") // exit when done
            .arg("-ss")
            .arg(format!("{:.3}", seek_secs))
            .arg(audio_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                self.ffplay_stdin = child.stdin.take();
                self.ffplay = Some(child);
            }
            Err(_) => {
                // ffplay not available — transcript-only mode
            }
        }
    }

    fn stop_audio(&mut self) {
        // Send 'q' to ffplay stdin to quit gracefully
        if let Some(ref mut stdin) = self.ffplay_stdin {
            let _ = stdin.write_all(b"q");
        }
        if let Some(ref mut child) = self.ffplay {
            let _ = child.kill();
        }
        self.ffplay = None;
        self.ffplay_stdin = None;
    }

    // ── Rendering ──────────────────────────────────────────────────────────

    fn render(&self, f: &mut Frame) {
        let area = f.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Min(3),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(area);

        self.render_header(f, chunks[0]);
        self.render_transcript(f, chunks[1]);
        self.render_progress(f, chunks[2]);
        self.render_help(f, chunks[3]);
    }

    fn render_header(&self, f: &mut Frame, area: Rect) {
        let filename = self
            .bundle
            .audio_path
            .rsplit('/')
            .next()
            .unwrap_or(&self.bundle.audio_path);
        let title = format!(
            " Transcript Viewer — {}  [{}]  {:.0}s ",
            filename,
            self.bundle.language.as_deref().unwrap_or("??"),
            self.bundle.audio_duration_secs,
        );
        let header = Paragraph::new(title)
            .block(Block::default().borders(Borders::ALL))
            .style(Style::default().fg(Color::Cyan));
        f.render_widget(header, area);
    }

    fn render_transcript(&self, f: &mut Frame, area: Rect) {
        if self.bundle.words.is_empty() {
            let text = Paragraph::new(self.bundle.full_text.as_str())
                .block(Block::default().borders(Borders::ALL))
                .wrap(Wrap { trim: true });
            f.render_widget(text, area);
            return;
        }

        let mut lines: Vec<Line> = Vec::new();
        let mut current_line: Vec<Span> = Vec::new();
        let mut line_width: u16 = 0;
        let max_width = area.width.saturating_sub(2);

        for (i, word) in self.bundle.words.iter().enumerate() {
            let style = if i == self.current_word_idx {
                Style::default()
                    .bg(Color::Rgb(183, 145, 99)) // Richmond Gold HC-41
                    .fg(Color::Black)
                    .bold()
            } else if i < self.current_word_idx {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default()
            };

            let word_text = if i + 1 < self.bundle.words.len() {
                format!("{} ", word.word)
            } else {
                word.word.clone()
            };

            let word_width = word_text.len() as u16;

            if line_width + word_width > max_width && !current_line.is_empty() {
                lines.push(Line::from(current_line));
                current_line = Vec::new();
                line_width = 0;
            }

            current_line.push(Span::styled(word_text, style));
            line_width += word_width;
        }

        if !current_line.is_empty() {
            lines.push(Line::from(current_line));
        }

        let transcript = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: true })
            .scroll((self.scroll_offset, 0));

        f.render_widget(transcript, area);
    }

    fn render_progress(&self, f: &mut Frame, area: Rect) {
        let pos_ms = if let Some(start) = self.play_start {
            (start.elapsed() + self.paused_position).as_millis() as u64
        } else {
            self.paused_position.as_millis() as u64
        };

        let total_ms = (self.bundle.audio_duration_secs * 1000.0) as u64;
        let ratio = if total_ms > 0 {
            (pos_ms as f64 / total_ms as f64).min(1.0)
        } else {
            0.0
        };

        let pos_secs = pos_ms as f64 / 1000.0;
        let total_secs = self.bundle.audio_duration_secs as f64;

        let label = format!(
            " {:.0}:{:02.0} / {:.0}:{:02.0} ",
            pos_secs / 60.0,
            pos_secs % 60.0,
            total_secs / 60.0,
            total_secs % 60.0,
        );

        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::ALL))
            .gauge_style(
                Style::default()
                    .fg(Color::Rgb(183, 145, 99))
                    .bg(Color::DarkGray),
            )
            .ratio(ratio)
            .label(label);

        f.render_widget(gauge, area);
    }

    fn render_help(&self, f: &mut Frame, area: Rect) {
        let state = if self.is_playing {
            "▶ Playing"
        } else {
            "⏸ Paused"
        };
        let audio_status = if self.audio_path.is_some() {
            ""
        } else {
            " [no audio]"
        };
        let help = format!(
            " {}  Space:Play/Pause  ←→:Seek±5s  ↑↓/jk:Words  []:Segments  Home/End  PgUp/PgDn  q:Quit{}",
            state, audio_status
        );
        let help_text = Paragraph::new(help).style(Style::default().fg(Color::DarkGray));
        f.render_widget(help_text, area);
    }
}

impl Drop for TranscriptViewer {
    fn drop(&mut self) {
        self.stop_audio();
    }
}
