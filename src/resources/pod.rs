pub mod proc;

use std::{borrow::Borrow, cmp::Ordering, error::Error, fmt::Display, net::IpAddr, sync::Arc};

use chrono::{TimeDelta, Utc};
use k8s_openapi::{
    api::core::v1::{
        ContainerState, ContainerStateTerminated, ContainerStateWaiting, ContainerStatus, Pod,
        PodStatus,
    },
    apimachinery::pkg::apis::meta::v1,
};
use kube::ResourceExt;
pub use proc::Proc;
use ratatui::{
    layout::Constraint,
    widgets::{Cell, Row},
};

use super::{
    age::Age,
    container::{Container, ContainerExt},
    Compare, Filter,
};
use crate::widget::{
    table::{Content, RowStyle},
    TableRow,
};

// TODO: There's probably a better debug implementation than this.
#[derive(Clone, Debug)]
pub struct StatusError {
    pub message: String,
}

// Because this is a golang error that's being returned, there's really no good
// way to convert this into something that is moderately usable. The rest of the
// `Status` struct is empty of anything useful. The decision is to be naive here
// and let other display handlers figure out if they would like to deal with the
// message.
impl StatusError {
    pub fn new(inner: v1::Status) -> Self {
        Self {
            message: inner.message.unwrap_or("unknown status".to_string()),
        }
    }
}

impl Error for StatusError {}

impl Display for StatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

pub trait StatusExt {
    fn is_success(&self) -> bool;
}

impl StatusExt for v1::Status {
    fn is_success(&self) -> bool {
        self.status == Some("Success".to_string())
    }
}

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

#[allow(clippy::module_name_repetitions)]
pub trait PodExt {
    fn age(&self) -> TimeDelta;
    fn ready(&self) -> String;
    fn restarts(&self) -> String;
    fn status(&self) -> Phase;
    fn containers(&self, filter: Option<String>) -> Vec<Container>;
    fn ip(&self) -> Option<IpAddr>;
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

    fn containers(&self, filter: Option<String>) -> Vec<Container> {
        let mut containers: Vec<Container> = self
            .spec
            .as_ref()
            .map(|spec| {
                spec.containers
                    .iter()
                    .map(|c| Container::new(self.clone(), c.clone()))
                    .collect()
            })
            .unwrap_or_default();

        let Some(PodStatus {
            container_statuses: Some(status),
            ..
        }) = &self.status
        else {
            return containers;
        };

        for status in status {
            if let Some(container) = containers.iter_mut().find(|c| c.name_any() == status.name) {
                container.with_status(status.clone());
            }
        }

        if filter.is_none() {
            return containers;
        }

        containers
            .into_iter()
            .filter(|c| filter.as_ref().map_or(true, |f| c.name_any().contains(f)))
            .collect()
    }

    fn ip(&self) -> Option<IpAddr> {
        let Some(status) = &self.status else {
            return None;
        };

        let Some(pod_ip) = &status.pod_ip else {
            return None;
        };

        pod_ip.parse().ok()
    }
}

impl<'a> TableRow<'a> for Arc<Pod> {
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

    fn row(&self, style: &RowStyle) -> Row {
        Row::new(vec![
            self.namespace().unwrap_or_default(),
            self.name_any(),
            self.ready(),
            self.status().to_string(),
            self.restarts(),
            self.age().to_age(),
        ])
        .style(match self.status() {
            Phase::Pending | Phase::Running => style.normal,
            Phase::Succeeded => style.healthy,
            Phase::Unknown(_) => style.unhealthy,
        })
    }
}

impl Filter for Pod {
    fn matches(&self, filter: &str) -> bool {
        self.name_any().contains(filter)
    }
}

impl Compare for Arc<Pod> {
    fn cmp(&self, other: &Self) -> Ordering {
        let lhs = self
            .namespace()
            .unwrap_or_default()
            .cmp(&other.namespace().unwrap_or_default());

        if lhs != Ordering::Equal {
            return lhs;
        }

        self.name_any().cmp(&other.name_any())
    }
}

impl<'a> Content<'a, Container> for Arc<Pod> {
    fn items(&self, filter: Option<String>) -> Vec<impl TableRow<'a>> {
        self.containers(filter)
    }
}
