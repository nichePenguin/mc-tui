use ratatui::widgets::WidgetRef;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

static FULL_BLOCK: char = '█';
static HORIZONTAL: [char; 8] = [' ', '▏','▎', '▍', '▌', '▋', '▊', '▉'];
static VERTICAL: [char; 8] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇' ];

#[derive(Clone)]
pub enum BarWidgetDirection {
    Horizontal,
    Vertical
}

#[derive(Clone)]
pub enum BarWidgetMode {
    NoText,
    Value,
    ValueWithMaxValue
}

pub struct BarWidget {
    state: BarWidgetState
}

impl BarWidget {
    pub fn construct(state: BarWidgetState) -> BarWidget {
        Self {
            state: state.clone()
        }
    }
}

impl WidgetRef for &BarWidget {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let text = match self.state.mode {
            BarWidgetMode::NoText => String::new(),
            BarWidgetMode::Value => self.state.value.to_string(),
            BarWidgetMode::ValueWithMaxValue => format!("{}/{}", self.state.value, self.state.max_value),
        };
        let charcount = text.chars().count() as u16;
        let has_text = charcount <= area.width;

        match self.state.direction {
            BarWidgetDirection::Horizontal => {
                let bar_length = area.width - charcount;
                let division = self.state.max_value as f64 / bar_length as f64;
                let subdivision = division as f64 / HORIZONTAL.len() as f64;
                if has_text {
                    for (i, character) in text.chars().enumerate() {
                        let cell = &mut buf[(area.x + i as u16, area.y + area.height / 2)];
                        cell.set_char(character);
                        cell.fg = self.state.color;
                    }
                }

                for i in 0..area.width-charcount {
                    for j in 0..area.height {
                        let cell = &mut buf[(i+charcount+area.x, j+area.y)];
                        if division * (i as f64 + 1.0) > self.state.value as f64{
                            let index = (self.state.value as f64 - (i as f64) * division) / subdivision;
                            cell.set_char(HORIZONTAL[index as usize]);
                        } else {
                            cell.set_char(FULL_BLOCK);
                        }
                        cell.fg = self.state.color;
                    }
                }
            },

            BarWidgetDirection::Vertical => {
                let bar_length = area.height - if has_text {1} else {0};
                let division = self.state.max_value as f64 / bar_length as f64;
                let subdivision = division as f64 / VERTICAL.len() as f64;
                if has_text {
                    for (i, character) in text.chars().enumerate() {
                        let cell = &mut buf[(area.width >> 1 - charcount >> 1 + i, 0)];
                        cell.set_char(character);
                        cell.fg = self.state.color;
                    }
                }

                for j in charcount-1..area.height {
                    for i in 0..area.width {
                        let cell = &mut buf[(i, j)];
                        if division * (i as f64 + 1.0) > self.state.value as f64{
                            let index = (self.state.value as f64 - (i as f64) * division) / subdivision;
                            cell.set_char(VERTICAL[index as usize]);
                        } else {
                            cell.set_char(FULL_BLOCK);
                        }
                        cell.fg = self.state.color;
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct BarWidgetState {
    pub mode: BarWidgetMode,
    pub color: Color,
    pub direction: BarWidgetDirection,
    pub value: u16,
    pub max_value: u16,
}
