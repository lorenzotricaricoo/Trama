//! Token usage of language models, its cost and the monthly budget.
//!
//! Every reply a provider sends is recorded with the tokens it reported
//! ([`Library::record_usage`]); its cost comes from the model's prices
//! ([`crate::storage::ModelPrices`]) at that moment. The month's spend is compared with the
//! budget in the [`UsageSettings`](crate::storage::UsageSettings).

use rusqlite::params;

use super::providers::load_model_prices;
use super::{Id, Library, ModelTask, Result, StorageError, Timestamp};

/// Tokens a provider reported for one reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageRecord {
    pub provider_id: Id,
    pub model_id: String,
    /// The task the call was made for, if any.
    pub task: Option<ModelTask>,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Usage added up over a period.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageTotals {
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Cost of the requests whose model had prices, in millionths of the currency unit.
    pub cost_micros: u64,
    /// Requests whose cost is unknown because their model had no price: the real spend
    /// is higher than [`UsageTotals::cost_micros`].
    pub unpriced_requests: u64,
}

text_enum! {
    /// Where the month's spend stands against the budget.
    BudgetState {
        /// No budget is set.
        NoBudget = "no_budget",
        /// Below the warning threshold.
        Ok = "ok",
        /// At or above the warning threshold, below the budget.
        Warning = "warning",
        /// At or above the budget.
        Exceeded = "exceeded",
    }
}

/// The month's spend against the budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetStatus {
    /// Start of the current month (UTC), from which usage is added up.
    pub period_start: Timestamp,
    pub totals: UsageTotals,
    pub budget_micros: Option<u64>,
    pub warn_percent: u8,
    /// Calls are refused while [`BudgetStatus::state`] is [`BudgetState::Exceeded`].
    pub enforce: bool,
    pub state: BudgetState,
}

impl BudgetStatus {
    /// Whether calls must be refused now.
    pub fn blocks_calls(&self) -> bool {
        self.enforce && self.state == BudgetState::Exceeded
    }
}

/// Where `spent` stands against `budget` with a warning at `warn_percent`.
pub fn budget_state(spent: u64, budget: Option<u64>, warn_percent: u8) -> BudgetState {
    let Some(budget) = budget else {
        return BudgetState::NoBudget;
    };
    if spent >= budget {
        BudgetState::Exceeded
    } else if u128::from(spent) * 100 >= u128::from(budget) * u128::from(warn_percent) {
        BudgetState::Warning
    } else {
        BudgetState::Ok
    }
}

/// Midnight UTC of the first day of the month that contains `at`.
pub fn month_start(at: Timestamp) -> Timestamp {
    const DAY_MS: i64 = 24 * 60 * 60 * 1000;
    let days = at.div_euclid(DAY_MS);
    let (year, month, _) = civil_from_days(days);
    days_from_civil(year, month, 1) * DAY_MS
}

/// Year, month (1–12) and day of a count of days since 1970-01-01 (H. Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Days since 1970-01-01 of a date (H. Hinnant's algorithm).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let month = i64::from(month);
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn to_sql(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| StorageError::Invalid("token count too large"))
}

impl Library {
    /// Records one reply's tokens and returns its cost, `None` when the model has no
    /// price. A model the library doesn't know (removed meanwhile) is recorded unpriced.
    pub fn record_usage(&mut self, record: &UsageRecord) -> Result<Option<u64>> {
        let prices = load_model_prices(&self.conn, record.provider_id, &record.model_id)?;
        let cost = prices.and_then(|p| p.cost_micros(record.input_tokens, record.output_tokens));
        let now = self.now();
        self.conn.execute(
            "INSERT INTO model_usage
                 (at, provider_id, model_id, task, input_tokens, output_tokens, cost_micros)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                now,
                record.provider_id.to_string(),
                record.model_id,
                record.task.map(ModelTask::as_str),
                to_sql(record.input_tokens)?,
                to_sql(record.output_tokens)?,
                cost.map(to_sql).transpose()?,
            ],
        )?;
        Ok(cost)
    }

    /// Usage recorded from `since` on.
    pub fn usage_since(&self, since: Timestamp) -> Result<UsageTotals> {
        let totals = self.conn.query_row(
            "SELECT COUNT(*), TOTAL(input_tokens), TOTAL(output_tokens), TOTAL(cost_micros),
                    COUNT(*) - COUNT(cost_micros)
             FROM model_usage WHERE at >= ?1",
            [since],
            |row| {
                // TOTAL() is a float that never overflows; it is exact well past any real use.
                let total = |index| row.get::<_, f64>(index).map(|n| n as u64);
                Ok(UsageTotals {
                    requests: row.get::<_, i64>(0)?.max(0) as u64,
                    input_tokens: total(1)?,
                    output_tokens: total(2)?,
                    cost_micros: total(3)?,
                    unpriced_requests: row.get::<_, i64>(4)?.max(0) as u64,
                })
            },
        )?;
        Ok(totals)
    }

    /// The current month's spend against the budget.
    pub fn budget_status(&self) -> Result<BudgetStatus> {
        let usage = self.library_settings()?.usage;
        let period_start = month_start(self.now());
        let totals = self.usage_since(period_start)?;
        Ok(BudgetStatus {
            period_start,
            totals,
            budget_micros: usage.monthly_budget_micros,
            warn_percent: usage.warn_percent,
            enforce: usage.enforce_budget,
            state: budget_state(
                totals.cost_micros,
                usage.monthly_budget_micros,
                usage.warn_percent,
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 24 * 60 * 60 * 1000;

    #[test]
    fn months_start_at_midnight_utc_on_the_first() {
        // 2026-11-28T10:00:00Z → 2026-11-01T00:00:00Z.
        assert_eq!(month_start(1_795_860_000_000), 1_793_491_200_000);
        // The first instant of a month is its own start.
        assert_eq!(month_start(1_793_491_200_000), 1_793_491_200_000);
        // One millisecond before belongs to October.
        assert_eq!(month_start(1_793_491_200_000 - 1), 1_790_812_800_000);
        // 2024-02-29 (leap year) → 2024-02-01.
        assert_eq!(month_start(1_709_164_800_000), 1_706_745_600_000);
        assert_eq!(month_start(0), 0);
        // Before the epoch: 1969-12-31 → 1969-12-01.
        assert_eq!(month_start(-DAY), -31 * DAY);
    }

    #[test]
    fn civil_dates_round_trip() {
        for days in [-1000, -1, 0, 1, 59, 365, 10_957, 20_000, 60_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
    }

    #[test]
    fn the_budget_state_follows_the_spend() {
        assert_eq!(budget_state(10, None, 80), BudgetState::NoBudget);
        assert_eq!(budget_state(0, Some(100), 80), BudgetState::Ok);
        assert_eq!(budget_state(79, Some(100), 80), BudgetState::Ok);
        assert_eq!(budget_state(80, Some(100), 80), BudgetState::Warning);
        assert_eq!(budget_state(100, Some(100), 80), BudgetState::Exceeded);
        assert_eq!(budget_state(0, Some(0), 80), BudgetState::Exceeded);
        assert_eq!(budget_state(99, Some(100), 100), BudgetState::Ok);
    }
}
