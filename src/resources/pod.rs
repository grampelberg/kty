use chrono::{format::strftime, TimeDelta, Utc};
use humantime::format_duration;
use k8s_openapi::api::core::v1::{ContainerStatus, Pod};
use kube::ResourceExt;
use ratatui::{
    backend::{self, CrosstermBackend},
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Rect},
    terminal::TerminalOptions,
    text::Text,
    widgets::{
        self, Block, BorderType, Borders, Cell, Clear, Paragraph, Row, Table, Widget, WidgetRef,
    },
    Frame, Terminal, Viewport,
};
use tracing::info;

use crate::widget::TableRow;

pub trait PodExt {
    fn age(&self) -> TimeDelta;
    fn ready(&self) -> String;
    fn restarts(&self) -> String;
    fn status(&self) -> String;
}

impl PodExt for Pod {
    fn age(&self) -> TimeDelta {
        let Some(creation) = self.creation_timestamp() else {
            return TimeDelta::zero();
        };

        Utc::now() - creation.0
    }

    fn ready(&self) -> String {
        let Some(status) = &self.status else {
            return "0/0".to_string();
        };

        let Some(containers) = &status.container_statuses else {
            return "0/0".to_string();
        };

        let ready = containers.iter().fold(0, |a, c| a + i32::from(c.ready));

        let total = containers.len();

        format!("{ready}/{total}")
    }

    fn restarts(&self) -> String {
        let Some(status) = &self.status else {
            return "0".to_string();
        };

        let Some(containers) = &status.container_statuses else {
            return "0".to_string();
        };

        let total = containers.iter().fold(0, |a, c| a + c.restart_count);

        let recent = containers
            .iter()
            .fold(chrono::DateTime::<Utc>::MIN_UTC, |a, c| {
                let Some(last_state) = &c.last_state else {
                    return a;
                };

                let Some(terminated) = &last_state.terminated else {
                    return a;
                };

                let Some(finished) = &terminated.finished_at else {
                    return a;
                };

                a.max(finished.0)
            });

        if recent == chrono::DateTime::<Utc>::MIN_UTC {
            return total.to_string();
        }

        format!("{total} ({})", (Utc::now() - recent).to_age())
    }

    fn status(&self) -> String {
        let Some(status) = &self.status else {
            return String::new();
        };

        match &status.phase {
            Some(phase) => phase.clone(),
            None => String::new(),
        }
    }
}

impl<'a> TableRow<'a> for Pod {
    fn header() -> Row<'a> {
        Row::new(vec![
            Cell::from("Namespace"),
            Cell::from("Name"),
            Cell::from("Ready"),
            Cell::from("Status"),
            Cell::from("Restarts"),
            Cell::from("Age"),
        ])
    }

    fn constraints() -> Vec<Constraint> {
        vec![
            Constraint::Min(10),
            Constraint::Min(10),
            Constraint::Min(3),
            Constraint::Min(10),
            Constraint::Min(1),
            Constraint::Min(10),
        ]
    }

    fn row(&self) -> Row {
        Row::new(vec![
            Cell::from(self.name_any()),
            Cell::from(self.namespace().unwrap()),
            Cell::from(self.ready()),
            Cell::from(self.status()),
            Cell::from(self.restarts()),
            Cell::from(self.age().to_age()),
        ])
    }
}

// TODO: this should probably be moved somewhere it can be used by other widgets
trait Age {
    fn to_age(&self) -> String;
}

impl Age for TimeDelta {
    fn to_age(&self) -> String {
        let mut out = vec![];

        if self.num_weeks() != 0 {
            out.push(format!("{}w", self.num_weeks()));
        }

        let days = self.num_days() % 7;
        if days != 0 {
            out.push(format!("{days}d"));
        }

        let hrs = self.num_hours() % 24;
        if hrs != 0 {
            out.push(format!("{hrs}h"));
        }

        let mins = self.num_minutes() % 60;
        if mins != 0 {
            out.push(format!("{mins}m"));
        }

        let secs = self.num_seconds() % 60;
        if secs != 0 {
            out.push(format!("{secs}s"));
        }

        if out.is_empty() {
            return "0s".to_string();
        }

        out.into_iter().take(2).collect::<String>()
    }
}
