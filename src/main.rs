#[macro_use] extern crate clap;
#[macro_use] extern crate log;

use clap::Parser;
use core::arch;
use std::sync::Arc;
use std::process::exit;
use std::thread;
use std::env;
use signal_hook::iterator::Signals;
use signal_hook::consts::signal::{SIGINT, SIGHUP, SIGTERM};

mod wlib;
use wlib::manager::RunManager;

#[derive(Parser, Debug)]
#[command(
    name=crate_name!(),
    author=crate_authors!(),
    version=crate_version!(),
    about=crate_description!(),
    long_about=None
)]
struct Args {
    /// The directory to write the state file to
    #[arg(short='d', long="state_directory", default_value="/var/tmp")]
    state_dir: String,
    /// Set a specific lock file to use. The default is to generate one,
    /// but this can be useful if you have different jobs that can't run concurrently.
    #[arg(short='F', long)]
    lock_file: Option<String>,
    /// The number of times to retry this if a previous instance is running.
    /// This will try every '-s' seconds if this is greater than zero.
    #[arg(short='r', long, default_value=0)]
    num_retries: usize,
    /// The number of seconds between retries if locked
    #[arg(short='s', long, default_value=10)]
    retry_secs: usize,
    /// Ignore the failures which occur because this tried
    /// to run while a previous instance was still running.
    #[arg(short, long)]
    ignore_retry_fails: bool,
    /// The number of consecutive failures that must occur
    /// before a report is printed.
    #[arg(short, long, default_value=1)]
    num_fails: usize,
    /// The default is to print a failure report only when a
    /// multiple of the threshold. If this is set, a report will
    /// *also* be generated on the 1st failure
    #[arg(short, long)]
    first_fail: bool,
    /// Instead of generating a report every '-n' failures, if this is set, 
    /// a report is generated at a decaying rate.  If you set '--num-fails'
    /// to 3, then a report is produced at 3, 6, 12, 24... failures.
    #[arg(short, long)]
    backoff: bool,
    /// Use this for the PATH variable instead of the default.
    #[arg(short, long)]
    path: Option<String>,
    /// If this flag is set, it signals that the command passed in
    /// should be run in a subshell as a single string.  This is useful for
    /// commands that include a '|' or similar character.
    /// Ex: `cat /tmp/file | grep stuff`"
    #[arg(short='g', long)]
    bash_string: bool,
    /// The number of seconds to allow the command to run before timing it out.
    /// If set to zero (default), timeouts are disabled.
    #[arg(short, long, default_value=0)]
    timeout: usize,
    /// This will add a random sleep between 0 and N seconds before
    /// executing the command.  Note that '--timeout' only pertains
    /// to command execution time.
    #[arg(short='z', long, default_value=0)]
    fuzz: usize,
    /// Only output error reports. If the command runs successfully,
    /// command runs successfully, nothing will be printed, even if
    /// the command had stdout or stderr output.
    #[arg(short, long)]
    quiet: bool,
    /// If this is set, it will log *all* failures to syslog.
    /// This is useful for diagnosing intermittent failures that don't
    /// necessarily trip the number of failures for a report
    #[arg(short='S', long)]
    syslog: bool,
    /// Set the logging facility.  The list of available facilities is here: http://t.ly/2nqs
    #[arg(short='C', long="syslog_facility", default_value="log_local7")]
    syslog_fac: String,
    /// Set the syslog priority
    #[arg(short='P', long="syslog_priority", default_value="log_info")]
    syslog_pri: String,
    /// The command and its arguments to run
    #[arg()]
    cmd: Vec<String>,
    /// Turn on debug output
    #[arg(short='D', long)]
    debug: bool,
}

static LOGGER: GlobalLogger = GlobalLogger;

struct GlobalLogger;

/// This implements the logging to stderr from the `log` crate
impl log::Log for GlobalLogger {
    fn enabled(&self, meta: &log::Metadata) -> bool {
        return meta.level() <= log::max_level();
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            let d = chrono::Local::now();
            eprintln!(
                "{} - {} - {}:{} {} - {}",
                d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                record.level(),
                record.file().unwrap(),
                record.line().unwrap(),
                record.target(),
                record.args(),
            );
        }
    }

    fn flush(&self) {}
}

/// Create a set of CLI args via the `clap` crate and return the matches
fn get_args() -> Args {
    return Args::parse();
}

/// Set the global logger from the `log` crate
fn setup_logging(args: &ArgMatches) {
    let l = if args.debug {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };

    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(l);
}

fn main() {
    let args = get_args();
    setup_logging(&args);

    if let Some(p) = args.path {
        env::set_var("PATH", p);
    }

    let mut mgr = RunManager::new(&args);
    let statefile = mgr.get_statefile_clone();

    // Setup signals after the manager to handle the signals and unlock in
    // the manager
    let mut signals = Signals::new(&[SIGINT, SIGTERM, SIGHUP]).ok().unwrap();
    thread::spawn(move || {
        for sig in signals.pending() {
            debug!("Received signal {}, exiting", sig);
            statefile.unlock().ok();
        }
    });

    mgr.run_instance(true);
    
    if let Err(e) = mgr.unlock() {
        error!("Failed to unlock this instance: {}", e);
        exit(1);
    }
}
