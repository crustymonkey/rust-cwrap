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
    let secs: i64 = ts.round() as i64;

    let dt = Utc.timestamp_opt(secs, 0).unwrap();

    return dt.to_rfc2822();
}

/// Check for a "/" in the cmd, and if it's there, just get the
/// binary name
#[allow(dead_code)]
pub fn basename(path: &str) -> String {
    return match path.find("/") {
        Some(_) => {
            let idx = path.rfind("/").unwrap() + 1;  // After the last "/"
            path[idx..].to_string()
        },
        None => path.to_string(),
    };
}

/// Convert a path from something like "/path/to/thing" to path-to-thing (or
/// whatever is set for the separator)
pub fn sanitize_path(path: &str, sep: char) -> String {
    let mut ret = path.replace("/", &sep.to_string());
    if ret.starts_with(sep) {
        ret = ret.trim_matches(sep).to_string();
    }

    // Last, replace a leading period with an underscore so it isn't hidden
    if ret.starts_with(".") {
        let tail = &ret[1..];
        let mut tmp = "_".to_string();
        tmp.push_str(tail);
        ret = tmp;
    }

    return ret;
}

#[test]
fn test_sanitize_path() {
    assert_eq!("cd", sanitize_path("cd", '-'));
    assert_eq!("usr-bin-true", sanitize_path("/usr/bin/true", '-'));
    assert_eq!("usr-bin-dir", sanitize_path("/usr/bin/dir/", '-'));
    assert_eq!("_-monkey.py", sanitize_path("./monkey.py", '-'));
    assert_eq!("_.-..-monkey.py", sanitize_path("../../monkey.py", '-'));
}

#[test]
fn test_basename() {
    assert_eq!("cat", basename("/bin/cat"));
    assert_eq!("cd", basename("cd"));
    assert_eq!("e", basename("/a/b/../d/e"));
    assert_eq!("a", basename("./a"));
}

#[test]
fn test_format_ts() {
    assert_eq!("Fri, 14 Jul 2017 02:40:00 +0000", format_ts(1_500_000_000.0));
    assert_eq!("Thu, 1 Jan 1970 00:00:00 +0000", format_ts(0.0));
    assert_eq!("Wed, 31 Dec 1969 23:59:59 +0000", format_ts(-1.0));
}
