use color_eyre::{Section, SectionExt};
use eyre::{eyre, Report};
use itertools::Itertools;
use k8s_openapi::apimachinery::pkg::apis::meta::v1;

#[allow(clippy::module_name_repetitions)]
pub trait StatusExt {
    fn is_success(&self) -> bool;
    fn into_report(self) -> Report;
}

impl StatusExt for v1::Status {
    fn is_success(&self) -> bool {
        self.status == Some("Success".to_string())
    }

    // Because this is a golang error that's being returned, there's really no good
    // way to convert this into something that is moderately usable. The rest of the
    // `Status` struct is empty of anything useful. The decision is to be naive here
    // and let other display handlers figure out if they would like to deal with the
    // message.
    fn into_report(self) -> Report {
        let msg = self
            .message
            .as_ref()
            .map_or("unknown status", |s| s.as_str());
        let separated = msg.splitn(8, ':');

        eyre!(
            "{}",
            separated.clone().last().unwrap_or("unknown error").trim()
        )
        .section(
            separated
                .with_position()
                .map(|(i, line)| {
                    let l = line.trim().to_string();

                    match i {
                        itertools::Position::Middle => format!("├─ {l}"),
                        itertools::Position::Last => format!("└─ {l}"),
                        _ => l,
                    }
                })
                .join("\n")
                .header("Raw:"),
        )
    }
}
