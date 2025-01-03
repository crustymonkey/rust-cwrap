#[macro_use]
extern crate clap;
#[macro_use]
extern crate log;

use clap::Parser;
use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::Signals;
use std::env;
use std::path::PathBuf;
use std::process::exit;
use std::thread;

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
    #[arg(short = 'd', long, default_value = "/var/tmp")]
    state_dir: String,
    /// Set a specific lock file to use. The default is to generate one,
    /// but this can be useful if you have different jobs that can't run concurrently.
    #[arg(short = 'F', long)]
    lock_file: Option<String>,
    /// The number of times to retry this if a previous instance is running.
    /// This will try every '-s' seconds if this is greater than zero.
    #[arg(short = 'r', long, default_value_t = 0, help_heading = "FAIL OPTS")]
    num_retries: usize,
    /// The number of seconds between retries if locked
    #[arg(short = 's', long, default_value_t = 10, help_heading = "FAIL OPTS")]
    retry_secs: usize,
    /// Ignore the failures which occur because this tried
    /// to run while a previous instance was still running.
    #[arg(short, long, help_heading = "FAIL OPTS")]
    ignore_retry_fails: bool,
    /// The number of consecutive failures that must occur
    /// before a report is printed.
    #[arg(short, long, default_value_t = 1, help_heading = "FAIL OPTS")]
    num_fails: usize,
    /// The default is to print a failure report only when a
    /// multiple of the threshold. If this is set, a report will
    /// *also* be generated on the 1st failure
    #[arg(short, long, help_heading = "FAIL OPTS")]
    first_fail: bool,
    /// Instead of generating a report every '-n' failures, if this is set,
    /// a report is generated at a decaying rate.  If you set '--num-fails'
    /// to 3, then a report is produced at 3, 6, 12, 24... failures.
    #[arg(short, long, help_heading = "FAIL OPTS")]
    backoff: bool,
    /// Use this for the PATH variable instead of the default.
    #[arg(short, long)]
    path: Option<String>,
    /// If this flag is set, it signals that the command passed in
    /// should be run in a subshell as a single string.  This is useful for
    /// commands that include a '|' or similar character.
    /// Ex: `cat /tmp/file | grep stuff`"
    #[arg(short = 'g', long)]
    bash_string: bool,
    /// The number of seconds to allow the command to run before timing it out.
    /// If set to zero (default), timeouts are disabled.
    #[arg(short, long, default_value_t = 0, help_heading = "FAIL OPTS")]
    timeout: usize,
    /// This will add a random sleep between 0 and N seconds before
    /// executing the command.  Note that '--timeout' only pertains
    /// to command execution time.
    #[arg(short = 'z', long, default_value_t = 0)]
    fuzz: usize,
    /// Only output error reports. If the command runs successfully,
    /// nothing will be printed, even if the command had stdout or stderr output.
    #[arg(short, long)]
    quiet: bool,
    /// If this is set, it will log *all* failures to syslog.
    /// This is useful for diagnosing intermittent failures that don't
    /// necessarily trip the number of failures for a report
    #[arg(short = 'S', long, help_heading = "SYSLOG")]
    syslog: bool,
    /// Set the logging facility.  The list of available facilities is here: http://t.ly/2nqs
    #[arg(short = 'C', long, help_heading = "SYSLOG", default_value = "log_local7")]
    syslog_fac: String,
    /// Set the syslog priority
    #[arg(short = 'P', long, help_heading = "SYSLOG", default_value = "log_info")]
    syslog_pri: String,
    /// Send an email directly from within cwrap itself.  This option is *required*
    /// with any of the SMTP options below this.  If this is not specified, any
    /// email options below will be ignored.  Note that this can be used with
    /// --also-normal-output to also output to stdout (default: email only).
    #[arg(short = 'M', long, help_heading = "EMAIL")]
    send_mail: bool,
    /// If specified along with --send-mail, cwrap will output to stdout *and*
    /// send a notification email.  The default with --send-mail is to send an
    /// email ONLY.
    #[arg(short = 'N', long, help_heading = "EMAIL")]
    also_normal_output: bool,
    /// The email address to use as the sending address.  If not specified, this
    /// will be set to the username and determined hostname.  It's advised that
    /// this is set.
    #[arg(short = 'E', long, help_heading = "EMAIL")]
    email_from: Option<String>,
    /// The recipient(s) to send the email to.  This can be specified multiple
    /// times to send to multiple addresses.
    #[arg(short = 'R', long, help_heading = "EMAIL")]
    recipient: Option<Vec<String>>,
    /// The subject to use for the email.
    #[arg(short = 'J', long, help_heading = "EMAIL", default_value = "cwrap failure report")]
    subject: String,
    /// The SMTP server address (hostname or IP) to connect to.
    #[arg(short = 'X', long, help_heading = "EMAIL", default_value = "localhost")]
    smtp_server: String,
    /// The SMTP port to connect to.
    #[arg(short = 'T', long, help_heading = "EMAIL", default_value_t = 25)]
    smtp_port: usize,
    /// Encrypt the connection using SSL/TLS directly.  Note that the port you
    /// connect to should expect a TLS connection (as opposed to STARTTLS).
    #[arg(short = 'L', long = "tls", help_heading = "EMAIL")]
    tls: bool,
    /// Encrypt the connection to the server using STARTTLS.  This is highly
    /// recommended unless you are using the default localhost connection.
    #[arg(short = 'Z', long = "starttls", help_heading = "EMAIL")]
    starttls: bool,
    /// The username to use for SMTP authentication.
    #[arg(short = 'U',long, help_heading = "EMAIL")]
    username: Option<String>,
    /// The password to use for SMTP authentication.
    #[arg(short = 'W', long, help_heading = "EMAIL")]
    password: Option<String>,
    /// (Recommended) Specify the path to a a credentials file instead of
    /// specifying a username and password directly.  The file should simply
    /// have the SMTP credentials in the form of USERNAME:PASSWORD as the only
    /// contents. Note that the username/password must be utf-8 or this will
    /// crash.
    #[arg(short = 'Y', long, help_heading = "EMAIL")]
    creds_file: Option<PathBuf>,
    /// The command to run.  This can be a single string (enclosed in quotes)
    /// passed to bash if "-g" is set or the command and it's arguments.
    cmd: Vec<String>,
    /// Turn on debug output
    #[arg(short = 'D', long)]
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
fn setup_logging(args: &Args) {
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

    if let Some(p) = &args.path {
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
