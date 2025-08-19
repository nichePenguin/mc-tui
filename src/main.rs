#![feature(int_roundings)]
#![feature(let_chains)]

mod buffered_reader;
mod packets;
mod nbt;
mod net;
mod util;
mod world;
mod game;
mod ui;
mod log;

use ratatui::{
    layout::{Layout, Constraint, Flex},
    prelude::Direction,
    style::{Style, Color},
    widgets::{
        Block, BorderType, Borders,
        List, ListDirection
    },
};
use tokio::time::{interval, Duration};
use std::path::PathBuf;
use std::error::Error;
use std::sync::Arc;

use ui::UiState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    log::info!("Application started");
    let mut global_ctx = game::GlobalContext::init(PathBuf::from("resources"));
    for i in 1..9 {
        let player = game::Player::start("localhost", 25565, format!("UristMc_{}", i)).await?;
        global_ctx.add_player(player, false).await;
    }
    global_ctx.set_active_player(0).await;

    let ui_state = UiState::init();

    let draw_join = draw_loop(Arc::clone(&ui_state));
    let game_join = game_loop(ui_state,  global_ctx);
    tokio::join!(game_join, draw_join).0.unwrap();
    Ok(())
}

fn game_loop(
    ui_state: Arc<UiState>,
    ctx: game::GlobalContext,) 
    -> tokio::task::JoinHandle<()> 
{
    let mut ctx = ctx;
    let mut interval = interval(Duration::from_millis(50));
    tokio::task::spawn(async move {
        loop {
            if ctx.stop {
                break;
            }
            ctx.tick().await;
            ctx.update_render(&ui_state).await;
            interval.tick().await;
        }
        ui_state.stop();
    })
}

fn draw_loop(ui_state: Arc<UiState>) -> tokio::task::JoinHandle<()> {
    let mut terminal = ratatui::init();
    let mut interval = interval(Duration::from_millis(16));
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![
            Constraint::Percentage(75),
            Constraint::Percentage(25),
        ]);
    let center_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![
            Constraint::Fill(1),
            Constraint::Percentage(50),
            Constraint::Fill(1),
        ]);
    let bottom_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![
            Constraint::Fill(1),
            Constraint::Length(3)
        ])
        .flex(Flex::End);
    let bar_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![
            Constraint::Length(1),
            Constraint::Length(1),
        ]);
    let world_widget = ui::WorldWidget::new();
    let mut tick = 0;
    tokio::task::spawn(async move {
        loop {
            if tick == std::usize::MAX {
                tick = 0;
            }
            tick += 1;
            if ui_state.is_stop() {
                break;
            }
            let block = Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Rgb(194,255,102)));
            let bar_block = Block::bordered()
                .borders(Borders::ALL & !Borders::BOTTOM)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Rgb(194,255,102)));
            let log_widget = List::new(log::lines(16, log::LogLevel::Info))
                .block(block)
                .direction(ListDirection::BottomToTop);
            {
                let food_bar = ui::BarWidget::construct(ui_state.food_bar.read().await.clone());
                let hp_bar = ui::BarWidget::construct(ui_state.hp_bar.read().await.clone());
                let world_state = &mut ui_state.world_state.write().await;
                let entity_state = ui_state.entity_state.read().await;
                terminal.draw(|frame| {
                    let layout = main_layout.split(frame.area());
                    let bar_area = bottom_layout.split(
                        center_layout.split(layout[0])[1])[1];
                    let inner_bar_area = bar_block.inner(bar_area);
                    let inner_bar_areas = bar_layout.split(inner_bar_area);
                    let entity_widget = ui::EntityOverlayWidget::new(&entity_state, tick);
                    frame.render_stateful_widget_ref(&world_widget, layout[0], world_state);
                    frame.render_widget_ref(&entity_widget, layout[0]);
                    frame.render_widget(log_widget.clone(), layout[1]);
                    frame.render_widget_ref(bar_block, bar_area);
                    frame.render_widget_ref(&hp_bar, inner_bar_areas[0]);
                    frame.render_widget_ref(&food_bar, inner_bar_areas[1]);
                }).map_err(|e| format!("Draw call failed: {}", e)).unwrap();
            }
            interval.tick().await;
        }
        ratatui::restore();
    })
}
