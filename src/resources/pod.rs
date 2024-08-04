use std::borrow::Borrow;

use chrono::{format::strftime, TimeDelta, Utc};
use humantime::format_duration;
use k8s_openapi::api::core::v1::{
    ContainerState, ContainerStateTerminated, ContainerStateWaiting, ContainerStatus, Pod,
};
use kube::ResourceExt;
use ratatui::{
    layout::Constraint,
    widgets::{Cell, Row},
};

use crate::widget::TableRow;

pub enum Phase {
    Pending,
    Running,
    Succeeded,
    Unknown(String),
}

impl From<&Option<String>> for Phase {
    fn from(s: &Option<String>) -> Self {
        match s {
            Some(s) => match s.as_str() {
                "Pending" => Phase::Pending,
                "Running" => Phase::Running,
                "Succeeded" => Phase::Succeeded,
                _ => Phase::Unknown(s.clone()),
            },
            None => Phase::Unknown("Unknown".to_string()),
        }
    }
}

impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Phase::Pending => write!(f, "Pending"),
            Phase::Running => write!(f, "Running"),
            Phase::Succeeded => write!(f, "Succeeded"),
            Phase::Unknown(s) => write!(f, "{s}"),
        }
    }
}

pub trait PodExt {
    fn age(&self) -> TimeDelta;
    fn ready(&self) -> String;
    fn restarts(&self) -> String;
    fn status(&self) -> Phase;
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

    fn status(&self) -> Phase {
        let Some(status) = &self.status else {
            return Some(String::new()).borrow().into();
        };

        let Some(containers) = &status.container_statuses else {
            return status.phase.borrow().into();
        };

        let statuses = containers
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    ContainerStatus {
                        state: Some(ContainerState {
                            waiting: Some(_),
                            ..
                        }),
                        ..
                    }
                )
            })
            .map(|c| match &c.state {
                Some(
                    ContainerState {
                        waiting:
                            Some(ContainerStateWaiting {
                                reason: Some(x), ..
                            }),
                        ..
                    }
                    | ContainerState {
                        terminated:
                            Some(ContainerStateTerminated {
                                reason: Some(x), ..
                            }),
                        ..
                    },
                ) => x.clone(),
                _ => "unknown".to_string(),
            })
            .collect::<Vec<String>>();

        if statuses.is_empty() {
            return status.phase.borrow().into();
        }

        Some(statuses.join(", ")).borrow().into()
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
            Constraint::Max(20),
            Constraint::Min(10),
            Constraint::Max(10),
            Constraint::Max(10),
            Constraint::Max(10),
            Constraint::Max(10),
        ]
    }

    fn row(&self) -> Row {
        Row::new(vec![
            Cell::from(self.namespace().unwrap()),
            Cell::from(self.name_any()),
            Cell::from(self.ready()),
            Cell::from(self.status().to_string()),
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
