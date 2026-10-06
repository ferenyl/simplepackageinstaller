use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::style::{Color, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::sudo;

use super::centered;

pub enum Outcome {
    Pending,
    Accepted,
    Cancelled,
}

/// Fixed so the buffer never reallocates and leaves copies of the password behind.
const CAPACITY: usize = 256;

pub struct PasswordPrompt {
    input: String,
    error: Option<&'static str>,
}

impl Default for PasswordPrompt {
    fn default() -> Self {
        Self { input: String::with_capacity(CAPACITY), error: None }
    }
}

impl Drop for PasswordPrompt {
    fn drop(&mut self) {
        self.wipe();
    }
}

impl PasswordPrompt {
    pub fn handle_key(&mut self, key: KeyEvent) -> Outcome {
        match key.code {
            KeyCode::Esc => {
                self.wipe();
                return Outcome::Cancelled;
            }
            KeyCode::Enter => {
                let ok = sudo::validate(&self.input);
                self.wipe();
                if ok {
                    sudo::start_keepalive();
                    return Outcome::Accepted;
                }
                self.error = Some("Wrong password, try again");
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) if self.input.len() + c.len_utf8() <= self.input.capacity() => self.input.push(c),
            _ => {}
        }
        Outcome::Pending
    }

    /// Zeroes the whole allocation, including bytes left behind by backspace.
    fn wipe(&mut self) {
        // SAFETY: only u8 zeros are written within the allocation, and the string is cleared afterwards.
        unsafe {
            let bytes = self.input.as_mut_vec();
            let ptr = bytes.as_mut_ptr();
            for i in 0..bytes.capacity() {
                std::ptr::write_volatile(ptr.add(i), 0);
            }
            bytes.clear();
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = centered(frame.area(), 50, 6);
        let mut lines = vec![
            Line::from("The installation requires sudo."),
            Line::from(format!("Password: {}", "•".repeat(self.input.chars().count()))),
        ];
        if let Some(e) = self.error {
            lines.push(Line::from(e).fg(Color::Red));
        }
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(lines).block(Block::bordered().title(" sudo · enter ok · esc cancel ")),
            area,
        );
    }
}
