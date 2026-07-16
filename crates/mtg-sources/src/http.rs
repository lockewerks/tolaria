//! Shared HTTP plumbing: agent construction, JSON and byte fetches with a
//! light retry.

use std::time::Duration;

use crate::SourceError;

pub fn agent(user_agent: &str) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .user_agent(user_agent)
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(120))
        .build()
}

fn map_err(e: ureq::Error) -> SourceError {
    SourceError::Http(e.to_string())
}

pub fn get_json<T: serde::de::DeserializeOwned>(
    agent: &ureq::Agent,
    url: &str,
) -> Result<T, SourceError> {
    let mut last = None;
    for _ in 0..2 {
        match agent.get(url).set("Accept", "application/json").call() {
            Ok(resp) => return Ok(resp.into_json()?),
            Err(e) => last = Some(map_err(e)),
        }
    }
    Err(last.unwrap())
}

pub fn get_bytes(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, SourceError> {
    let mut last = None;
    for _ in 0..2 {
        match agent.get(url).call() {
            Ok(resp) => {
                let mut buf = Vec::new();
                use std::io::Read;
                resp.into_reader()
                    .take(1 << 30)
                    .read_to_end(&mut buf)
                    .map_err(SourceError::Io)?;
                return Ok(buf);
            }
            Err(e) => last = Some(map_err(e)),
        }
    }
    Err(last.unwrap())
}

pub fn get_string(agent: &ureq::Agent, url: &str) -> Result<String, SourceError> {
    Ok(String::from_utf8_lossy(&get_bytes(agent, url)?).into_owned())
}

/// Days since the unix epoch for a y/m/d, via Howard Hinnant's
/// days-from-civil. Avoids a chrono dependency.
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as i64;
    let mp = ((m as i64) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + (d as i64) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn today_days() -> i64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    (secs / 86_400) as i64
}
