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

#[derive(Default)]
pub struct PasswordPrompt {
    input: String,
    error: Option<&'static str>,
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
                self.error = Some("Fel lösenord, försök igen");
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) => self.input.push(c),
            _ => {}
        }
        Outcome::Pending
    }

    fn wipe(&mut self) {
        let len = self.input.len();
        self.input.replace_range(.., &"\0".repeat(len));
        self.input.clear();
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = centered(frame.area(), 50, 6);
        let mut lines = vec![
            Line::from("Installationen kräver sudo."),
            Line::from(format!("Lösenord: {}", "•".repeat(self.input.chars().count()))),
        ];
        if let Some(e) = self.error {
            lines.push(Line::from(e).fg(Color::Red));
        }
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(lines).block(Block::bordered().title(" sudo · enter ok · esc avbryt ")),
            area,
        );
    }
}
