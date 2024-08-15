use chrono::TimeDelta;

pub trait Age {
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
