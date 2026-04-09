use chrono::{DateTime, Utc};

use crate::{error::WorkflowError, types::CronSchedule};

impl CronSchedule {
    /// Returns `true` if the cron expression can be parsed.
    pub fn is_valid(&self) -> bool {
        self.expression.parse::<cron::Schedule>().is_ok()
    }

    /// Returns the next fire time after now, or `None` if the expression is
    /// invalid or the schedule never fires again.
    pub fn next_fire(&self) -> Option<DateTime<Utc>> {
        self.next_fire_after(Utc::now())
    }

    /// Returns the next fire time strictly after `after`.
    pub fn next_fire_after(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let schedule = self.expression.parse::<cron::Schedule>().ok()?;
        schedule.after(&after).next()
    }

    /// Validates the expression, returning an error with context on failure.
    pub fn validate(&self) -> Result<(), WorkflowError> {
        self.expression.parse::<cron::Schedule>().map(|_| ()).map_err(|e| {
            WorkflowError::InvalidCronExpression {
                expression: self.expression.clone(),
                reason: e.to_string(),
            }
        })
    }
}
