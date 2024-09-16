use std::{cmp::Ordering, str::FromStr, sync::Arc};

use chrono::{TimeDelta, Utc};
use itertools::Itertools;
use k8s_openapi::api::core::v1::{Node, NodeSpec};
use kube::ResourceExt;
use ratatui::{
    layout::Constraint,
    widgets::{Cell, Row},
};
use strum::{Display, EnumString};

use super::{age::Age, Compare, Filter};
use crate::widget::table;

#[derive(EnumString, Display)]
pub enum Status {
    MemoryPressure,
    DiskPressure,
    PIDPressure,
    Ready,
    Unknown,
    SchedulingDisabled,
    Error(String),
}

#[allow(clippy::module_name_repetitions)]
pub trait NodeExt {
    fn age(&self) -> TimeDelta;
    fn instance_type(&self) -> String;
    fn roles(&self) -> Vec<String>;
    fn status(&self) -> Vec<Status>;
    fn version(&self) -> String;
}

impl NodeExt for Node {
    fn age(&self) -> TimeDelta {
        let Some(creation) = self.creation_timestamp() else {
            return TimeDelta::zero();
        };

        Utc::now() - creation.0
    }

    fn instance_type(&self) -> String {
        self.metadata
            .labels
            .as_ref()
            .and_then(|labels| labels.get("beta.kubernetes.io/instance-type"))
            .map(ToString::to_string)
            .unwrap_or_default()
    }

    fn roles(&self) -> Vec<String> {
        self.metadata
            .labels
            .as_ref()
            .map(|labels| {
                labels
                    .iter()
                    .filter_map(|(k, _)| {
                        let mut split = k.rsplitn(2, '/');

                        let role = split.next().unwrap_or_default();
                        let key = split.next().unwrap_or_default();

                        if key == "node-role.kubernetes.io" {
                            Some(role.to_string())
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn status(&self) -> Vec<Status> {
        let mut status = self
            .status
            .as_ref()
            .and_then(|s| {
                s.conditions.as_ref().map(|c| {
                    c.iter()
                        .filter_map(|c| {
                            if c.status == "True" {
                                Some(
                                    Status::from_str(c.type_.as_str())
                                        .unwrap_or(Status::Error(c.type_.to_string())),
                                )
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                })
            })
            .unwrap_or_default();

        if status.is_empty() {
            status.push(Status::Unknown);
        }

        if let Some(NodeSpec {
            unschedulable: Some(true),
            ..
        }) = self.spec
        {
            status.push(Status::SchedulingDisabled);
        }

        status
    }

    fn version(&self) -> String {
        self.status
            .as_ref()
            .and_then(|s| s.node_info.as_ref().map(|n| n.kubelet_version.to_string()))
            .unwrap_or_default()
    }
}

impl table::Row for Arc<Node> {
    fn header<'a>() -> Option<Row<'a>> {
        Some(Row::new(vec![
            Cell::from("Name"),
            Cell::from("Status"),
            Cell::from("Roles"),
            Cell::from("Type"),
            Cell::from("Version"),
            Cell::from("Age"),
        ]))
    }

    fn constraints() -> Vec<Constraint> {
        vec![
            Constraint::Max(20),
            Constraint::Max(30),
            Constraint::Fill(1),
            Constraint::Max(10),
            Constraint::Max(10),
            Constraint::Max(10),
        ]
    }

    fn row(&self, style: &table::RowStyle) -> Row {
        let status = self.status();

        Row::new(vec![
            self.name_any(),
            status.iter().join(", "),
            self.roles().join(", "),
            self.instance_type(),
            self.version(),
            self.age().to_age(),
        ])
        .style(status.iter().fold(style.normal, |acc, s| match s {
            Status::Ready => style.healthy,
            _ => acc,
        }))
    }
}

impl Filter for Node {
    fn matches(&self, filter: &str) -> bool {
        self.name_any().contains(filter)
    }
}

impl Compare for Arc<Node> {
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
