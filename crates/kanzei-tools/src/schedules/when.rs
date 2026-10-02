use chrono::{DateTime, Datelike, Duration, Local, TimeZone, Timelike};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum When {
    Minutes(u32),
    Hours(u32),
    Daily(u32, u32),
    Weekdays(u32, u32),
    Weekly(u32, u32, u32),
}

impl When {
    pub fn parse(text: &str) -> Result<Self, String> {
        let words: Vec<_> = text.split_whitespace().collect();
        let period = |n: &str, total: u32| {
            n.parse::<u32>()
                .ok()
                .filter(|n| *n > 0 && total.is_multiple_of(*n))
                .ok_or_else(|| format!("间隔必须整除 {total}"))
        };
        match words.as_slice() {
            ["每", n, "分钟"] => return Ok(Self::Minutes(period(n, 60)?)),
            ["每", n, "小时"] => return Ok(Self::Hours(period(n, 24)?)),
            _ => {}
        }
        let [kind, time] = words.as_slice() else {
            return Err(
                "频率格式：每 15 分钟 / 每 2 小时 / 每天 09:00 / 工作日 09:00 / 每周一 09:00"
                    .into(),
            );
        };
        let (h, m) = time.split_once(':').ok_or("时间格式应为 HH:MM")?;
        let h = h
            .parse::<u32>()
            .ok()
            .filter(|h| *h < 24)
            .ok_or("小时超出范围")?;
        let m = m
            .parse::<u32>()
            .ok()
            .filter(|m| *m < 60)
            .ok_or("分钟超出范围")?;
        match *kind {
            "每天" => Ok(Self::Daily(h, m)),
            "工作日" => Ok(Self::Weekdays(h, m)),
            value => {
                let day = value
                    .strip_prefix("每周")
                    .and_then(|day| match day {
                        "一" => Some(0),
                        "二" => Some(1),
                        "三" => Some(2),
                        "四" => Some(3),
                        "五" => Some(4),
                        "六" => Some(5),
                        "日" | "天" => Some(6),
                        _ => None,
                    })
                    .ok_or("未知频率")?;
                Ok(Self::Weekly(day, h, m))
            }
        }
    }
    fn matches<T: TimeZone>(&self, time: &DateTime<T>) -> bool {
        match *self {
            Self::Minutes(n) => time.minute().is_multiple_of(n),
            Self::Hours(n) => time.minute() == 0 && time.hour().is_multiple_of(n),
            Self::Daily(h, m) => time.hour() == h && time.minute() == m,
            Self::Weekdays(h, m) => {
                time.weekday().num_days_from_monday() < 5 && time.hour() == h && time.minute() == m
            }
            Self::Weekly(day, h, m) => {
                time.weekday().num_days_from_monday() == day
                    && time.hour() == h
                    && time.minute() == m
            }
        }
    }
    pub fn latest<T: TimeZone>(&self, now: DateTime<T>) -> DateTime<T> {
        let mut time = now.with_second(0).unwrap().with_nanosecond(0).unwrap();
        for _ in 0..11521 {
            if self.matches(&time) {
                return time;
            }
            time -= Duration::minutes(1);
        }
        unreachable!("valid weekly frequency must have a slot within eight days")
    }
    pub fn next<T: TimeZone>(&self, now: DateTime<T>) -> DateTime<T> {
        let mut time =
            now.with_second(0).unwrap().with_nanosecond(0).unwrap() + Duration::minutes(1);
        for _ in 0..11521 {
            if self.matches(&time) {
                return time;
            }
            time += Duration::minutes(1);
        }
        unreachable!("valid weekly frequency must have a slot within eight days")
    }
    pub fn latest_ms(&self, now: i64) -> i64 {
        self.latest(Local.timestamp_millis_opt(now).single().unwrap())
            .timestamp_millis()
    }
    pub fn next_ms(&self, now: i64) -> i64 {
        self.next(Local.timestamp_millis_opt(now).single().unwrap())
            .timestamp_millis()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_slots_align_across_midnight_and_weekends() {
        let tz = chrono::FixedOffset::east_opt(28800).unwrap();
        let friday = tz.with_ymd_and_hms(2026, 10, 2, 23, 58, 0).unwrap();
        let every = When::parse("每 15 分钟").unwrap();
        assert_eq!(every.next(friday).to_rfc3339(), "2026-10-03T00:00:00+08:00");
        assert_eq!(every.latest(friday).minute(), 45);
        assert_eq!(
            When::parse("工作日 09:00")
                .unwrap()
                .next(friday)
                .to_rfc3339(),
            "2026-10-05T09:00:00+08:00"
        );
        assert_eq!(When::parse("每周日 09:00").unwrap().next(friday).day(), 4);
        assert!(When::parse("每 7 分钟").is_err());
        assert!(When::parse("每天 25:00").is_err());
    }
}
