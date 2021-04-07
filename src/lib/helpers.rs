extern crate syslog;
extern crate hostname;

use chrono::{TimeZone, Utc};
use std::convert::TryFrom;
use std::process::id;
use std::str::FromStr;
use super::errors::loc_syslog;
use syslog::{Severity, Facility, Formatter3164, Logger, LoggerBackend};

#[macro_export]
macro_rules! sleep_ms {
    ($n:expr) => {
        let n: u64 = $n;
        std::thread::sleep(std::time::Duration::from_millis(n));
    }
}

pub fn syslog_severity_from_str(sev_str: &str) -> loc_syslog::Result<Severity> {
    let result = match &sev_str.to_lowercase()[..] {
        "log_info" | "info" => Severity::LOG_INFO,
        "log_emerg" | "emerg" => Severity::LOG_EMERG,
        "log_alert" | "alert" => Severity::LOG_ALERT,
        "log_crit" | "crit" => Severity::LOG_CRIT,
        "log_err" | "err" => Severity::LOG_ERR,
        "log_warning" | "warning" | "warn" => Severity::LOG_WARNING,
        "log_notice" | "notice" => Severity::LOG_NOTICE,
        "log_debug" | "debug" => Severity::LOG_DEBUG,
        _ => return Err(loc_syslog::SyslogError::new(
            format!("Invalid syslog priority: {}", sev_str)
        )),
    };

    return Ok(result);
}

/// This is just a simple helper struct around the syslog library so it's
/// a bit easier to use
pub struct SyslogHelper {
    severity: Severity,
    logger: Logger<LoggerBackend, Formatter3164>,
}

impl SyslogHelper {
    pub fn new(severity: &str, facility: &str) -> Self {
        let loc_hostname = match hostname::get() {
            Ok(name) => Some(name.into_string().unwrap()),
            Err(_) => None,
        };

        let sev = syslog_severity_from_str(severity).ok().unwrap();

        let formatter = Formatter3164 {
            facility: Facility::from_str(facility).unwrap(),
            hostname: loc_hostname,
            process: "cwrap".to_string(),
            pid: i32::try_from(id()).ok().unwrap(),
        };
        
        let writer = syslog::unix(formatter).ok().unwrap();

        return SyslogHelper {
            severity: sev,
            logger: writer,
        };
    }

    #[allow(unused_must_use)]
    pub fn log<S: Into<String>>(&mut self, msg: S) {
        let m = msg.into();
        match self.severity {
            Severity::LOG_INFO => self.logger.info(m),
            Severity::LOG_EMERG => self.logger.emerg(m),
            Severity::LOG_ALERT => self.logger.alert(m),
            Severity::LOG_CRIT => self.logger.crit(m),
            Severity::LOG_ERR => self.logger.err(m),
            Severity::LOG_WARNING => self.logger.warning(m),
            Severity::LOG_NOTICE => self.logger.notice(m),
            Severity::LOG_DEBUG => self.logger.debug(m),
        };
    }
}

/// Return a formatted timestamp string
pub fn format_ts(ts: f64) -> String {
    // hacky, but it works
    let fsecs = ts.floor();
    let secs: i64 = fsecs.round() as i64;
    let nsecs: u32 = ((ts - fsecs) * 1_000_000_000.0).round() as u32;

    let dt = Utc.timestamp(secs, nsecs);

    return dt.to_rfc2822();
}

/// Check for a "/" in the cmd, and if it's there, just get the
/// binary name
pub fn basename(path: &str) -> String {
    return match path.find("/") {
        Some(_) => {
            let idx = path.rfind("/").unwrap() + 1;  // After the last "/"
            path[idx..].to_string()
        },
        None => path.to_string(),
    };
}