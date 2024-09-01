pub mod file;

use chrono::Utc;
#[allow(clippy::module_name_repetitions)]
pub use file::ContainerFiles;
use k8s_openapi::api::core::v1::{
    self, ContainerState, ContainerStateRunning, ContainerStateTerminated, ContainerStateWaiting,
    ContainerStatus, Pod,
};
use kube::ResourceExt;
use ratatui::{
    layout::Constraint,
    widgets::{Cell, Row},
};

use super::{age::Age, Compare};
use crate::widget::{table::RowStyle, TableRow};

#[allow(clippy::module_name_repetitions)]
pub trait ContainerExt {
    fn name_any(&self) -> String;
    fn namespace(&self) -> Option<String>;
    fn pod_name(&self) -> String;
    fn image(&self) -> &str;
    fn state(&self) -> State;
    fn restarts(&self) -> String;
    fn age(&self) -> String;
    fn ready(&self) -> String;
}

#[derive(Default)]
pub enum State {
    Running,
    Terminated(String),
    Waiting(String),
    #[default]
    Unknown,
}

impl std::fmt::Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            State::Running => write!(f, "Running"),
            State::Terminated(s) => write!(f, "Terminated ({s})"),
            State::Waiting(s) => write!(f, "Waiting ({s})"),
            State::Unknown => write!(f, "Unknown"),
        }
    }
}

impl From<&ContainerStateRunning> for State {
    fn from(_: &ContainerStateRunning) -> Self {
        State::Running
    }
}

impl From<&ContainerStateTerminated> for State {
    fn from(terminated: &ContainerStateTerminated) -> Self {
        State::Terminated(terminated.reason.clone().unwrap_or_default())
    }
}

impl From<&ContainerStateWaiting> for State {
    fn from(waiting: &ContainerStateWaiting) -> Self {
        State::Waiting(waiting.reason.clone().unwrap_or_default())
    }
}

impl From<Option<&ContainerState>> for State {
    fn from(state: Option<&ContainerState>) -> Self {
        match state {
            Some(ContainerState {
                running: Some(running),
                ..
            }) => State::from(running),
            Some(ContainerState {
                terminated: Some(terminated),
                ..
            }) => State::from(terminated),
            Some(ContainerState {
                waiting: Some(waiting),
                ..
            }) => State::from(waiting),
            _ => State::Unknown,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Container {
    pod: Pod,
    spec: v1::Container,
    status: Option<ContainerStatus>,
}

impl Container {
    pub fn new(pod: Pod, spec: v1::Container) -> Self {
        Self {
            pod,
            spec,
            status: None,
        }
    }

    pub fn with_status(&mut self, status: ContainerStatus) -> &mut Self {
        self.status = Some(status);

        self
    }
}

impl ContainerExt for Container {
    fn name_any(&self) -> String {
        self.spec.name.clone()
    }

    fn namespace(&self) -> Option<String> {
        self.pod.namespace()
    }

    fn pod_name(&self) -> String {
        self.pod.name_any()
    }

    fn image(&self) -> &str {
        match self {
            Container {
                spec: v1::Container {
                    image: Some(image), ..
                },
                ..
            }
            | Container {
                status: Some(v1::ContainerStatus { image, .. }),
                ..
            } => image,
            _ => "-",
        }
    }

    fn state(&self) -> State {
        self.status
            .as_ref()
            .map(|status| State::from(status.state.as_ref()))
            .unwrap_or_default()
    }

    fn restarts(&self) -> String {
        let Some(status) = self.status.as_ref() else {
            return "-".to_string();
        };

        let Some(ContainerState {
            terminated:
                Some(ContainerStateTerminated {
                    finished_at: Some(finished_at),
                    ..
                }),
            ..
        }) = status.last_state.as_ref()
        else {
            return format!("{}", status.restart_count);
        };

        format!(
            "{} ({})",
            status.restart_count,
            (Utc::now() - finished_at.0).to_age()
        )
    }

    fn age(&self) -> String {
        let Some(ContainerStatus {
            state:
                Some(ContainerState {
                    running:
                        Some(ContainerStateRunning {
                            started_at: Some(started_at),
                        }),
                    ..
                }),
            ..
        }) = self.status.as_ref()
        else {
            return "-".to_string();
        };

        (Utc::now() - started_at.0).to_age()
    }

    fn ready(&self) -> String {
        match self.status.as_ref() {
            Some(ContainerStatus { ready: true, .. }) => "Yes".to_string(),
            _ => "No".to_string(),
        }
    }
}

impl<'a> TableRow<'a> for Container {
    fn header() -> Row<'a> {
        Row::new(vec![
            Cell::from("Name"),
            Cell::from("Image"),
            Cell::from("Ready"),
            Cell::from("State"),
            Cell::from("Restarts"),
            Cell::from("Age"),
        ])
    }

    fn constraints() -> Vec<Constraint> {
        vec![
            Constraint::Max(20),
            Constraint::Min(10),
            Constraint::Max(5),
            Constraint::Max(10),
            Constraint::Max(10),
            Constraint::Max(10),
        ]
    }

    fn row(&self, style: &RowStyle) -> Row {
        Row::new(vec![
            Cell::from(self.name_any()),
            Cell::from(self.image()),
            Cell::from(self.ready()),
            Cell::from(self.state().to_string()),
            Cell::from(self.restarts()),
            Cell::from(self.age()),
        ])
        .style(match self.state() {
            State::Running | State::Waiting(_) => style.normal,
            _ => style.unhealthy,
        })
    }
}

impl Compare for Container {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.name_any().cmp(&other.name_any())
    }
}
