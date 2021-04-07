extern crate chrono;
#[macro_use]
extern crate clap;
#[macro_use]
extern crate log;
extern crate signal_hook;

use clap::{ArgMatches, App, Arg};
use std::sync::Arc;
use std::process::exit;
use std::thread;
use std::env;
use signal_hook::iterator::Signals;
use signal_hook::consts::signal::{SIGINT, SIGHUP, SIGTERM};

mod lib;
use lib::manager::RunManager;

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
fn get_args() -> ArgMatches<'static> {
    let matches = App::new(crate_name!())
        .version(crate_version!())
        .author("Jay Deiman")
        .about(crate_description!())
        .set_term_width(80)
        .arg(Arg::with_name("state_dir")
            .short("-d")
            .long("--state-directory")
            .default_value("/var/tmp")
            .help("The directory to write the state file to")
        )
        .arg_from_usage("-F, --lock-file [FILE] 'Set a specific lock file \
            to use. The default is to generate one, but this can be useful \
            if you have different jobs that can't run concurrently.'"
        )
        .arg(Arg::with_name("num_retries")
            .default_value("0")
            .short("-r")
            .long("--num-retries")
            .value_name("INT")
            .help("The number of times to retry this if a previous instance \
                is running. This will try every '-s' seconds if this is \
                greater than zero")
        )
        .arg(Arg::with_name("retry_secs")
            .short("-s")
            .long("--retry-seconds")
            .default_value("10")
            .value_name("SECS")
            .help("The number of seconds between retries if locked")
        )
        .arg_from_usage("-i, --ignore-retry-fails 'Ignore the failures which \
            occur becuase this tried to while a previous instance was still \
            running. Basically, an error will not be printed if the number \
            of run retries were exceeded."
        )
        .arg(Arg::with_name("num_fails")
            .short("-n")
            .long("--num-fails")
            .default_value("1")
            .value_name("INT")
            .help("The number of consecutive failures that must occur before \
                a report is printed"
            )
        )
        .arg_from_usage("-f, --first-fail 'The default is to print a failure \
            report only when a multiple of the threshold is reached.  If \
            this is set, a report will *also* be generated on the 1st failure"
        )
        .arg_from_usage("-b, --backoff 'Instead of generating a report every \
            '-n' failures, if this is set, a report is generated at a \
            decaying rate.  If you set '--num-fails' to 3, then a report is \
            produced at 3, 6, 12, 24... failures.")
        .arg_from_usage("-p, --path [PATH] 'Use this for the PATH variable \
            instead of the default'"
        )
        .arg_from_usage("-g, --bash-string 'If this flag is set, it signals \
            that the command passed in should be run in a subshell as a \
            single string.  This is useful for commands that include a '|' \
            or similar character.  Ex: `cat /tmp/file | grep stuff`"
        )
        .arg(Arg::with_name("timeout")
            .short("-t")
            .long("--timeout")
            .default_value("0")
            .value_name("SECS")
            .help("The number of seconds to allow the command to run before \
                timing it out.  If set to zero (default), timeouts are \
                disabled"
            )
        )
        .arg(Arg::with_name("fuzz")
            .short("-z")
            .long("--fuzz")
            .default_value("0")
            .value_name("SECS")
            .help("This will add a random sleep between 0 and N seconds before \
                executing the command.  Note that '--timeout' only pertains \
                to command execution time."
            )
        )
        .arg_from_usage("-q, --quiet 'Only output error reports. If the \
            command runs successfully, nothing will be printed, even if the \
            command had stdout or stderr output."
        )
        .arg_from_usage("-S, --syslog 'If this is set, it will log *all* \
            failures to syslog.  This is useful for diagnosing intermittent \
            failures that don't necessarily trip the number of failures for a \
            report"
        )
        .arg(Arg::with_name("syslog_fac")
            .short("-C")
            .long("--syslog-facility")
            .default_value("log_local7")
            .value_name("FACILITY")
            .help("Set the logging facility.  The list of available facilities \
                is here: http://t.ly/2nqs"
            )
        )
        .arg(Arg::with_name("syslog_pri")
            .short("-P")
            .long("--syslog-priority")
            .default_value("log_info")
            .value_name("PRIORITY")
            .help("Set the syslog priority")
        )
        .arg_from_usage("-D, --debug 'Turn on debug output'")
        .arg(Arg::with_name("CMD")
            .required(true)
            .index(1)
            .help("The command to run.  This can be a string passed to bash \
                if '-g' is set."
            )
        )
        .arg(Arg::with_name("ARGS")
            .required(false)
            .multiple(true)
            .help("The arguments for the CMD")
        )
        .get_matches();

    return matches;
}

/// Set the global logger from the `log` crate
fn setup_logging(args: &ArgMatches) {
    let l = if args.is_present("debug") {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };

    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(l);
}

fn main() {
    let args = Arc::new(get_args());
    setup_logging(&args);

    if let Some(p) = args.value_of("path") {
        env::set_var("PATH", p);
    }

    let mut mgr = RunManager::new(args.clone());
    let statefile = mgr.get_statefile_clone();

    // Setup signals after the manager to handle the signals and unlock in
    // the manager
    let signals = Signals::new(&[SIGINT, SIGTERM, SIGHUP]);
    thread::spawn(move || {
        for _ in signals {
            statefile.unlock().ok();
        }
    });

    if let Err(e) = mgr.lock() {
        if !args.is_present("ignore-retry-fails") {
            error!(
                "Could not get lock to run instance in {} retries: {}",
                value_t!(args, "num_retries", u32).unwrap(),
                e,
            );
            exit(1);
        }
    }

    mgr.run_instance();
    
    if let Err(e) = mgr.unlock() {
        error!("Failed to unlock this instance: {}", e);
        exit(1);
    }
}
