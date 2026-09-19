use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    DefaultTerminal,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};
use verbsmith_core::{ExecutionOptions, HttpRequest, Workspace, execute};

pub fn run(workspace: Workspace) -> Result<()> {
    let mut terminal = ratatui::init();
    let result = run_loop(&mut terminal, workspace);
    ratatui::restore();
    result
}

fn run_loop(terminal: &mut DefaultTerminal, workspace: Workspace) -> Result<()> {
    let requests = workspace.requests()?;
    let environment = workspace.environment(None)?;
    let mut variables = workspace.variables(&environment);
    let mut redactions = environment.secrets.values().cloned().collect::<Vec<_>>();
    redactions.extend(super::resolve_secret_references(
        &workspace,
        &mut variables,
    )?);
    let mut selected = 0usize;
    let mut response = String::from("Press Enter to send the selected request.");

    loop {
        terminal.draw(|frame| {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
                .split(frame.area());
            let items = requests
                .iter()
                .map(|request| {
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("{:7}", request.method),
                            Style::default()
                                .fg(method_color(&request.method))
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(&request.name),
                    ]))
                })
                .collect::<Vec<_>>();
            let mut state =
                ListState::default().with_selected((!requests.is_empty()).then_some(selected));
            let list = List::new(items)
                .block(
                    Block::default()
                        .title(format!(" {} ", workspace.manifest.name))
                        .borders(Borders::ALL),
                )
                .highlight_style(Style::default().bg(Color::DarkGray));
            frame.render_stateful_widget(list, chunks[0], &mut state);

            let detail = requests.get(selected).map_or_else(
                || "No requests found. Add a .http file under requests/.".to_owned(),
                |request| format!("{} {}\n\n{}", request.method, request.url, response),
            );
            frame.render_widget(
                Paragraph::new(detail)
                    .block(
                        Block::default()
                            .title(" Request / Response ")
                            .borders(Borders::ALL),
                    )
                    .wrap(Wrap { trim: false }),
                chunks[1],
            );
        })?;

        if !event::poll(Duration::from_millis(200))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => break,
            KeyCode::Down | KeyCode::Char('j') if !requests.is_empty() => {
                selected = (selected + 1).min(requests.len() - 1)
            }
            KeyCode::Up | KeyCode::Char('k') if !requests.is_empty() => {
                selected = selected.saturating_sub(1)
            }
            KeyCode::Enter if !requests.is_empty() => {
                response = run_request(&requests[selected], variables.clone(), &redactions);
            }
            _ => {}
        }
    }
    Ok(())
}

fn run_request(
    request: &HttpRequest,
    variables: std::collections::BTreeMap<String, String>,
    redactions: &[String],
) -> String {
    match execute(
        request,
        &ExecutionOptions {
            variables,
            ..ExecutionOptions::default()
        },
    ) {
        Ok(response) => {
            let body = String::from_utf8_lossy(&response.body);
            let response = format!(
                "{} · {} ms\n\n{}",
                response.status, response.elapsed_ms, body
            );
            redactions
                .iter()
                .filter(|value| !value.is_empty())
                .fold(response, |text, value| text.replace(value, "[REDACTED]"))
        }
        Err(error) => format!("Request failed: {error}"),
    }
}

fn method_color(method: &str) -> Color {
    match method {
        "GET" => Color::Green,
        "POST" => Color::Yellow,
        "PUT" | "PATCH" => Color::Blue,
        "DELETE" => Color::Red,
        _ => Color::Cyan,
    }
}
